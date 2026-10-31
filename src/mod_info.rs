use crate::check_auth;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum_extra::extract::CookieJar;
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::env::temp_dir;
use std::fs::{DirEntry, File};
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::{fs, io};
use zip::ZipArchive;

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct ModInfo {
    icon: String,
    hash: String,
    modrinth_info: Option<ModrinthProjectInfo>,
    name: String,
    file_name: String,
    authors: Vec<String>,
    version: String,
    children: Vec<String>,
    resolved_children: Option<Vec<String>>
}

static INSTALLED_MODS: Mutex<Vec<String>> = Mutex::new(vec![]);
static MOD_CACHE: Mutex<LazyLock<HashMap<String, ModInfo>>> = Mutex::new(LazyLock::new(HashMap::new));

pub(crate) async fn api_installed_mods(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    load_mods().await;

    let mutex = INSTALLED_MODS.lock().unwrap();
    let mutex2 = MOD_CACHE.lock().unwrap();

    let mut mods = vec![];

    for x in mutex.iter() {
        mods.push(mutex2.get(x).unwrap());
    }

    (StatusCode::OK, serde_json::to_string_pretty(&mods).unwrap()).into_response()
}

pub(crate) async fn api_child_mod(cookies: CookieJar, payload: String) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }

    let mut children = vec![];
    let name;

    {
        let cache = MOD_CACHE.lock().unwrap();
        match cache.get(&payload.clone()) {
            None => {
                return (StatusCode::BAD_REQUEST, "Unknown mod").into_response();
            }
            Some(v) => {
                if v.resolved_children.is_some() {
                    let mut resolved = vec![];
                    let mut worked = true;
                    {
                        for hash in v.resolved_children.clone().unwrap() {
                            if cache.contains_key(&hash.clone()) {
                                resolved.push(cache.get(&hash).unwrap().clone());
                            } else {
                                worked = false;
                                break;
                            }
                        }
                    }
                    if worked {
                        return (StatusCode::OK, serde_json::to_string(&resolved).unwrap()).into_response();
                    }
                }
                children = v.children.clone();
                name = v.file_name.clone();
            }
        }
    }

    match load_children(name, children).await {
        None => {
            (StatusCode::BAD_REQUEST, "").into_response()
        }
        Some(v) => {
            let copy = v.clone();
            {
                let mut hashes = vec![];
                for x in copy {
                    hashes.push(x.hash.clone());
                }
                let mut mutex2 = MOD_CACHE.lock().unwrap();
                let mut current = mutex2.get(payload.as_str()).unwrap().clone();
                current.resolved_children = Some(hashes);
                mutex2.insert(payload.clone(), current);
            }
            save_cached_mods();
            (StatusCode::OK, serde_json::to_string(&v).unwrap()).into_response()
        }
    }
}

async fn load_children(name: String, children: Vec<String>) -> Option<Vec<ModInfo>> {
    let file;
    match File::open(Path::new(format!("mods/{}", &name).as_str())) {
        Ok(v) => {
            file = v;
        }
        Err(_) => {
            println!("Unknown mod {}", name);
            return None;
        }
    };
    let reader = BufReader::new(file.try_clone().unwrap());
    let mut out = vec![];

    match ZipArchive::new(reader) {
        Ok(mut v) => {
            for x in children {
                match v.by_name(x.clone().as_str()) {
                    Ok(mut f) => {
                        let path = temp_dir().join(x.clone().replace("\\", "/").replace("/", "_"));
                        println!("{:?}", path.clone());
                        {
                            let mut output_file = File::create(path.clone()).expect("Failed to create temp file");
                            io::copy(&mut f, &mut output_file).expect("Failed to copy to temp file");
                            output_file.sync_all().expect("Failed to sync temp file");
                        }
                        let output_file = File::open(path.clone()).expect("Failed to create temp file");
                        let hash = hash_file(output_file);
                        match hash {
                            Ok(v) => {
                                let contains;
                                {
                                    let mutex2 = MOD_CACHE.lock().unwrap();
                                    contains = mutex2.contains_key(&v);
                                }
                                if !contains {
                                    let val = read(path.into_string().unwrap(), Some(x.rsplit("/").next().unwrap().to_string())).await;
                                    {
                                        let mut mutex2 = MOD_CACHE.lock().unwrap();
                                        mutex2.insert(v.clone(), val);
                                    }
                                }
                                {
                                    let mutex2 = MOD_CACHE.lock().unwrap();
                                    out.push(mutex2.get(&v).unwrap().clone());
                                }
                            }
                            Err(v) => {
                                println!("Could not hash mod {} {}", x, v);
                            }
                        }
                    }
                    Err(_) => {
                        println!("Could not fi  nd mod {}", x);
                    }
                }
            }
        }
        Err(_) => {
            return None;
        }
    };
    save_cached_mods();
    Some(out)
}

pub(crate) async fn api_get_updates(cookies: CookieJar) -> Response<Body> {
    if let Some(out) = check_auth(cookies, None) {
        return out;
    }
    let mut mods = vec![];

    {
        let mutex = INSTALLED_MODS.lock().unwrap();
        let mutex2 = MOD_CACHE.lock().unwrap();
        for x in mutex.iter() {
            if mutex2.get(x).unwrap().modrinth_info.is_some() {
                println!("Modrinth info found {}", x.clone());
                mods.push(x.clone());
            }
        }
    }

    let mut out = vec![];

    let all1 = get_updates_from_hashes(mods).await;
    let all;
    if all1.is_some() {
        all = all1.unwrap();
    } else {
        return (StatusCode::GATEWAY_TIMEOUT, "Failed to get from modrinth API").into_response();
    }

    for x in all.iter() {
        out.push(x.1);
    }

    (StatusCode::OK, serde_json::to_string_pretty(&out).unwrap()).into_response()
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SavedModCache {
    cache: Vec<ModInfo>
}

pub(crate) async fn first_load() {
    let mut mutex2 = MOD_CACHE.lock().unwrap();
    mutex2.clear();
        match fs::read_to_string("manager/mod_cache.json") {
        Ok(content) => match serde_json::from_str::<SavedModCache>(&content) {
            Ok(c) => {
                for x in c.cache {
                    mutex2.insert(x.hash.clone(), x);
                }
            }
            Err(_) => {
            }
        },
        Err(_) => {
        }
    }
}

async fn load_mods() {
    {
        let mut mutex = INSTALLED_MODS.lock().unwrap();
        mutex.clear();
    }
    match fs::read_dir("./mods/") {
        Ok(v) => {
            let mut handles = vec![];

            for x in v {
                if x.is_ok() {
                    handles.push(tokio::spawn(load_mod(x.unwrap())));
                }
            }

            for handle in handles {
                match handle.await {
                    Ok(_) => {},
                    Err(e) => eprintln!("Task panicked: {:?}", e),
                }
            }
        }
        Err(_) => {
        }
    }
    save_cached_mods();
}

fn save_cached_mods() {
    {
        let mutex2 = MOD_CACHE.lock().unwrap();
        let mut mods = SavedModCache{cache: vec![]};

        for x in mutex2.iter() {
            mods.cache.push(x.1.clone());
        }

        fs::write(
            "manager/mod_cache.json",
            serde_json::to_string(&mods).expect("Mod cache could not be serialized"),
        )
            .expect("Mod cache could not be saved");
    }
}

async fn load_mod(x: DirEntry) {
    let path = x.path();
    let path_string = path.clone().into_string().unwrap();
    if !path_string.ends_with(".jar") {
        return;
    }
    println!("Loading mod {}", &path_string);
    let file  = File::open(Path::new(path_string.to_string().as_str()));
    let hash = hash_file(file.unwrap());
    match hash {
        Ok(v) => {
            let contains;
            {
                let mut mutex = INSTALLED_MODS.lock().unwrap();
                let mutex2 = MOD_CACHE.lock().unwrap();
                mutex.push(v.clone());
                contains = mutex2.contains_key(&v);
            }
            if !contains {
                let val = read(path_string, None).await;
                {
                    let mut mutex2 = MOD_CACHE.lock().unwrap();
                    mutex2.insert(v, val);
                }
            }
        }
        Err(_) => {}
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FabricAuthor {
    name: String
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum AuthorOrString {
    Text(String),
    Object(FabricAuthor),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FabricModJson {
    name: Option<String>,
    authors: Option<Vec<AuthorOrString>>,
    version: String,
    icon: Option<String>,
    jars: Option<Vec<HashMap<String, String>>>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeoforgeMainToml {
    mods: Vec<NeoforgeModToml>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeoforgeModToml {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    version: String,
    authors: Option<String>,
}

async fn read(name: String, real_name: Option<String>) -> ModInfo {
    let mut mod_name = "Unknown".to_string();
    let mut mod_authors = vec![];
    let mut mod_version = "???".to_string();
    let mut icon_file = "".to_string();
    let mut children = vec![];

    let file  = File::open(Path::new(&name.clone())).unwrap();
    let reader = BufReader::new(file.try_clone().unwrap());
    let mut archive = ZipArchive::new(reader).unwrap();
    let mut found = false;
    match archive.by_name("fabric.mod.json") {
        Ok(mut v) => {
            let mut contents = String::new();
            v.read_to_string(&mut contents).unwrap();
            match serde_json::from_str::<FabricModJson>(&contents) {
                Ok(mod_json) => {
                    mod_name = mod_json.name.unwrap_or("Unspecified".to_string());
                    println!("A {:?}", mod_json.authors);
                    if mod_json.authors.is_some() {
                        for x in mod_json.authors.unwrap() {
                            match x {
                                AuthorOrString::Text(v) => {
                                    mod_authors.push(v.clone());
                                }
                                AuthorOrString::Object(v) => {
                                    mod_authors.push(v.name.clone());
                                }
                            }
                        }
                    }
                    mod_version = mod_json.version.clone();
                    if mod_json.icon.is_some() {
                        icon_file = mod_json.icon.unwrap();
                    }
                    if mod_json.jars.is_some() {
                        for x in mod_json.jars.unwrap() {
                            children.push(x.get("file").expect("Invalid format").to_string());
                        }
                    }
                    found = true;
                }
                Err(v) => {
                    println!("Failed to load mod json {:?}", v);
                }
            };

        }
        //not a fabric mod
        Err(_) => {
            println!("Not fabric mod")
        }
    }

    if !found {
        match archive.by_name("META-INF/neoforge.mods.toml") {
            Ok(mut v) => {
                let mut contents = String::new();
                v.read_to_string(&mut contents).unwrap();
                let main_toml: NeoforgeMainToml = toml::from_str(&contents).unwrap();
                let mod_toml = main_toml.mods.get(0).unwrap().clone();
                mod_name = mod_toml.display_name.unwrap_or("Unspecified".to_string());
                if mod_toml.authors.is_some() {
                    let authors = mod_toml.authors.unwrap();
                    if authors.contains(", ") {
                        for x in authors.split(", ") {
                            mod_authors.push(x.to_string());
                        }
                    } else {
                        mod_authors.push(authors);
                    }
                }
                mod_version = mod_toml.version.clone();
            }
            //unknown mod type
            Err(_) => {}
        }
    }

    let mut modrinth_info: Option<ModrinthProjectInfo> = None;
    let mut info = ModInfo {
        hash: match hash_file(File::open(&name).unwrap()) {
            Ok(b) => {
                modrinth_info = hash_info(b.clone()).await;
                b
            }
            Err(_) => {
                "Failed to hash".to_string()
            }
        },
        modrinth_info: modrinth_info,
        icon: "".to_string(),
        authors: mod_authors,
        name: mod_name,
        version: mod_version,
        file_name: name.split('/').last().unwrap().to_string(),
        children,
        resolved_children: None
    };
    if real_name.is_some() {
        info.file_name = real_name.unwrap();
    }
    if info.modrinth_info.is_some() {
        let copied = info.modrinth_info.clone().unwrap();
        info.icon = copied.icon_url.unwrap_or("".to_string());
        info.name = copied.title;
    } else {
        if !icon_file.is_empty() {
            match archive.by_name(&*icon_file) {
                Ok(mut v) => {
                    let mut buffer = Vec::new();
                    v.read_to_end(&mut buffer).unwrap();
                    let base64_encoded = BASE64_STANDARD.encode(&buffer);
                    info.icon = format!("data:image/png;base64,{}", base64_encoded);
                }
                Err(_) => {}
            }
        }
    }
    info
}

fn hash_file(mut file: File) -> io::Result<String> {
    println!("Start");
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
    println!("Finish");
    Ok(hex::encode(hash_result))
}

#[derive(Serialize, Deserialize, Debug)]
struct ModrinthVersionInfo {
    name: String,
    version_number: String,
    id: String,
    project_id: String,
    downloads: usize
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ModrinthProjectInfo {
    id: String,
    team: String,
    title: String,
    description: String,
    body: String,
    project_type: String,
    categories: Vec<String>,
    additional_categories: Vec<String>,
    environment: Vec<String>,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    versions: Vec<String>,
    downloads: usize,
    icon_url: Option<String>,
}

async fn hash_info(hash: String) -> Option<ModrinthProjectInfo> {
    println!("Getting {}", hash);
    let client = reqwest::Client::new();

    let out = client.get(format!("https://api.modrinth.com/v2/version_file/{}", hash)).send().await;
    match out {
        Ok(info) => {
            if info.status() == StatusCode::OK {
                let body = info.text().await.unwrap();
                let temp_info = serde_json::from_str::<ModrinthVersionInfo>(&body).unwrap();
                return project_from_id(temp_info.project_id).await;
            } else if info.status() == StatusCode::NOT_FOUND {
                println!("Not found {}", hash);
            } else {
                println!("Failed to get project version from hash, {}, {}", info.status(), format!("https://api.modrinth.com/v2/version_file/{}", hash));
            }
        }
        Err(_) => {
            println!("Failed to get project version from hash");
        }
    }
    None
}

#[derive(Serialize, Deserialize, Debug)]
struct FileHashes {
    sha512: String
}

#[derive(Serialize, Deserialize, Debug)]
struct UpdateFile {
    hashes: FileHashes,
    url: String,
    filename: String,
    primary: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ModToUpdate {
    name: String,
    version_number: String,
    changelog: String,
    version_type: String,
    files: Vec<UpdateFile>
}

#[derive(Serialize, Deserialize, Debug)]
struct GetUpdatesBody {
    hashes: Vec<String>,
    algorithm: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
}


async fn get_updates_from_hashes(hashes: Vec<String>) -> Option<HashMap<String, ModToUpdate>> {
    let client = reqwest::Client::new();

    let body = GetUpdatesBody{
        hashes,
        algorithm: "sha512".to_string(),
        loaders: vec![
            "fabric".to_string(),
        ],
        game_versions: vec![
            "26.3".to_string(),
        ],
    };

    let out = client.post("https://api.modrinth.com/v2/version_files/update/".to_string()).body(serde_json::to_string(&body).unwrap()).send().await;
    match out {
        Ok(info) => {
            if info.status() == StatusCode::OK {
                let body = info.text().await.unwrap();
                return Some(serde_json::from_str::<HashMap<String, ModToUpdate>>(&body).unwrap());
            } else {
                println!("Failed to get updates, {}", info.status());
            }
        }
        Err(_) => {
            println!("Failed to get updates");
        }
    }
    None
}

async fn project_from_id(project_id: String) -> Option<ModrinthProjectInfo> {
    let client = reqwest::Client::new();

    let out = client.get(format!("https://api.modrinth.com/v2/project/{}", project_id)).send().await;
    match out {
        Ok(info) => {
            if info.status() == StatusCode::OK {
                let body = info.text().await.unwrap();
                return Some(serde_json::from_str::<ModrinthProjectInfo>(&body).unwrap());
            } else {
                println!("Failed to get project info from project id, {}", info.status());
            }
        }
        Err(_) => {
            println!("Failed to get project info from project id");
        }
    }
    None
}
