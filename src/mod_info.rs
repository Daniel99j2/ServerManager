use crate::check_auth;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::fs::File;
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
            for x in v {
                if x.is_ok() {
                    let path = x.unwrap().path();
                    let path_string = path.clone().into_string().unwrap();
                    if !path_string.ends_with(".jar") {
                        continue;
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
                                let val = read(path_string).await;
                                {
                                    let mut mutex2 = MOD_CACHE.lock().unwrap();
                                    mutex2.insert(v, val);
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        Err(_) => {
        }
    }
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

async fn read(name: String) -> ModInfo {
    let mut mod_name = "Unknown".to_string();
    let mut mod_authors = vec![];
    let mut mod_version = "???".to_string();

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
                    found = true;
                }
                Err(_) => {

                }
            };

        }
        //not a fabric mod
        Err(_) => {

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
                    mod_authors.push(mod_toml.authors.unwrap());
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
        file_name: name.split('/').last().unwrap().to_string()
    };
    if info.modrinth_info.is_some() {
        let copied = info.modrinth_info.clone().unwrap();
        info.icon = copied.icon_url.unwrap_or("".to_string());
        info.name = copied.title;
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
