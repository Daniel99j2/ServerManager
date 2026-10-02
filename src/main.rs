mod manage_server;
mod get_info;

use axum::body::Body;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode, header};
use axum::http::{Request, Response, Uri};
use axum::response::{IntoResponse, Redirect};
use axum::routing::post;
use axum::{Router, routing::get};
use axum_extra::extract::cookie::CookieJar;
use include_dir::Dir as IncludedDir;
use include_dir::{include_dir};
use mc_rpc::{Client};
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast};
use tokio::sync::broadcast::Sender;
use tokio::time::sleep;

enum MinecraftStatus {
    Offline,
    Starting { stop_tx: tokio::sync::mpsc::Sender<String>, log_line: i64 },
    Online { command_tx: tokio::sync::mpsc::Sender<String>, stop_tx: tokio::sync::mpsc::Sender<String>, manager: Client, players: Vec<String>, tps: f32, rcon: mc_rcon::RconClient, log_line: i64},
    Stopping { stop_tx: tokio::sync::mpsc::Sender<String> },
    Crashed,
}

struct AppState {
    status: MinecraftStatus,
    event_transmitter: OnceLock<Sender<String>>,
    logs_transmitter: OnceLock<Sender<String>>,
    max_players: OnceLock<i32>,
    reboot_requested: bool,
    last_crash: Option<Instant>
}

#[derive(Deserialize, Serialize, Debug)]
struct AppConfig {
    no_share: String,
    secret: String,
    auto_restart: bool,
    execute_command: String,
    execute_command_arg: String,
    site_port: i32,
    tps_update_interval_seconds: u64,
    management_port: i32,
    management_secret: String,
    rcon_port: i32,
    rcon_password: String,
    manager_discord_webhook: String,
    anti_crash_loop_time_seconds: u64
}

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

static STATE: Mutex<AppState> = Mutex::new(AppState {
    status: MinecraftStatus::Offline,
    event_transmitter: OnceLock::new(),
    logs_transmitter: OnceLock::new(),
    max_players: OnceLock::new(),
    reboot_requested: false,
    last_crash: None
});

const PAGES_DIR: IncludedDir = include_dir!("$CARGO_MANIFEST_DIR/src/gen/");

#[tokio::main]
async fn main() {
    let default_config = AppConfig {
        secret: Alphanumeric.sample_string(&mut rand::rng(), 32),
        auto_restart: true,
        tps_update_interval_seconds: 60,
        no_share: "DO NOT SHARE YOUR CONFIG OR SECRETS!!!".parse().unwrap(),
        execute_command: "bash".parse().unwrap(),
        execute_command_arg: "start.sh".parse().unwrap(),
        site_port: 80,
        management_port: 25585,
        management_secret: Alphanumeric.sample_string(&mut rand::rng(), 40),
        rcon_port: 25575,
        rcon_password: Alphanumeric.sample_string(&mut rand::rng(), 40),
        manager_discord_webhook: "".parse().unwrap(),
        anti_crash_loop_time_seconds: 180
    };

    match fs::read_to_string("manager/config.json") {
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
        "manager/config.json",
        serde_json::to_string_pretty(&CONFIG.get()).expect("Config could not be serialized"),
    )
    .expect("Config could not be saved");

    if !fs::read_dir("manager").is_ok() {
        fs::create_dir("manager").unwrap();
    }

    let mut max_players = -2;

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
                } else if line.starts_with("enable-rcon=") {
                    out.push("enable-rcon=true".to_string());
                } else if line.starts_with("rcon.password=") {
                    out.push(format!(
                        "rcon.password={}",
                        CONFIG.get().unwrap().rcon_password
                    ));
                } else if line.starts_with("rcon.port=") {
                    out.push(format!(
                        "rcon.port={}",
                        CONFIG.get().unwrap().rcon_port
                    ));
                } else {
                    out.push(line.to_string());
                }
                if line.starts_with("max-players") {
                    max_players = line.replace("max-players=", "").parse().unwrap_or(-1);
                }
            }
            fs::write("server.properties", out.join("\n"))
                .expect("Server properties could not be saved");
        }
        Err(_) => {}
    }

    let transmitters = (broadcast::channel::<String>(100).0, broadcast::channel::<String>(100).0);

    let app = Router::new()
        .route("/api/login", post(api_login))
        .route("/api/live_logs", get(get_info::api_live_logs))
        .route("/api/live_updates", get(get_info::api_live_updates))
        .route("/api/start_server", post(manage_server::api_start))
        .route("/api/stop_server", post(manage_server::api_stop))
        .route("/api/send_command", post(manage_server::api_send_command))
        .route("/api/reboot_server", post(manage_server::api_restart))
        .route("/api/kill_server", post(manage_server::api_kill))
        .route("/api/read_log", post(get_info::api_read_log))
        .route("/api/log_line_count", post(get_info::api_log_line_count))
        .route("/api/all_logs", get(get_info::api_all_logs))
        .fallback_service(get(static_html))
        .with_state(transmitters.clone());
    include_bytes!("public/main.css");

    {
        let state = STATE.lock().unwrap();
        state.event_transmitter.set(transmitters.0).expect("");
        state.logs_transmitter.set(transmitters.1).expect("");
        state.max_players.set(max_players).expect("");
    }

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(CONFIG.get().unwrap().tps_update_interval_seconds as u64)).await;

            {
                let mut state = STATE.lock().unwrap();
                match &mut state.status {
                    MinecraftStatus::Online { players, .. } => {
                        if players.len() == 0 {
                            continue;
                        }
                    },
                    _ => continue,
                }
            }

            let v = run_command("tick query".to_string());
            let lines = v.split("\n").collect::<Vec<&str>>();
            if lines.get(0).unwrap().starts_with("The game is frozen") {
                {
                    let mut state = STATE.lock().unwrap();
                    match &mut state.status {
                        MinecraftStatus::Online { tps, .. } => {
                            *tps = 0.0;
                        }
                        _ => {}
                    }
                }

                on_status_edited();
            } else if lines.get(0).unwrap().starts_with("The game is sprinting") {
                {
                    let mut state = STATE.lock().unwrap();
                    match &mut state.status {
                        MinecraftStatus::Online { tps, .. } => {
                            *tps = 1000.0;
                        }
                        _ => {}
                    }
                }

                on_status_edited();
            } else {
                let line = match lines.get(1) {
                    None => {
                        continue;
                    }
                    Some(v) => {
                        v
                    }
                };
                let prefix = "Average time per tick: ";

                let start = prefix.len();
                let end = line[start..]
                    .find("ms")
                    .map(|i| start + i)
                    .unwrap_or(start);

                let text = &line[start..end];

                let target_start = line.find("(Target: ")
                    .map(|i| i + "(Target: ".len())
                    .unwrap_or(0);

                let target_end = line[target_start..]
                    .find("ms)")
                    .map(|i| target_start + i)
                    .unwrap_or(target_start);

                let text2 = &line[target_start..target_end];

                let tick_rate = 1000.0 / text.parse().unwrap_or(-2.0);
                let target = 1000.0 / text2.parse().unwrap_or(-2.0);
                {
                    let mut state = STATE.lock().unwrap();
                    match &mut state.status {
                        MinecraftStatus::Online { tps, .. } => {
                            *tps = f32::min(target, tick_rate);
                        }
                        _ => {}
                    }
                }

                on_status_edited();
            }
        }
    });

    match fs::read_to_string("manager/previous_state.json") {
        Ok(content) => match serde_json::from_str::<SharedData>(&content) {
            Ok(c) => {
                if c.status_online {
                    let send = "Host rebooted whilst server was online, restarting server!";
                    send_webhook(send.to_string()).await;
                    println!("{}", send);
                    manage_server::start_server();
                }
            }
            Err(_) => {}
        },
        Err(_) => {}
    }

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", CONFIG.get().unwrap().site_port)).await.unwrap();
    axum::serve(listener, app)
        .await
        .expect("Error hosting webserver");
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
    if !path.contains(".") {
        path = format!("{}.html", path);
        content_type = "text/html";
    }
    if path.ends_with(".svg") {
        content_type = "image/svg+xml";
    }

    if PAGES_DIR.contains(path.clone()) {
        match PAGES_DIR.get_file(path) {
            None => {}
            Some(v) => {
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type)],
                    v.contents().to_vec()
                ).into_response();
            }
        }
    }
    (StatusCode::NOT_FOUND, "File not found!").into_response()
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

#[derive(Serialize, Deserialize, Debug)]
struct WebhookRequest {
    content: String
}

async fn send_webhook(text: String) {
    if CONFIG.get().unwrap().manager_discord_webhook.is_empty() {
        println!("E");
        return;
    }
    let client = reqwest::Client::new();

    let new_post = WebhookRequest {
        content: text
    };

    let out = client.post(CONFIG.get().unwrap().manager_discord_webhook.clone())
        .body(serde_json::to_string(&new_post).unwrap())
        .header(header::CONTENT_TYPE, "application/json")
        .send()
        .await;
    match out {
        Ok(_) => {
        }
        Err(_) => {
            println!("Failed to send webhook");
        }
    }
}


fn run_command(command: String) -> String {
    let out;
    {
        let mut state = STATE.lock().unwrap();

        match &mut state.status {
            MinecraftStatus::Online { rcon, .. } => {
                match rcon.send_command(command.as_str()) {
                    Ok(v) => {
                        out = v;
                    }
                    Err(v) => {
                        out = format!("Failed to send command: {}", v);
                    }
                }
            },
            _ => {
                return "Server not running".to_string();
            }
        }
    };
    out
}

fn safe_file(dir_name: &str, file_name: &str) -> Result<Vec<u8>, String> {
    let dir_path = Path::new(dir_name);
    let full_path = dir_path.join(file_name);

    // removes .. or symlinks
    let canonical_dir = match dir_path.canonicalize() {
        Ok(v) => v,
        Err(_) => {
            return Err(format!("Can't canonicalize {}", dir_path.display()));
        }
    };
    let canonical_file = match full_path.canonicalize() {
        Ok(v) => v,
        Err(_) => {
            return Err(format!("Can't canonicalize {}", full_path.display()));
        }
    };

    // it has to start with the intended directory
    if !canonical_file.starts_with(canonical_dir) {
        return Err("File escaped the allowed dir".to_string());
    }

    match fs::read(canonical_file) {
        Ok(v) => Ok(v),
        Err(_) => {
            Err("Failed to read file".to_string())
        }
    }
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

#[derive(Serialize, Deserialize)]
struct SharedData {
    status: String,
    status_colour: String,
    status_online: bool,
    players_online: i32,
    players: Vec<String>,
    max_players: i32,
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
                MinecraftStatus::Crashed => String::from("Crashed!"),
                MinecraftStatus::Offline => String::from("Offline"),
                MinecraftStatus::Stopping { .. } => String::from("Stopping"),
                MinecraftStatus::Starting { .. } => String::from("Starting"),
            },
            status_colour: match state.status {
                MinecraftStatus::Online { .. } => String::from("#2dcf58"),
                MinecraftStatus::Crashed => String::from("#cf332d"),
                MinecraftStatus::Offline => String::from("#4d4d4d"),
                MinecraftStatus::Stopping { .. } => String::from("#d9a300"),
                MinecraftStatus::Starting { .. } => String::from("#d9a300"),
            },
            status_online: match state.status {
                MinecraftStatus::Online { .. } => true,
                MinecraftStatus::Stopping { .. } => true,
                MinecraftStatus::Starting { .. } => true,
                _ => false,
            },
            players_online: online.len() as i32,
            players: online,
            max_players: state.max_players.get().unwrap().clone(),
            tps: match state.status {
                MinecraftStatus::Online { tps, .. } => tps,
                MinecraftStatus::Starting { .. } => 20.0, //So it doesnt show -1 when booting
                _ => -1.0,
            },
        })
        .unwrap();
    }
    out
}

static PREVIOUS_STATUS: Mutex<String> = Mutex::new(String::new());

fn on_status_edited() {
    let mut state = PREVIOUS_STATUS.lock().unwrap();
    let sending = create_status();
    if state.eq(&sending) {
        return;
    }
    *state = sending.clone();
    println!("Sending status");
    let state = STATE.lock().unwrap();
    if let Some(transmitter) = state.event_transmitter.get() {
        let _ = transmitter.send(sending.clone());
    }
    fs::write("manager/previous_state.json", sending)
    .expect("Config could not be saved");
}
