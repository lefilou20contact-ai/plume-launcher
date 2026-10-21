#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::{stream, StreamExt};
use serde_json::Value;
use std::{path::{Path, PathBuf}, process::Stdio};
use tauri::{AppHandle, Emitter, Manager};

mod config;

fn e<E: ToString>(x: E) -> String { x.to_string() }

fn root(app: &AppHandle) -> PathBuf { app.path().app_data_dir().expect("dossier de données") }

fn os_name() -> &'static str {
    if cfg!(target_os = "windows") { "windows" } else if cfg!(target_os = "macos") { "osx" } else { "linux" }
}

// Évalue les "rules" du JSON Mojang (système, architecture). Les règles à "features" sont ignorées.
fn allowed(rules: &Value) -> bool {
    let Some(arr) = rules.as_array() else { return true };
    let mut ok = false;
    for r in arr {
        let mut m = true;
        if let Some(os) = r.get("os") {
            if let Some(n) = os.get("name").and_then(|v| v.as_str()) { if n != os_name() { m = false } }
            match os.get("arch").and_then(|v| v.as_str()) {
                Some("x86") if !cfg!(target_arch = "x86") => m = false,
                Some("arm64") if !cfg!(target_arch = "aarch64") => m = false,
                _ => {}
            }
        }
        if r.get("features").is_some() { m = false }
        if m { ok = r["action"] == "allow" }
    }
    ok
}

async fn get_json(c: &reqwest::Client, url: &str) -> Result<Value, String> {
    let t = c.get(url).send().await.map_err(e)?.error_for_status().map_err(e)?.text().await.map_err(e)?;
    serde_json::from_str(&t).map_err(e)
}

async fn fetch(c: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    if dest.exists() { return Ok(()) }
    if let Some(p) = dest.parent() { tokio::fs::create_dir_all(p).await.map_err(e)?; }
    let b = c.get(url).send().await.map_err(e)?.error_for_status().map_err(e)?.bytes().await.map_err(e)?;
    let tmp = dest.with_extension("part");
    tokio::fs::write(&tmp, b).await.map_err(e)?;
    tokio::fs::rename(&tmp, dest).await.map_err(e)
}

fn collect(args: &Value, vars: &[(&str, String)]) -> Vec<String> {
    let mut out = vec![];
    for a in args.as_array().into_iter().flatten() {
        let vals: Vec<&Value> = if a.is_string() {
            vec![a]
        } else {
            if !allowed(a.get("rules").unwrap_or(&Value::Null)) { continue }
            match &a["value"] { Value::Array(x) => x.iter().collect(), v => vec![v] }
        };
        for v in vals {
            if let Some(s) = v.as_str() {
                let mut s = s.to_string();
                for (k, val) in vars { s = s.replace(k, val) }
                out.push(s);
            }
        }
    }
    out
}

#[tauri::command]
async fn launch(app: AppHandle, version: String, username: String, ram_gb: u32, nice: i32) -> Result<i32, String> {
    let say = |m: &str| { let _ = app.emit("progress", m.to_string()); };
    let shared = root(&app).join("shared");
    let game_dir = root(&app).join("instances").join("default");
    tokio::fs::create_dir_all(&game_dir).await.map_err(e)?;
    let c = reqwest::Client::builder().user_agent("Plume/0.1").build().map_err(e)?;

    say("Récupération de la version…");
    let manifest = get_json(&c, config::MOJANG_MANIFEST).await?;
    let url = manifest["versions"].as_array().and_then(|v| v.iter().find(|x| x["id"] == version.as_str()))
        .and_then(|x| x["url"].as_str()).ok_or(format!("Version {version} introuvable"))?.to_string();
    let v = get_json(&c, &url).await?;

    say("Téléchargement du jeu…");
    let jar = shared.join("versions").join(&version).join(format!("{version}.jar"));
    fetch(&c, v["downloads"]["client"]["url"].as_str().ok_or("client.jar manquant")?, &jar).await?;

    let mut classpath: Vec<String> = vec![];
    let mut libs: Vec<(String, PathBuf)> = vec![];
    for l in v["libraries"].as_array().into_iter().flatten() {
        if !allowed(l.get("rules").unwrap_or(&Value::Null)) { continue }
        let a = &l["downloads"]["artifact"];
        if let (Some(p), Some(u)) = (a["path"].as_str(), a["url"].as_str()) {
            let dest = shared.join("libraries").join(p);
            classpath.push(dest.to_string_lossy().to_string());
            libs.push((u.to_string(), dest));
        }
    }
    classpath.push(jar.to_string_lossy().to_string());
    say(&format!("Téléchargement de {} bibliothèques…", libs.len()));
    for (u, d) in &libs { fetch(&c, u, d).await?; }

    let idx_id = v["assetIndex"]["id"].as_str().ok_or("assetIndex manquant")?.to_string();
    let idx_path = shared.join("assets").join("indexes").join(format!("{idx_id}.json"));
    fetch(&c, v["assetIndex"]["url"].as_str().ok_or("assetIndex url")?, &idx_path).await?;
    let idx: Value = serde_json::from_str(&tokio::fs::read_to_string(&idx_path).await.map_err(e)?).map_err(e)?;
    let objs: Vec<String> = idx["objects"].as_object().map(|o| o.values().filter_map(|x| x["hash"].as_str().map(String::from)).collect()).unwrap_or_default();
    let total = objs.len();
    let mut done = 0;
    let mut st = stream::iter(objs.into_iter().map(|h| {
        let c = c.clone();
        let dest = shared.join("assets").join("objects").join(&h[..2]).join(&h);
        async move { fetch(&c, &format!("https://resources.download.minecraft.net/{}/{}", &h[..2], h), &dest).await }
    })).buffer_unordered(32);
    while let Some(r) = st.next().await {
        r?; done += 1;
        if done % 50 == 0 || done == total { say(&format!("Ressources du jeu : {done}/{total}")); }
    }

    let natives = shared.join("natives").join(&version);
    tokio::fs::create_dir_all(&natives).await.map_err(e)?;
    let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let cp = classpath.join(sep);
    let vars: Vec<(&str, String)> = vec![
        ("${auth_player_name}", username.clone()),
        ("${version_name}", version.clone()),
        ("${game_directory}", game_dir.to_string_lossy().to_string()),
        ("${assets_root}", shared.join("assets").to_string_lossy().to_string()),
        ("${assets_index_name}", idx_id),
        ("${auth_uuid}", "00000000-0000-0000-0000-000000000000".into()),
        ("${auth_access_token}", "0".into()),
        ("${clientid}", "0".into()),
        ("${auth_xuid}", "0".into()),
        ("${user_type}", "legacy".into()),
        ("${version_type}", "Plume".into()),
        ("${natives_directory}", natives.to_string_lossy().to_string()),
        ("${launcher_name}", "Plume".into()),
        ("${launcher_version}", "0.1.0".into()),
        ("${classpath}", cp),
        ("${library_directory}", shared.join("libraries").to_string_lossy().to_string()),
        ("${classpath_separator}", sep.into()),
    ];
    let mut args: Vec<String> = vec![format!("-Xmx{}G", ram_gb.clamp(1, 32)), "-Xms512M".into()];
    args.extend(collect(&v["arguments"]["jvm"], &vars));
    args.push(v["mainClass"].as_str().ok_or("mainClass manquant")?.to_string());
    args.extend(collect(&v["arguments"]["game"], &vars));

    let log = std::fs::File::create(game_dir.join("launcher.log")).map_err(e)?;
    let log2 = log.try_clone().map_err(e)?;
    let java_major = v["javaVersion"]["majorVersion"].as_u64().unwrap_or(21);
    let mut cmd = if cfg!(unix) && nice != 0 {
        let mut c = tokio::process::Command::new("nice");
        c.arg("-n").arg(nice.to_string()).arg("java");
        c
    } else { tokio::process::Command::new("java") };
    let mut child = cmd.args(&args).current_dir(&game_dir)
        .stdout(Stdio::from(log)).stderr(Stdio::from(log2))
        .spawn().map_err(|_| format!("Java introuvable. Minecraft {version} demande Java {java_major} ou plus récent."))?;

    let win = app.get_webview_window("main");
    if let Some(w) = &win { let _ = w.hide(); }
    let status = child.wait().await.map_err(e)?;
    if let Some(w) = &win { let _ = w.show(); let _ = w.set_focus(); }
    Ok(status.code().unwrap_or(-1))
}

#[tauri::command]
fn read_log(app: AppHandle) -> String {
    let t = std::fs::read_to_string(root(&app).join("instances/default/launcher.log")).unwrap_or_default();
    let l: Vec<&str> = t.lines().collect();
    l[l.len().saturating_sub(40)..].join("\n")
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![launch, read_log])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Plume");
}
