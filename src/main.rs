use axum::body::Body;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode, header};
use axum::http::{Request, Response, Uri};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Redirect, Sse};
use axum::routing::post;
use axum::{Json, Router, routing::get};
use axum_extra::extract::cookie::CookieJar;
use futures_util::{Stream, StreamExt};
use include_dir::Dir as IncludedDir;
use include_dir::{File, include_dir};
use mc_rpc::{Client, ClientConfig, Player};
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::fmt::format;
use std::fs;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{broadcast, watch, Notify};
use tokio::sync::broadcast::Sender;
use tower::util::ServiceExt;
use tower_http::services::ServeFile;
use tower_http::services::fs::ServeDir;

enum MinecraftStatus {
    Online { command_tx: tokio::sync::mpsc::Sender<String>, manager: Client, players: Vec<String> },
    Offline,
    Starting,
    Stopping,
    CrashedRebooting(String),
    RunningWithoutManager,
}

struct AppState {
    status: MinecraftStatus,
    event_transmitter: OnceLock<Sender<String>>,
    logs_transmitter: OnceLock<Sender<String>>,
}

#[derive(Deserialize, Serialize, Debug)]
struct AppConfig {
    no_share: String,
    secret: String,
    auto_restart: bool,
    execute_command: String,
    execute_command_arg: String,
    management_port: i32,
    management_secret: String,
}

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

static STATE: Mutex<AppState> = Mutex::new(AppState {
    status: MinecraftStatus::Offline,
    event_transmitter: OnceLock::new(),
    logs_transmitter: OnceLock::new(),
});

const PAGES_DIR: IncludedDir = include_dir!("$CARGO_MANIFEST_DIR/src/public");

#[tokio::main]
async fn main() {
    let default_config = AppConfig {
        secret: Alphanumeric.sample_string(&mut rand::rng(), 32),
        auto_restart: true,
        no_share: "DO NOT SHARE YOUR SECRET!!!".parse().unwrap(),
        execute_command: "bash".parse().unwrap(),
        execute_command_arg: "start.sh".parse().unwrap(),
        management_port: 1234,
        management_secret: Alphanumeric.sample_string(&mut rand::rng(), 40),
    };

    match fs::read_to_string("config.json") {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(c) => {
                CONFIG.set(c).expect("Config was already set???!!!");
            }
            Err(_) => {
                CONFIG
                    .set(default_config)
                    .expect("Config was already set???!!!");
            }
        },
        Err(_) => {
            CONFIG
                .set(default_config)
                .expect("Config was already set???!!!");
        }
    }
    fs::write(
        "config.json",
        serde_json::to_string_pretty(&CONFIG.get()).expect("Config could not be serialized"),
    )
    .expect("Config could not be saved");

    match fs::read_to_string("server.properties") {
        Ok(content) => {
            let lines = content.split("\n").collect::<Vec<&str>>();
            let mut out: Vec<String> = vec![];
            for line in lines {
                if line.starts_with("management-server-enabled=") {
                    out.push("management-server-enabled=true".to_string());
                } else if line.starts_with("management-server-allowed-origins=") {
                    out.push("management-server-allowed-origins=localhost".to_string());
                } else if line.starts_with("management-server-port=") {
                    out.push(format!(
                        "management-server-port={}",
                        CONFIG.get().unwrap().management_port.to_string()
                    ));
                } else if line.starts_with("management-server-tls-enabled=") {
                    out.push("management-server-tls-enabled=false".to_string());
                } else if line.starts_with("management-server-secret=") {
                    out.push(format!(
                        "management-server-secret={}",
                        CONFIG.get().unwrap().management_secret
                    ));
                } else {
                    out.push(line.to_string());
                }
            }
            fs::write("server.properties", out.join("\n"))
                .expect("Server properties could not be saved");
        }
        Err(_) => {}
    }

    let transmitters = (broadcast::channel::<String>(100).0, broadcast::channel::<String>(100).0);

    let app = Router::new()
        .route("/testing/{e}", get(|| async { "Hello, World! {body}" }))
        .route("/test", post(create_user))
        .route("/api/login", post(api_login))
        .route("/api/live_logs", get(api_live_logs))
        .route("/api/live_updates", get(api_live_updates))
        .route("/api/start_server", post(api_start))
        .route("/api/stop_server", post(api_stop))
        .route("/api/send_command", post(api_send_command))
        .fallback_service(get(static_html))
        .with_state(transmitters.clone());
    include_bytes!("public/main.css");

    {
        let state = STATE.lock().unwrap();
        state.event_transmitter.set(transmitters.0).expect("");
        state.logs_transmitter.set(transmitters.1).expect("");
    }

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app)
        .await
        .expect("Error hosting webserver");
}

#[derive(Deserialize)]
struct CreateUserPayload {
    val: i32,
    enabled: bool,
}

async fn static_html(cookies: CookieJar, req: Request<Body>) -> Response<Body> {
    let uri = req.uri().clone();
    let mut path = uri.path().to_string().split_at(1).1.to_string(); //get rid of the / at the start
    if path.eq("login") || path.eq("main.css") {
        //Dont auth
    } else if let Some(out) = check_auth(cookies, Some(uri)) {
        return out;
    }
    let mut content_type = "";
    if (!path.contains(".")) {
        path = format!("{}.html", path);
        content_type = "text/html";
    }
    if (PAGES_DIR.contains(path.clone())) {
        match PAGES_DIR.get_file(path) {
            None => {}
            Some(v) => {
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type)],
                    v.contents(),
                )
                    .into_response();
            }
        }
    }
    (StatusCode::NOT_FOUND, "File not found!").into_response()
}

async fn create_user(cookies: CookieJar, Json(payload): Json<CreateUserPayload>) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    (
        StatusCode::OK,
        format!("{}, {}", payload.val, payload.enabled),
    )
        .into_response()
}

async fn api_login(payload: String) -> Response<Body> {
    if payload.eq(&CONFIG.get().unwrap().secret) {
        let mut out = (StatusCode::OK, "").into_response();
        out.headers_mut().append(
            SET_COOKIE,
            HeaderValue::from_str(
                format!(
                    "secret={}; Max-Age=86400; Path=/; Secure; HttpOnly; SameSite=Strict",
                    &CONFIG.get().unwrap().secret
                )
                .as_str(),
            )
            .unwrap(),
        );
        return out;
    } else {
        (StatusCode::UNAUTHORIZED, "Invalid secret").into_response()
    }
}

async fn api_start(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    {
        let mut state = STATE.lock().unwrap();
        match state.status {
            MinecraftStatus::Offline => {}
            _ => return (StatusCode::BAD_REQUEST, "Server already running").into_response(),
        }
    }

    tokio::spawn(async move {
        println!("Starting server");

        let mut output = Command::new(CONFIG.get().unwrap().execute_command.clone())
            .arg(CONFIG.get().unwrap().execute_command_arg.clone())
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .expect("Failed to start server");
        {
            let mut state = STATE.lock().unwrap();
            state.status = MinecraftStatus::Starting;
        }
        on_status_edited();

        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel::<String>(32);
        let t = output.stdout.take().expect("Unable to get stdout");
        let reader = BufReader::new(t);
        let mut lines = reader.lines();

        let mut has_started = false;
        let mut about_to_stop = false;


        tokio::spawn(async move {
            let mut stdin = output.stdin.take().expect("Unable to get stdin");

            loop {
                if let Some(command) = command_rx.recv().await {
                    stdin.write_all(format!("{}\n", command).as_bytes()).await.expect("Failed to write command");
                }
            }
        });

        while let Some(line) = lines.next_line().await.expect("Failed to read line") {
            if(!line.contains("Named entity") && !line.contains("<")) {
                //This is fine even due to 'chat injections' as only mods can output at this time
                if !has_started
                    && line.contains("Done (")
                    && line.ends_with(")! For help, type \"help\"")
                {
                    has_started = true;
                    println!("Server started!");
                    let client = Client::new(
                        format!("ws://localhost:{}", &CONFIG.get().unwrap().management_port),
                        ClientConfig::with_bearer(&CONFIG.get().unwrap().management_secret),
                    )
                        .await;
                    {
                        let mut state = STATE.lock().unwrap();
                        let client = client.expect("Couldn't start management service");
                        state.status = MinecraftStatus::Online {
                            players: vec![],
                            manager: client,
                            command_tx: command_tx.clone(),
                        };
                    }
                    let manager = {
                        let state = STATE.lock().unwrap();

                        match &state.status {
                            MinecraftStatus::Online { manager, .. } => manager.clone(),
                            _ => return,
                        }
                    };
                    // manager
                    //     .allowlist_add(&vec![Player {
                    //         id: "3039dead-e198-4ab3-8384-fa0739d712f5".to_string(),
                    //         name: "Daniel99j".to_string(),
                    //     }])
                    //     .await
                    //     .unwrap();
                    on_status_edited();
                    tokio::spawn(async move {
                        let manager = {
                            let state = STATE.lock().unwrap();

                            match &state.status {
                                MinecraftStatus::Online { manager, .. } => manager.clone(),
                                _ => return,
                            }
                        };

                        //For some reason the server/ notifications dont work so I have to rely on logs!
                        let mut join = manager.notification_players_joined().await.unwrap();
                        let mut leave = manager.notification_players_left().await.unwrap();



                        loop {
                            tokio::select! {
                                Some(v) = join.next() => {
                                    println!("Join2");
                                    {
                                        let mut state = STATE.lock().unwrap();

                                        match &mut state.status {
                                            MinecraftStatus::Online { players, .. } => {
                                                players.push(v.unwrap().unwrap().get(0).unwrap().name.clone());
                                            },
                                            _ => return,
                                        }
                                    }
                                    on_status_edited();
                                }
                                Some(v) = leave.next() => {
                                    println!("Leave");
                                    let name = v.unwrap().unwrap().get(0).unwrap().name.clone();
                                    {
                                        let mut state = STATE.lock().unwrap();

                                        match &mut state.status {
                                            MinecraftStatus::Online { players, .. } => {
                                                players.retain(|a| !a.eq(&name));
                                            },
                                            _ => return,
                                        }
                                    }
                                    on_status_edited();
                                }
                            }
                        }
                    });
                }
                if(line.ends_with("[Server thread/INFO]: Stopping server")) {
                    about_to_stop = true;
                    {
                        let mut state = STATE.lock().unwrap();
                        state.status = MinecraftStatus::Stopping;
                    }
                    on_status_edited();
                }
                //Its always #1 as no other managers should be running
                //This is the last thing printed before shutdown
                if (about_to_stop && line.contains("[Management server IO #1/INFO]: RPC Connection #1: Management connection closed for /127.0.0.1:")) {
                    {
                        let mut state = STATE.lock().unwrap();
                        state.status = MinecraftStatus::Offline;
                    }
                    on_status_edited();
                }
            }
            println!("Server: {}", line);

            {
                let state = STATE.lock().unwrap();
                if let Some(transmitter) = state.logs_transmitter.get() {
                    let _ = transmitter.send(line);
                }
            }
        }
    });

    (StatusCode::OK, "").into_response()
}

async fn api_stop(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    let manager = {
        let state = STATE.lock().unwrap();

        match &state.status {
            MinecraftStatus::Online { manager, .. } => manager.clone(),
            _ => {
                return (StatusCode::BAD_REQUEST, "Server not running",).into_response();
            }
        }
    };

    manager.server_stop().await.expect("Failed to stop server");
    (StatusCode::OK, "").into_response()
}

async fn api_send_command(cookies: CookieJar, payload: String) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    let command_tx = {
        let state = STATE.lock().unwrap();

        match &state.status {
            MinecraftStatus::Online { command_tx, .. } => command_tx.clone(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Server not running",
                )
                    .into_response();
            }
        }
    };

    command_tx
        .send(payload.clone())
        .await
        .expect("Couldn't send payload");

    println!("Running command {}", payload.clone());


    (StatusCode::OK, "").into_response()
}

async fn api_live_updates(
    State(transmitter): State<(Sender<String>, Sender<String>)>,
    cookies: CookieJar,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    if let Some(_) = check_auth(cookies, None) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let mut rx = transmitter.0.subscribe();

    let stream = async_stream::stream! {
        yield Ok(Event::default()
            .data(create_status()));

        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn api_live_logs(
    State(transmitter): State<(Sender<String>, Sender<String>)>,
    cookies: CookieJar,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    if let Some(_) = check_auth(cookies, None) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let mut rx = transmitter.1.subscribe();

    let stream = async_stream::stream! {
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn check_auth(cookies: CookieJar, uri: Option<Uri>) -> Option<Response<Body>> {
    if let Some(val) = cookies.get("secret") {
        if val.value().eq(CONFIG.get().unwrap().secret.as_str()) {
            return None;
        }
    }
    if uri.is_some() {
        Some(
            Redirect::to(
                format!(
                    "/login?redirect={}",
                    urlencoding::encode(uri.expect("It does exist!").path())
                )
                .as_str(),
            )
            .into_response(),
        )
    } else {
        Some(StatusCode::UNAUTHORIZED.into_response())
    }
}

#[derive(Serialize)]
struct SharedData {
    status: String,
    status_colour: String,
    status_online: bool,
    players_online: i32,
    players: Vec<String>,
    tps: f32,
}

fn create_status() -> String {
    let out;
    {
        let state = STATE.lock().unwrap();
        let online: Vec<String> = match &state.status {
            MinecraftStatus::Online { players, .. } => {
                players.clone()
            },
            _ => {
                vec![]
            }
        };
        out = serde_json::to_string(&SharedData {
            status: match state.status {
                MinecraftStatus::Online { .. } => String::from("Online"),
                MinecraftStatus::CrashedRebooting(..) => String::from("Crashed!"),
                MinecraftStatus::Offline => String::from("Offline"),
                MinecraftStatus::Stopping => String::from("Stopping"),
                MinecraftStatus::Starting => String::from("Starting"),
                MinecraftStatus::RunningWithoutManager => String::from("Running without manager"),
            },
            status_colour: match state.status {
                MinecraftStatus::Online { .. } => String::from("#2dcf58"),
                MinecraftStatus::CrashedRebooting(..) => String::from("#cf332d"),
                MinecraftStatus::Offline => String::from("#4d4d4d"),
                MinecraftStatus::Stopping => String::from("#d9a300"),
                MinecraftStatus::Starting => String::from("#d9a300"),
                MinecraftStatus::RunningWithoutManager => String::from("#4287f5"),
            },
            status_online: match state.status {
                MinecraftStatus::Online { .. } => true,
                MinecraftStatus::Stopping => true,
                MinecraftStatus::Starting => true,
                MinecraftStatus::RunningWithoutManager => true,
                _ => false,
            },
            players_online: online.len() as i32,
            players: online,
            tps: 20.0,
        })
        .unwrap();
    }
    out
}

fn on_status_edited() {
    println!("Sending status");
    let sending = create_status();
    let state = STATE.lock().unwrap();
    if let Some(transmitter) = state.event_transmitter.get() {
        let _ = transmitter.send(sending);
    }
}
