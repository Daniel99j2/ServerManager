use std::process::Stdio;
use std::time::{Duration, Instant};
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum_extra::extract::CookieJar;
use futures_util::StreamExt;
use mc_rpc::{Client, ClientConfig};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::sleep;
use crate::{check_auth, STATE, MinecraftStatus, run_command, on_status_edited, CONFIG, send_webhook};

pub(crate) async fn api_start(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    {
        let state = STATE.lock().unwrap();
        match state.status {
            MinecraftStatus::Offline => {},
            MinecraftStatus::Crashed => {}
            _ => return (StatusCode::BAD_REQUEST, "Server already running").into_response(),
        }
    }

    start_server();

    (StatusCode::OK, "").into_response()
}

pub(crate) async fn api_stop(cookies: CookieJar) -> Response<Body> {
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

pub(crate) async fn api_restart(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }

    let manager = {
        let mut state = STATE.lock().unwrap();

        let out = match &mut state.status {
            MinecraftStatus::Online { manager, .. } => {
                manager.clone()
            },
            _ => {
                return (StatusCode::BAD_REQUEST, "Server not running",).into_response();
            }
        };
        state.reboot_requested = true;

        out
    };

    run_command("kick @a Server Restarting...".to_string());

    manager.server_stop().await.expect("Failed to stop server");

    (StatusCode::OK, "").into_response()
}

pub(crate) async fn api_kill(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    let stop_tx = {
        let state = STATE.lock().unwrap();

        match &state.status {
            MinecraftStatus::Starting { stop_tx, .. } => stop_tx.clone(),
            MinecraftStatus::Online { stop_tx, .. } => stop_tx.clone(),
            MinecraftStatus::Stopping { stop_tx, .. } => stop_tx.clone(),

            _ => {
                return (StatusCode::BAD_REQUEST, "Server not running",).into_response();
            }
        }
    };

    let err = stop_tx.send("STOP".to_string()).await;
    match err {
        Ok(_) => {
            println!("Killed server")
        }
        Err(err) => {
            println!("Failed to kill server: {}", err);
        }
    }

    {
        let mut state = STATE.lock().unwrap();
        state.status = MinecraftStatus::Offline;
        state.reboot_requested = false;
    }
    on_status_edited();

    (StatusCode::OK, "").into_response()
}

pub(crate) async fn api_send_command(cookies: CookieJar, payload: String) -> Response<Body> {
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

pub(crate) fn start_server() {
    tokio::spawn(async move {
        println!("Starting server");

        let mut output = Command::new(CONFIG.get().unwrap().execute_command.clone())
            .arg(CONFIG.get().unwrap().execute_command_arg.clone())
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .expect("Failed to start server");

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<String>(1);
        {
            let mut state = STATE.lock().unwrap();
            state.status = MinecraftStatus::Starting { stop_tx: stop_tx.clone(), log_line: 0 };
            if let Some(transmitter) = state.logs_transmitter.get() {
                let _ = transmitter.send("#### CLEAR MANAGER LOGS ####".to_string());
                let _ = transmitter.send("#### MANAGER LOG LINE: 0 ####".to_string());
            }
        }
        on_status_edited();

        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel::<String>(32);
        let t = output.stdout.take().expect("Unable to get stdout");
        let reader = BufReader::new(t);
        let mut lines = reader.lines();

        let mut has_started = false;
        let mut about_to_stop = false;
        let mut stdin = output.stdin.take().expect("Unable to get stdin");

        tokio::spawn(async move {
            loop {
                if let Some(command) = command_rx.recv().await {
                    stdin.write_all(format!("{}\n", command).as_bytes()).await.expect("Failed to write command");
                }
            }
        });

        loop {
            tokio::select! { biased;
                Some(_) = stop_rx.recv() => {
                    output.kill().await.expect("Failed to send KILL to the process!");
                    println!("K");
                    break;
                }
                v = lines.next_line() => {
                    if v.is_err() {
                        break;
                    }
                    let line = v.unwrap().unwrap();
                                if !line.contains("Named entity") && !line.contains("<") {
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
                    ).await;
                    let rcon = mc_rcon::RconClient::connect(format!("localhost:{}", CONFIG.get().unwrap().rcon_port)).expect("Failed to start RCON client");
                    rcon.log_in(CONFIG.get().unwrap().rcon_password.as_str()).expect("Failed to log into RCON client");

                    {
                        let mut state = STATE.lock().unwrap();
                        let client = client.expect("Couldn't start management service");
                        let mut line = 0;
                        match &mut state.status {
                                MinecraftStatus::Starting {log_line, ..} => {
                                    line = log_line.clone();
                                },
                                _ => {}
                            };
                        state.status = MinecraftStatus::Online {
                            tps: 20.0,
                            players: vec![],
                            manager: client,
                            command_tx: command_tx.clone(),
                            stop_tx: stop_tx.clone(),
                            rcon,
                            log_line: line
                        };
                    }
                    {
                        let state = STATE.lock().unwrap();

                        match &state.status {
                            MinecraftStatus::Online { manager, .. } => manager.clone(),
                            _ => return,
                        }
                    };
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
                if line.ends_with("[Server thread/INFO]: Stopping server") {
                    about_to_stop = true;
                    {
                        let mut state = STATE.lock().unwrap();
                        state.status = MinecraftStatus::Stopping { stop_tx: stop_tx.clone() };
                    }
                    on_status_edited();
                }
                //Its always #1 as no other managers should be running
                //This is the last thing printed before shutdown
                if about_to_stop && line.contains("[Management server IO") && line.contains(": Management connection closed for /127.0.0.1:") {
                    let to_reboot;
                    {
                        let mut state = STATE.lock().unwrap();
                        state.status = MinecraftStatus::Offline;
                        to_reboot = state.reboot_requested;
                        state.reboot_requested = false;
                    }
                    on_status_edited();
                    if to_reboot {
                        start_server()
                    }
                    return;
                }
                if line.eq("---- Minecraft Crash Report ----") || line.contains("[main/ERROR]: Failed to start the minecraft server") {
                    let crashed_recently ;
                    {
                        let mut state = STATE.lock().unwrap();
                        state.status = MinecraftStatus::Crashed;
                        crashed_recently =  state.last_crash.is_some() && state.last_crash.unwrap().elapsed().as_secs() < CONFIG.get().unwrap().anti_crash_loop_time_seconds;
                        state.last_crash = Some(Instant::now());
                    }
                    on_status_edited();

                    if crashed_recently {
                        let text;
                        let value = CONFIG.get().unwrap().anti_crash_loop_time_seconds % 60;
                        if value % 30 == 0 {
                            text = format!("{} minutes", value / 60);
                        } else {
                            text = format!("{} seconds", value)
                        }
                        let s = format!("Server has crashed, but has crashed within the last {}. Not rebooting!", text);
                        println!("{}", s);
                        send_webhook(s).await;
                    } else {
                        let s = "Server has crashed, rebooting!";
                        println!("{}", s);
                        send_webhook(s.to_string()).await;
                        //wait 10 secs so it has fully ended the task
                        sleep(Duration::from_secs(10u64)).await;
                        start_server()
                    }

                    return;
                }
            }
            println!("Server: {}", line);

            {
                let mut state = STATE.lock().unwrap();
                if let Some(transmitter) = state.logs_transmitter.get() {
                    let _ = transmitter.send(line);
                }
                match &mut state.status {
                    MinecraftStatus::Starting { log_line, .. } => {
                        *log_line += 1;
                    },
                MinecraftStatus::Online { log_line, .. } => {
                        *log_line += 1;
                    },
                    _ => {},
                }
            }
                }
            }
        }
    });
}