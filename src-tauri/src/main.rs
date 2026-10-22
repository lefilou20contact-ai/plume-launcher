#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::{stream, StreamExt};
use serde_json::Value;
use std::{collections::HashSet, path::{Path, PathBuf}, process::Stdio, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

mod auth;
mod config;
mod instances;
mod modrinth;

fn prog(app: &AppHandle, t: &str, d: u64, n: u64) {
    let _ = app.emit("progress", serde_json::json!({"t": t, "d": d, "n": n}));
}

fn e<E: ToString>(x: E) -> String { x.to_string() }

fn root(app: &AppHandle) -> PathBuf { app.path().app_data_dir().expect("dossier de données") }

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().user_agent(config::USER_AGENT).connect_timeout(Duration::from_secs(10)).pool_max_idle_per_host(64).build().map_err(e)
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
async fn ensure_java(app: &AppHandle, c: &reqwest::Client, shared: &Path, v: &Value) -> Result<String, String> {
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
    let n = jobs.len() as u64;
    let mut got = 0u64;
    let mut st = stream::iter(jobs.into_iter().map(|(u, d, x)| {
        let c = c.clone();
        async move { fetch(&c, &u, &d).await?; if x { make_exec(&d)?; } Ok::<(), String>(()) }
    })).buffer_unordered(16);
    while let Some(r) = st.next().await {
        r?; got += 1;
        if got % 10 == 0 || got == n { prog(app, "Téléchargement de Java", got, n); }
    }
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

fn lib_key(name: &str) -> String { name.split(':').take(2).collect::<Vec<_>>().join(":") }

fn maven_path(name: &str) -> Option<String> {
    let p: Vec<&str> = name.split(':').collect();
    if p.len() < 3 || p.iter().any(|s| s.is_empty() || s.contains('/') || s.contains("..")) { return None }
    let cl = p.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Some(format!("{}/{}/{}/{}-{}{}.jar", p[0].replace('.', "/"), p[1], p[2], p[1], p[2], cl))
}

async fn fabric_loader_version(c: &reqwest::Client, mc: &str) -> Result<String, String> {
    let v = get_json(c, &format!("{}/versions/loader/{}", config::FABRIC_META, mc)).await
        .map_err(|_| format!("Impossible de préparer Fabric pour Minecraft {mc} (version non supportée ou hors ligne)"))?;
    let arr = v.as_array().cloned().unwrap_or_default();
    arr.iter().find(|x| x["loader"]["stable"] == true).or(arr.first())
        .and_then(|x| x["loader"]["version"].as_str()).map(String::from)
        .ok_or_else(|| format!("Fabric ne supporte pas Minecraft {mc}"))
}

async fn loader_profile(c: &reqwest::Client, shared: &Path, mc: &str, lv: &str) -> Result<Value, String> {
    if !lv.chars().all(|ch| ch.is_ascii_alphanumeric() || ".-_".contains(ch)) { return Err("Version de Fabric invalide".into()) }
    let p = shared.join("loaders").join(format!("fabric-{mc}-{lv}.json"));
    if p.exists() { return serde_json::from_str(&tokio::fs::read_to_string(&p).await.map_err(e)?).map_err(e) }
    let t = c.get(format!("{}/versions/loader/{}/{}/profile/json", config::FABRIC_META, mc, lv)).send().await
        .map_err(|_| "Connexion requise pour télécharger Fabric".to_string())?
        .error_for_status().map_err(e)?.text().await.map_err(e)?;
    tokio::fs::create_dir_all(p.parent().unwrap()).await.map_err(e)?;
    tokio::fs::write(&p, &t).await.map_err(e)?;
    serde_json::from_str(&t).map_err(e)
}

// Ajoute les bibliothèques de Fabric (en premier) et remplace la classe principale.
fn merge_fabric(v: &mut Value, prof: &Value) {
    let mut fab: Vec<Value> = vec![];
    let mut keys: HashSet<String> = HashSet::new();
    for l in prof["libraries"].as_array().into_iter().flatten() {
        let (Some(name), Some(base)) = (l["name"].as_str(), l["url"].as_str()) else { continue };
        if !base.starts_with("https://") { continue }
        let Some(path) = maven_path(name) else { continue };
        let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
        keys.insert(lib_key(name));
        fab.push(serde_json::json!({"name": name, "downloads": {"artifact": {"path": path, "url": format!("{base}{path}")}}}));
    }
    let vanilla: Vec<Value> = v["libraries"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|l| !keys.contains(&lib_key(l["name"].as_str().unwrap_or("")))).collect();
    fab.extend(vanilla);
    v["libraries"] = serde_json::json!(fab);
    if let Some(m) = prof["mainClass"].as_str() { v["mainClass"] = serde_json::json!(m); }
    for k in ["jvm", "game"] {
        if let Some(extra) = prof["arguments"][k].as_array() {
            let mut a = v["arguments"][k].as_array().cloned().unwrap_or_default();
            a.extend(extra.iter().cloned());
            v["arguments"][k] = serde_json::json!(a);
        }
    }
}

#[tauri::command]
fn list_instances(app: AppHandle) -> Vec<Value> {
    instances::load(&app).into_iter().map(|mut i| {
        let n = modrinth::count(&app, i["id"].as_str().unwrap_or(""));
        i["mods"] = serde_json::json!(n);
        i
    }).collect()
}

#[tauri::command]
async fn create_instance(app: AppHandle, name: String, version: String, loader: String) -> Result<Value, String> {
    if version.is_empty() || version.len() > 40 || !version.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)) {
        return Err("Version invalide".into());
    }
    let lv = match loader.as_str() {
        "vanilla" => None,
        "fabric" => Some(fabric_loader_version(&client()?, &version).await?),
        _ => return Err("Type d'instance inconnu".into()),
    };
    instances::create(&app, &name, &version, &loader, lv)
}

#[tauri::command]
fn update_instance(app: AppHandle, id: String, patch: Value) -> Result<Value, String> { instances::update(&app, &id, &patch) }

#[tauri::command]
fn delete_instance(app: AppHandle, id: String) -> Result<(), String> { instances::delete(&app, &id) }

#[tauri::command]
fn open_folder(app: AppHandle, id: Option<String>) -> Result<(), String> {
    let p = match id { Some(i) => instances::dir(&app, &i)?, None => root(&app) };
    std::fs::create_dir_all(&p).map_err(e)?;
    let mut c = if cfg!(target_os = "windows") { std::process::Command::new("explorer") }
        else if cfg!(target_os = "macos") { std::process::Command::new("open") }
        else { std::process::Command::new("xdg-open") };
    c.arg(&p).spawn().map(|_| ()).map_err(e)
}

#[tauri::command]
async fn mod_search(app: AppHandle, instance_id: String, query: String, offset: u32) -> Result<Value, String> {
    modrinth::search(&app, &instance_id, &query, offset).await
}

#[tauri::command]
async fn mod_install(app: AppHandle, instance_id: String, project: String) -> Result<Value, String> {
    modrinth::install(&app, &instance_id, &project).await
}

#[tauri::command]
fn mod_list(app: AppHandle, instance_id: String) -> Vec<Value> { modrinth::list(&app, &instance_id) }

#[tauri::command]
fn mod_toggle(app: AppHandle, instance_id: String, project: String) -> Result<bool, String> { modrinth::toggle(&app, &instance_id, &project) }

#[tauri::command]
fn mod_remove(app: AppHandle, instance_id: String, project: String) -> Result<(), String> { modrinth::remove(&app, &instance_id, &project) }

#[tauri::command]
async fn mod_updates(app: AppHandle, instance_id: String, apply: bool) -> Result<Vec<Value>, String> {
    modrinth::updates(&app, &instance_id, apply).await
}

#[tauri::command]
async fn launch(app: AppHandle, instance_id: String, opts: Value) -> Result<i32, String> {
    let inst = instances::get(&app, &instance_id).ok_or("Instance introuvable")?;
    let version = inst["version"].as_str().ok_or("Version manquante")?.to_string();
    let ram_gb = opts["ramGb"].as_u64().unwrap_or(4) as u32;
    let nice = opts["nice"].as_i64().unwrap_or(0) as i32;
    let hide = opts["hide"] != false;
    let shared = root(&app).join("shared");
    let game_dir = instances::dir(&app, &instance_id)?;
    tokio::fs::create_dir_all(game_dir.join("mods")).await.map_err(e)?;
    let c = client()?;

    // Compte Microsoft obligatoire (mode test réservé aux builds de développement).
    let sess = match auth::session(&c, &root(&app)).await {
        Ok(s) => s,
        Err(m) => match dev_session() {
            Some(s) => s,
            None => return Err(m),
        },
    };

    // Version déjà téléchargée : aucune connexion nécessaire.
    let vjson = shared.join("versions").join(&version).join(format!("{version}.json"));
    let mut v: Value = if vjson.exists() {
        serde_json::from_str(&tokio::fs::read_to_string(&vjson).await.map_err(e)?).map_err(e)?
    } else {
        prog(&app, "Récupération de la version", 0, 0);
        let manifest = get_json(&c, config::MOJANG_MANIFEST).await.map_err(|_| "Connexion requise pour télécharger cette version".to_string())?;
        let url = manifest["versions"].as_array().and_then(|v| v.iter().find(|x| x["id"] == version.as_str()))
            .and_then(|x| x["url"].as_str()).ok_or(format!("Version {version} introuvable"))?.to_string();
        let t = c.get(&url).send().await.map_err(e)?.error_for_status().map_err(e)?.text().await.map_err(e)?;
        tokio::fs::create_dir_all(vjson.parent().unwrap()).await.map_err(e)?;
        tokio::fs::write(&vjson, &t).await.map_err(e)?;
        serde_json::from_str(&t).map_err(e)?
    };

    if inst["loader"] == "fabric" {
        let lv = inst["loader_version"].as_str().ok_or("Version de Fabric manquante")?;
        prog(&app, "Préparation de Fabric", 0, 0);
        let prof = loader_profile(&c, &shared, &version, lv).await?;
        merge_fabric(&mut v, &prof);
    }

    prog(&app, "Préparation de Java", 0, 0);
    let java = ensure_java(&app, &c, &shared, &v).await.unwrap_or_else(|_| "java".to_string());

    // Une version déjà préparée n'est pas revérifiée : démarrage bien plus rapide.
    let loader_key = if inst["loader"] == "fabric" { format!("fabric-{}", inst["loader_version"].as_str().unwrap_or("")) } else { "vanilla".to_string() };
    let ready = shared.join("versions").join(&version).join(format!(".ready-{loader_key}"));
    let fast = ready.exists();

    let jar = shared.join("versions").join(&version).join(format!("{version}.jar"));
    if !fast {
        prog(&app, "Téléchargement du jeu", 0, 0);
        fetch(&c, v["downloads"]["client"]["url"].as_str().ok_or("client.jar manquant")?, &jar).await?;
    }

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
    if !fast {
        let nl = libs.len() as u64;
        let mut got = 0u64;
        let mut st = stream::iter(libs.into_iter().map(|(u, d)| {
            let c = c.clone();
            async move { fetch(&c, &u, &d).await }
        })).buffer_unordered(16);
        while let Some(r) = st.next().await {
            r?; got += 1;
            prog(&app, "Bibliothèques", got, nl);
        }
    }

    let idx_id = v["assetIndex"]["id"].as_str().ok_or("assetIndex manquant")?.to_string();
    let game_assets: PathBuf;
    if fast {
        game_assets = PathBuf::from(tokio::fs::read_to_string(&ready).await.map_err(e)?.trim());
    } else {
        let idx_path = shared.join("assets").join("indexes").join(format!("{idx_id}.json"));
        fetch(&c, v["assetIndex"]["url"].as_str().ok_or("assetIndex url")?, &idx_path).await?;
        let idx: Value = serde_json::from_str(&tokio::fs::read_to_string(&idx_path).await.map_err(e)?).map_err(e)?;
        let objs: Vec<(String, String)> = idx["objects"].as_object()
            .map(|o| o.iter().filter_map(|(k, x)| x["hash"].as_str().map(|h| (k.clone(), h.to_string()))).collect()).unwrap_or_default();
        let total = objs.len();
        let mut done = 0;
        {
            let jobs: Vec<(String, PathBuf)> = objs.iter().map(|(_, h)| (
                format!("https://resources.download.minecraft.net/{}/{}", &h[..2], h),
                shared.join("assets").join("objects").join(&h[..2]).join(h),
            )).collect();
            let mut st = stream::iter(jobs.into_iter().map(|(url, dest)| {
                let c = c.clone();
                async move { fetch(&c, &url, &dest).await }
            })).buffer_unordered(48);
            while let Some(r) = st.next().await {
                r?; done += 1;
                if done % 25 == 0 || done == total { prog(&app, "Ressources du jeu", done as u64, total as u64); }
            }
        }
        // Très anciennes versions : les ressources doivent être copiées à leur nom d'origine.
        let virt = idx["virtual"] == true;
        let to_res = idx["map_to_resources"] == true;
        game_assets = if virt { shared.join("assets/virtual/legacy") } else if to_res { game_dir.join("resources") } else { shared.join("assets") };
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
    }

    let natives = shared.join("natives").join(&version);
    if !fast {
        tokio::fs::create_dir_all(&natives).await.map_err(e)?;
        for j in natives_jars {
            let n = natives.clone();
            tokio::task::spawn_blocking(move || extract(&j, &n)).await.map_err(e)??;
        }
        tokio::fs::write(&ready, game_assets.to_string_lossy().as_bytes()).await.map_err(e)?;
    }

    let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let vars: Vec<(&str, String)> = vec![
        ("${auth_player_name}", sess.name.clone()),
        ("${version_name}", version.clone()),
        ("${game_directory}", game_dir.to_string_lossy().to_string()),
        ("${assets_root}", shared.join("assets").to_string_lossy().to_string()),
        ("${game_assets}", game_assets.to_string_lossy().to_string()),
        ("${assets_index_name}", idx_id),
        ("${auth_uuid}", sess.uuid.clone()),
        ("${auth_access_token}", sess.token.clone()),
        ("${auth_session}", format!("token:{}", sess.token)),
        ("${user_properties}", "{}".into()),
        ("${clientid}", "0".into()),
        ("${auth_xuid}", "0".into()),
        ("${user_type}", if sess.token == "0" { "legacy".into() } else { "msa".into() }),
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
    let ram = ram_gb.clamp(1, 32);
    let mut args: Vec<String> = vec![format!("-Xmx{ram}G"), format!("-Xms{}G", ram.min(2))];
    let gc: &[&str] = match opts["profile"].as_u64().unwrap_or(1) {
        0 => &["-XX:+UseG1GC", "-XX:ParallelGCThreads=2", "-XX:ConcGCThreads=1"],
        2 => &["-XX:+UnlockExperimentalVMOptions", "-XX:+UseG1GC", "-XX:MaxGCPauseMillis=30", "-XX:G1NewSizePercent=30", "-XX:G1MaxNewSizePercent=40", "-XX:G1ReservePercent=15", "-XX:+DisableExplicitGC", "-XX:+ParallelRefProcEnabled"],
        _ => &["-XX:+UnlockExperimentalVMOptions", "-XX:+UseG1GC", "-XX:MaxGCPauseMillis=50", "-XX:G1NewSizePercent=20", "-XX:G1ReservePercent=20"],
    };
    args.extend(gc.iter().map(|s| s.to_string()));
    // Archive de classes (Java 21+) : les lancements suivants démarrent plus vite.
    if v["javaVersion"]["majorVersion"].as_u64().unwrap_or(0) >= 21 && java != "java" {
        args.push("-XX:+AutoCreateSharedArchive".into());
        args.push(format!("-XX:SharedArchiveFile={}", game_dir.join("plume.jsa").to_string_lossy()));
    }
    args.extend(opts["jvmArgs"].as_str().unwrap_or("").split_whitespace().filter(|a| a.starts_with('-')).map(String::from));
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

    if opts["fullscreen"] == true {
        args.push("--fullscreen".into());
    } else if let (Some(w), Some(h)) = (opts["width"].as_u64(), opts["height"].as_u64()) {
        if w >= 400 && h >= 300 { args.extend(["--width".to_string(), w.to_string(), "--height".to_string(), h.to_string()]); }
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
    prog(&app, "Le jeu est en cours d'exécution", 0, 0);
    instances::touch(&app, &instance_id);
    if hide { if let Some(w) = &win { let _ = w.hide(); } }
    let status = child.wait().await.map_err(e)?;
    if let Some(w) = &win { let _ = w.show(); let _ = w.set_focus(); }
    Ok(status.code().unwrap_or(-1))
}

#[tauri::command]
fn dev_offline() -> Option<String> { dev_session().map(|s| s.name) }

// Mode test sans compte : n'existe QUE dans les builds de développement (npm run dev).
// Les versions publiées (release) exigent toujours un compte Microsoft qui possède Minecraft.
#[cfg(debug_assertions)]
fn dev_session() -> Option<auth::Session> {
    std::env::var("PLUME_DEV_OFFLINE").ok().filter(|s| !s.is_empty())
        .map(|n| auth::Session { name: n, uuid: "00000000000000000000000000000000".into(), token: "0".into() })
}
#[cfg(not(debug_assertions))]
fn dev_session() -> Option<auth::Session> { None }

#[tauri::command]
fn account_info(app: AppHandle) -> Option<Value> {
    auth::active(&root(&app)).map(|a| serde_json::json!({"name": a["name"], "uuid": a["uuid"]}))
}

#[tauri::command]
async fn login_start() -> Result<Value, String> { auth::start(&client()?).await }

#[tauri::command]
async fn login_finish(app: AppHandle, device_code: String, interval: u64, expires_in: u64) -> Result<Value, String> {
    auth::finish(&client()?, &root(&app), &device_code, interval, expires_in).await
}

#[tauri::command]
fn account_list(app: AppHandle) -> Vec<Value> { auth::list(&root(&app)) }

#[tauri::command]
fn account_switch(app: AppHandle, uuid: String) -> Result<(), String> { auth::switch(&root(&app), &uuid) }

#[tauri::command]
fn account_remove(app: AppHandle, uuid: String) -> Result<(), String> { auth::remove(&root(&app), &uuid) }

// Oublie l'état « version prête » : tous les fichiers seront revérifiés au prochain lancement.
#[tauri::command]
fn repair_instance(app: AppHandle, id: String) -> Result<(), String> {
    let inst = instances::get(&app, &id).ok_or("Instance introuvable")?;
    let v = inst["version"].as_str().unwrap_or("");
    if v.is_empty() || v.contains('/') || v.contains("..") { return Err("Version invalide".into()) }
    if let Ok(rd) = std::fs::read_dir(root(&app).join("shared").join("versions").join(v)) {
        for f in rd.flatten() {
            if f.file_name().to_string_lossy().starts_with(".ready-") { let _ = std::fs::remove_file(f.path()); }
        }
    }
    Ok(())
}

#[tauri::command]
fn logout(app: AppHandle) { auth::logout(&root(&app)) }

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") || !url.chars().all(|ch| ch.is_ascii_alphanumeric() || "/:.-_?=".contains(ch)) {
        return Err("Adresse refusée".into());
    }
    let mut c = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(&url);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    c.spawn().map(|_| ()).map_err(e)
}

#[tauri::command]
fn read_log(app: AppHandle, instance_id: String) -> String {
    let Ok(d) = instances::dir(&app, &instance_id) else { return String::new() };
    let t = std::fs::read_to_string(d.join("launcher.log")).unwrap_or_default();
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
            prog(&app, "Mise à jour", got, total.unwrap_or(0));
        },
        || {},
    ).await.map_err(e)?;
    app.restart()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![account_list, account_switch, account_remove, repair_instance, launch, read_log, list_instances, create_instance, update_instance, delete_instance, open_folder, mod_search, mod_install, mod_list, mod_toggle, mod_remove, mod_updates, list_versions, check_update, install_update, account_info, dev_offline, login_start, login_finish, logout, open_url])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Plume");
}
