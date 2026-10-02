use std::cmp::{min};
use std::convert::Infallible;
use std::fs;
use std::io::Read;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::Json;
use axum::response::{IntoResponse, Sse};
use axum::response::sse::{Event, KeepAlive};
use axum_extra::extract::CookieJar;
use flate2::read::GzDecoder;
use futures_util::Stream;
use serde::Deserialize;
use tokio::sync::broadcast::Sender;
use crate::{check_auth, create_status, safe_file, STATE, MinecraftStatus};

#[derive(Deserialize)]
pub(crate) struct ReadLogPayload {
    start: usize,
    lines: usize,
    name: String
}

pub(crate) async fn api_read_log(cookies: CookieJar, Json(payload): Json<ReadLogPayload>) -> Response<Body> {
    log_out(cookies, payload, false).await
}

pub(crate) async fn api_all_logs(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    match fs::read_dir("./logs/") {
        Ok(v) => {
            let mut names = vec![];
            for x in v {
                if x.is_ok() {
                    let unwrapped = x.unwrap();
                    if unwrapped.file_type().is_ok() && unwrapped.file_type().unwrap().is_file() {
                        names.push(unwrapped.file_name().to_str().unwrap().to_string());
                    }
                }
            }
            (StatusCode::OK, names.join(", ")).into_response()
        }
        Err(_) => {
            (StatusCode::BAD_REQUEST, "Failed to get logs").into_response()
        }
    }
}

pub(crate) async fn api_log_line_count(cookies: CookieJar, payload: String) -> Response<Body> {
    log_out(cookies, ReadLogPayload { start: 0, lines: 0, name: payload }, true).await
}

async fn log_out(cookies: CookieJar, payload: ReadLogPayload, line_count: bool) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    match safe_file("./logs/", payload.name.as_str()) {
        Ok(v) => {
            let out;
            if payload.name.ends_with(".gz") {
                let mut gz = GzDecoder::new(&v[..]);
                let mut s = String::new();
                match gz.read_to_string(&mut s) {
                    Ok(_) => {
                        out = s.into_bytes();
                    }
                    Err(v) => {
                        return (StatusCode::OK, format!("Error: {}", v)).into_response();
                    }
                };
            } else {
                out = v;
            }
            (StatusCode::OK, match String::from_utf8(out) {
                    Ok(o) => {
                        let split = o.split("\n").collect::<Vec<&str>>();
                        if line_count {
                            format!("{}", split.len())
                        } else {
                            let end = min(payload.start + payload.lines, split.len());
                            split[payload.start..end].join("\n").to_string()
                        }
                    }
                    Err(e) => {
                        return (StatusCode::BAD_REQUEST, format!("Error: {}", e)).into_response();
                    }
                }).into_response()
        }
        Err(v) => {
            (StatusCode::BAD_REQUEST, format!("Error: {}", v)).into_response()
        }
    }
}

pub(crate) async fn api_live_updates(
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

pub(crate) async fn api_live_logs(
    State(transmitter): State<(Sender<String>, Sender<String>)>,
    cookies: CookieJar,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    if let Some(_) = check_auth(cookies, None) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let current_line;
    {
        let mut state = STATE.lock().unwrap();
        match &mut state.status {
            MinecraftStatus::Starting { log_line, .. } => {
                current_line = log_line.clone();
            },
            MinecraftStatus::Online { log_line, .. } => {
                current_line = log_line.clone();
            },
            _ => {
                current_line = 0;
            },
        }
    }

    let mut rx = transmitter.1.subscribe();

    let stream = async_stream::stream! {
        yield Ok(Event::default().data("#### CLEAR MANAGER LOGS ####"));
        yield Ok(Event::default().data(format!("#### MANAGER LOG LINE: {} ####", current_line)));

        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}