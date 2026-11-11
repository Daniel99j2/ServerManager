use std::fs;
use std::fs::File;
use std::io::Read;
use anvil_nbt::nbt::NbtTag;
use axum::body::{Body, Bytes};
use axum::http::{header, Response};
use axum::Json;
use axum::response::IntoResponse;
use axum_extra::extract::CookieJar;
use content_inspector::{inspect, ContentType};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use crate::{check_auth, get_info, is_safe_file};
use crate::get_info::{compress_bytes, hash_file, read_nbt};
use anvil_nbt::nbt::serde_impl::{to_nbt, from_nbt};
use base64::{engine::general_purpose::STANDARD, Engine};

#[derive(Deserialize)]
pub(crate) struct SaveFilePayload {
    old_hash: String,
    bypass_hash: bool,
    content: String,
    path: String
}

pub(crate) async fn api_save_file(cookies: CookieJar, Json(payload): Json<SaveFilePayload>) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    let path = payload.path.clone();
    if !is_safe_file("./", &*path) {
        return (StatusCode::BAD_REQUEST, "Invalid file path").into_response();
    }
    if !payload.bypass_hash {
        if get_info::hash_file(File::open(&path).unwrap()).unwrap_or("FAILED TO HASH".to_string()) != payload.old_hash {
            return (StatusCode::UNPROCESSABLE_ENTITY, "Wrong hash").into_response();
        }
    }
    match fs::write(path, payload.content) {
        Ok(_) => {
            (StatusCode::OK, "Saved file").into_response()
        }
        Err(_) => {
            (StatusCode::BAD_REQUEST, "Failed to write file").into_response()
        }
    }
}

#[derive(Serialize)]
pub(crate) struct GetFilePayload {
    hash: String,
    content: String,
    editor: String
}

pub(crate) async fn api_get_file(cookies: CookieJar, payload: String) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    if !is_safe_file("./", &*payload) {
        return (StatusCode::BAD_REQUEST, "Invalid file path").into_response();
    }
    let mut buffer = vec![0; 1024];
    let bytes_read = match File::open(&payload) {
        Ok(mut v) => {
            match v.read(&mut buffer) {
                Ok(v) => {
                    v
                }
                Err(_) => {
                    return (StatusCode::BAD_REQUEST, "Failed to read file").into_response();
                }
            }
        }
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Failed to read file").into_response();
        }
    };

    let binary = inspect(&buffer[..bytes_read]).is_binary();
    let mut out = GetFilePayload{
        hash: "not sent!".to_string(),
        editor: "text".to_string(),
        content: "".to_string()
    };
    if binary {
        out.editor = "binary".to_string();
        if payload.contains("data/maps/") && payload.ends_with(".dat") {
            out.editor = "mc_map".to_string();
        }
    }
    if !binary {
        out.hash = match hash_file(match File::open(&payload) {
            Ok(v) => {
                v
            }
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Failed to read file").into_response();
            }
        }) {
            Ok(v) => {
                v
            }
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Failed to read file").into_response();
            }
        };
        match fs::read_to_string(payload) {
            Ok(v) => {
                out.content = v;
            }
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Failed to read file").into_response();
            }
        }
    }
    (StatusCode::OK, serde_json::to_string(&out).unwrap()).into_response()
}

#[derive(Serialize, Deserialize)]
struct MapDataData {
    #[serde(with = "serde_bytes")]
    colors: Vec<u8>
}

#[derive(Serialize, Deserialize)]
struct MapData {
    data: MapDataData,
}

pub(crate) async fn api_map_data(cookies: CookieJar, payload: String) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    if !is_safe_file("./", &*payload) {
        return (StatusCode::BAD_REQUEST, "Invalid file path").into_response();
    }
    let nbt = match read_nbt(&payload) {
        None => {
            return (StatusCode::BAD_REQUEST, "Failed to read NBT").into_response();
        }
        Some(v) => {
            v
        }
    };
    let decoded: MapData = match from_nbt(nbt) {
        Err(v) => {
            return (StatusCode::BAD_REQUEST, format!("Failed to deserialize to data {}", v)).into_response();
        }
        Ok(v) => {
            v
        }
    };
    match compress_bytes(&*decoded.data.colors) {
        Err(v) => {
            (StatusCode::BAD_REQUEST, format!("Failed to compress data {}", v)).into_response()
        }
        Ok(v) => {
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/octet-stream")], Bytes::from(v)).into_response()
        }
    }
}

pub(crate) async fn api_list_dir(cookies: CookieJar, payload: String) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    if !is_safe_file("./", &*payload) {
        return (StatusCode::BAD_REQUEST, "Invalid dir path").into_response();
    }
    match fs::read_dir(payload.clone()) {
        Ok(v) => {
            let mut out = vec![];
            for x in v {
                if x.is_ok() {
                    let path = x.unwrap().path();
                    let dir = path.is_dir();
                    let mut name = path.into_string().unwrap();
                    if dir {
                        name = format!("{}/", name);
                    }
                    if name.starts_with("/") {
                        name = name.split_at(1).1.to_string();
                    }
                    out.push(name.replace(&payload.clone(), ""));
                }
            }
            (StatusCode::OK, out.join("\n")).into_response()
        }
        Err(_) => {
            (StatusCode::BAD_REQUEST, "Failed to find dir").into_response()
        }
    }
}
