use std::cmp::{min};
use std::convert::Infallible;
use std::{fs, io};
use std::fs::File;
use std::io::{Read, Write};
use anvil_nbt::nbt::NbtTag;
use anvil_nbt::nbt::parse::{parse_named_tag, ParseError};
use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::Json;
use axum::response::{IntoResponse, Sse};
use axum::response::sse::{Event, KeepAlive};
use axum_extra::extract::CookieJar;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use tokio::sync::broadcast::Sender;
use crate::{check_auth, create_status, get_file_safe, STATE, MinecraftStatus};

#[derive(Deserialize)]
pub(crate) struct ReadLogPayload {
    start: usize,
    lines: usize,
    name: String
}

pub(crate) async fn api_read_log(cookies: CookieJar, Json(payload): Json<ReadLogPayload>) -> Response<Body> {
    log_out(cookies, payload, false).await
}

pub(crate) async fn api_search_info(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    let out;
    {
        let state = STATE.lock().unwrap();
        out = format!("{},{}", state.game_version.clone().unwrap_or("Unknown".parse().unwrap()), state.loader_type.clone().unwrap_or("Unknown".parse().unwrap()))
    }
    (StatusCode::OK, out).into_response()
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
    match get_file_safe("./logs/", payload.name.as_str()) {
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

pub(crate) fn hash_file(mut file: File) -> io::Result<String> {
    let mut hasher = Sha512::new();
    let mut buffer: [u8; 8192] = [0; 8192];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let hash_result = hasher.finalize();
    Ok(hex::encode(hash_result))
}

pub(crate) fn read_nbt(name: &String) -> Option<NbtTag> {
    let file = match File::open(name) {
        Ok(v) => {
            v
        }
        Err(_) => {
            return None;
        }
    };
    let mut decoder = GzDecoder::new(file);
    let mut data = Vec::new();
    match decoder.read_to_end(&mut data) {
        Ok(_) => {}
        Err(_) => {
            return None;
        }
    };

    let mut input = &data[..];
    match parse_named_tag(&mut input) {
        Ok((name, tag)) => {
            return Some(tag);
        }
        Err(_) => {
            return None;
        }
    }
}

pub(crate) fn compress_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}