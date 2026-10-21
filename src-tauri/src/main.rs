#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::{stream, StreamExt};
use serde_json::Value;
use std::{path::{Path, PathBuf}, process::Stdio, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

mod config;

fn e<E: ToString>(x: E) -> String { x.to_string() }

fn root(app: &AppHandle) -> PathBuf { app.path().app_data_dir().expect("dossier de données") }

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().user_agent("Plume/0.1").connect_timeout(Duration::from_secs(10)).build().map_err(e)
}

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

fn sub(s: &str, vars: &[(&str, String)]) -> String {
    let mut s = s.to_string();
    for (k, v) in vars { s = s.replace(k, v) }
    s
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
        for v in vals { if let Some(s) = v.as_str() { out.push(sub(s, vars)) } }
    }
    out
}

// Extrait les bibliothèques natives (anciennes versions) à plat dans un dossier.
fn extract(jar: &Path, dir: &Path) -> Result<(), String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(jar).map_err(e)?).map_err(e)?;
    for i in 0..z.len() {
        let mut x = z.by_index(i).map_err(e)?;
        let n = x.name().to_string();
        if x.is_dir() || n.starts_with("META-INF/") || n.contains("..") { continue }
        let Some(name) = Path::new(&n).file_name() else { continue };
        let mut o = std::fs::File::create(dir.join(name)).map_err(e)?;
        std::io::copy(&mut x, &mut o).map_err(e)?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_exec(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).map_err(e)
}
#[cfg(not(unix))]
fn make_exec(_: &Path) -> Result<(), String> { Ok(()) }

// Télécharge le Java officiel que Mojang associe à cette version (comme le launcher officiel).
async fn ensure_java(c: &reqwest::Client, shared: &Path, v: &Value) -> Result<String, String> {
    let comp = v["javaVersion"]["component"].as_str().unwrap_or("java-runtime-delta");
    let plat = if cfg!(all(target_os = "linux", target_arch = "x86_64")) { "linux" }
        else if cfg!(all(target_os = "windows", target_arch = "x86_64")) { "windows-x64" }
        else if cfg!(all(target_os = "windows", target_arch = "aarch64")) { "windows-arm64" }
        else if cfg!(all(target_os = "macos", target_arch = "aarch64")) { "mac-os-arm64" }
        else if cfg!(target_os = "macos") { "mac-os" }
        else { return Err("plateforme sans Java Mojang".into()) };
    let dir = shared.join("runtime").join(comp);
    let rel = if cfg!(windows) { "bin/java.exe" } else if cfg!(target_os = "macos") { "jre.bundle/Contents/Home/bin/java" } else { "bin/java" };
    let bin = dir.join(rel).to_string_lossy().to_string();
    if dir.join(".ok").exists() { return Ok(bin) }
    let all = get_json(c, config::JAVA_RUNTIMES).await?;
    let murl = all[plat][comp][0]["manifest"]["url"].as_str().ok_or("runtime introuvable")?;
    let m = get_json(c, murl).await?;
    let mut jobs = vec![];
    for (p, f) in m["files"].as_object().ok_or("manifest Java invalide")? {
        let dest = dir.join(p);
        match f["type"].as_str() {
            Some("directory") => tokio::fs::create_dir_all(&dest).await.map_err(e)?,
            Some("file") => if let Some(u) = f["downloads"]["raw"]["url"].as_str() {
                jobs.push((u.to_string(), dest, f["executable"] == true));
            },
            _ => {}
        }
    }
    let mut st = stream::iter(jobs.into_iter().map(|(u, d, x)| {
        let c = c.clone();
        async move { fetch(&c, &u, &d).await?; if x { make_exec(&d)?; } Ok::<(), String>(()) }
    })).buffer_unordered(16);
    while let Some(r) = st.next().await { r?; }
    tokio::fs::write(dir.join(".ok"), "").await.map_err(e)?;
    Ok(bin)
}

#[tauri::command]
async fn list_versions(app: AppHandle, snapshots: bool) -> Result<Vec<String>, String> {
    let shared = root(&app).join("shared");
    let cache = shared.join("version_manifest.json");
    let m = match get_json(&client()?, config::MOJANG_MANIFEST).await {
        Ok(m) => {
            let _ = tokio::fs::create_dir_all(&shared).await;
            let _ = tokio::fs::write(&cache, m.to_string()).await;
            m
        }
        Err(_) => {
            let t = tokio::fs::read_to_string(&cache).await.map_err(|_| "Hors ligne : aucune liste en cache".to_string())?;
            serde_json::from_str(&t).map_err(e)?
        }
    };
    Ok(m["versions"].as_array().into_iter().flatten()
        .filter(|x| snapshots || x["type"] == "release")
        .filter_map(|x| x["id"].as_str().map(String::from)).collect())
}

#[tauri::command]
async fn launch(app: AppHandle, version: String, username: String, ram_gb: u32, nice: i32) -> Result<i32, String> {
    let say = |m: &str| { let _ = app.emit("progress", m.to_string()); };
    let shared = root(&app).join("shared");
    let game_dir = root(&app).join("instances").join("default");
    tokio::fs::create_dir_all(&game_dir).await.map_err(e)?;
    let c = client()?;

    // Version déjà téléchargée : aucune connexion nécessaire.
    let vjson = shared.join("versions").join(&version).join(format!("{version}.json"));
    let v: Value = if vjson.exists() {
        serde_json::from_str(&tokio::fs::read_to_string(&vjson).await.map_err(e)?).map_err(e)?
    } else {
        say("Récupération de la version…");
        let manifest = get_json(&c, config::MOJANG_MANIFEST).await.map_err(|_| "Connexion requise pour télécharger cette version".to_string())?;
        let url = manifest["versions"].as_array().and_then(|v| v.iter().find(|x| x["id"] == version.as_str()))
            .and_then(|x| x["url"].as_str()).ok_or(format!("Version {version} introuvable"))?.to_string();
        let t = c.get(&url).send().await.map_err(e)?.error_for_status().map_err(e)?.text().await.map_err(e)?;
        tokio::fs::create_dir_all(vjson.parent().unwrap()).await.map_err(e)?;
        tokio::fs::write(&vjson, &t).await.map_err(e)?;
        serde_json::from_str(&t).map_err(e)?
    };

    say("Préparation de Java…");
    let java = ensure_java(&c, &shared, &v).await.unwrap_or_else(|_| "java".to_string());

    say("Téléchargement du jeu…");
    let jar = shared.join("versions").join(&version).join(format!("{version}.jar"));
    fetch(&c, v["downloads"]["client"]["url"].as_str().ok_or("client.jar manquant")?, &jar).await?;

    let os = os_name();
    let mut classpath: Vec<String> = vec![];
    let mut libs: Vec<(String, PathBuf)> = vec![];
    let mut natives_jars: Vec<PathBuf> = vec![];
    for l in v["libraries"].as_array().into_iter().flatten() {
        if !allowed(l.get("rules").unwrap_or(&Value::Null)) { continue }
        let a = &l["downloads"]["artifact"];
        if let (Some(p), Some(u)) = (a["path"].as_str(), a["url"].as_str()) {
            let dest = shared.join("libraries").join(p);
            classpath.push(dest.to_string_lossy().to_string());
            libs.push((u.to_string(), dest));
        }
        if let Some(k) = l["natives"][os].as_str() {
            let k = k.replace("${arch}", if cfg!(target_pointer_width = "64") { "64" } else { "32" });
            let n = &l["downloads"]["classifiers"][k.as_str()];
            if let (Some(p), Some(u)) = (n["path"].as_str(), n["url"].as_str()) {
                let dest = shared.join("libraries").join(p);
                natives_jars.push(dest.clone());
                libs.push((u.to_string(), dest));
            }
        }
    }
    classpath.push(jar.to_string_lossy().to_string());
    say(&format!("Téléchargement de {} bibliothèques…", libs.len()));
    for (u, d) in &libs { fetch(&c, u, d).await?; }

    let idx_id = v["assetIndex"]["id"].as_str().ok_or("assetIndex manquant")?.to_string();
    let idx_path = shared.join("assets").join("indexes").join(format!("{idx_id}.json"));
    fetch(&c, v["assetIndex"]["url"].as_str().ok_or("assetIndex url")?, &idx_path).await?;
    let idx: Value = serde_json::from_str(&tokio::fs::read_to_string(&idx_path).await.map_err(e)?).map_err(e)?;
    let objs: Vec<(String, String)> = idx["objects"].as_object()
        .map(|o| o.iter().filter_map(|(k, x)| x["hash"].as_str().map(|h| (k.clone(), h.to_string()))).collect()).unwrap_or_default();
    let total = objs.len();
    let mut done = 0;
    {
        let mut st = stream::iter(objs.iter().map(|(_, h)| {
            let c = c.clone();
            let dest = shared.join("assets").join("objects").join(&h[..2]).join(h);
            let url = format!("https://resources.download.minecraft.net/{}/{}", &h[..2], h);
            async move { fetch(&c, &url, &dest).await }
        })).buffer_unordered(32);
        while let Some(r) = st.next().await {
            r?; done += 1;
            if done % 50 == 0 || done == total { say(&format!("Ressources du jeu : {done}/{total}")); }
        }
    }
    // Très anciennes versions : les ressources doivent être copiées à leur nom d'origine.
    let virt = idx["virtual"] == true;
    let to_res = idx["map_to_resources"] == true;
    let game_assets = if virt { shared.join("assets/virtual/legacy") } else if to_res { game_dir.join("resources") } else { shared.join("assets") };
    if virt || to_res {
        for (name, h) in &objs {
            let src = shared.join("assets").join("objects").join(&h[..2]).join(h);
            let dst = game_assets.join(name);
            if !dst.exists() {
                if let Some(p) = dst.parent() { tokio::fs::create_dir_all(p).await.map_err(e)?; }
                tokio::fs::copy(&src, &dst).await.map_err(e)?;
            }
        }
    }

    let natives = shared.join("natives").join(&version);
    tokio::fs::create_dir_all(&natives).await.map_err(e)?;
    for j in natives_jars {
        let n = natives.clone();
        tokio::task::spawn_blocking(move || extract(&j, &n)).await.map_err(e)??;
    }

    let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let vars: Vec<(&str, String)> = vec![
        ("${auth_player_name}", username.clone()),
        ("${version_name}", version.clone()),
        ("${game_directory}", game_dir.to_string_lossy().to_string()),
        ("${assets_root}", shared.join("assets").to_string_lossy().to_string()),
        ("${game_assets}", game_assets.to_string_lossy().to_string()),
        ("${assets_index_name}", idx_id),
        ("${auth_uuid}", "00000000-0000-0000-0000-000000000000".into()),
        ("${auth_access_token}", "0".into()),
        ("${auth_session}", "token:0".into()),
        ("${user_properties}", "{}".into()),
        ("${clientid}", "0".into()),
        ("${auth_xuid}", "0".into()),
        ("${user_type}", "legacy".into()),
        ("${version_type}", "Plume".into()),
        ("${profile_name}", "Plume".into()),
        ("${natives_directory}", natives.to_string_lossy().to_string()),
        ("${launcher_name}", "Plume".into()),
        ("${launcher_version}", "0.1.0".into()),
        ("${classpath}", classpath.join(sep)),
        ("${library_directory}", shared.join("libraries").to_string_lossy().to_string()),
        ("${classpath_separator}", sep.into()),
    ];
    let main_class = v["mainClass"].as_str().ok_or("mainClass manquant")?.to_string();
    let mut args: Vec<String> = vec![format!("-Xmx{}G", ram_gb.clamp(1, 32)), "-Xms512M".into()];
    if v["arguments"].is_object() {
        args.extend(collect(&v["arguments"]["jvm"], &vars));
        args.push(main_class);
        args.extend(collect(&v["arguments"]["game"], &vars));
    } else {
        // Format ancien (1.12 et avant)
        for s in ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"] { args.push(sub(s, &vars)); }
        args.push(main_class);
        args.extend(v["minecraftArguments"].as_str().unwrap_or("").split_whitespace().map(|s| sub(s, &vars)));
    }

    let log = std::fs::File::create(game_dir.join("launcher.log")).map_err(e)?;
    let log2 = log.try_clone().map_err(e)?;
    let mut cmd = if cfg!(unix) && nice != 0 {
        let mut c = tokio::process::Command::new("nice");
        c.arg("-n").arg(nice.to_string()).arg(&java);
        c
    } else { tokio::process::Command::new(&java) };
    let mut child = cmd.args(&args).current_dir(&game_dir)
        .stdout(Stdio::from(log)).stderr(Stdio::from(log2))
        .spawn().map_err(|_| format!("Impossible de lancer Java ({java})."))?;

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

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    let u = app.updater().map_err(e)?.check().await.map_err(e)?;
    Ok(u.map(|u| u.version))
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let u = app.updater().map_err(e)?.check().await.map_err(e)?.ok_or("Aucune mise à jour disponible")?;
    let mut got = 0u64;
    u.download_and_install(
        |chunk, total| {
            got += chunk as u64;
            let _ = app.emit("progress", format!("Mise à jour : {} / {} Mo", got / 1_000_000, total.unwrap_or(0) / 1_000_000));
        },
        || {},
    ).await.map_err(e)?;
    app.restart()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![launch, read_log, list_versions, check_update, install_update])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Plume");
}
