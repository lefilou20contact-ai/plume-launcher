//! Gestion des instances : chacune a son dossier (mods, mondes, options) et sa version.
use crate::{e, root};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};
use tauri::AppHandle;

static LOCK: Mutex<()> = Mutex::new(());

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn file(app: &AppHandle) -> PathBuf { root(app).join("instances.json") }

pub fn load(app: &AppHandle) -> Vec<Value> {
    std::fs::read_to_string(file(app)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(app: &AppHandle, list: &[Value]) -> Result<(), String> {
    std::fs::create_dir_all(root(app)).map_err(e)?;
    let tmp = root(app).join("instances.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(list).map_err(e)?).map_err(e)?;
    std::fs::rename(tmp, file(app)).map_err(e)
}

pub fn get(app: &AppHandle, id: &str) -> Option<Value> {
    if !valid_id(id) { return None }
    load(app).into_iter().find(|i| i["id"] == id)
}

pub fn dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) { return Err("Instance invalide".into()) }
    Ok(root(app).join("instances").join(id))
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && (s.is_empty() || s.ends_with('-')) { continue }
        s.push(c);
    }
    let s: String = s.trim_end_matches('-').chars().take(30).collect();
    if s.is_empty() { "instance".into() } else { s }
}

pub fn create(app: &AppHandle, name: &str, version: &str, loader: &str, loader_version: Option<String>) -> Result<Value, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 { return Err("Le nom doit faire entre 1 et 40 caractères".into()) }
    let _g = LOCK.lock().map_err(e)?;
    let mut list = load(app);
    let base = slug(name);
    let mut id = base.clone();
    let mut n = 2;
    while list.iter().any(|i| i["id"] == id.as_str()) { id = format!("{base}-{n}"); n += 1; }
    let colors = ["#2f55f0", "#1f9d63", "#d6812b", "#b74bd6", "#d64545", "#2b9fd6"];
    let inst = json!({
        "id": id, "name": name, "version": version, "loader": loader, "loader_version": loader_version,
        "ram_gb": Value::Null, "color": colors[list.len() % colors.len()], "created": now(), "last_played": 0, "playtime": 0
    });
    std::fs::create_dir_all(dir(app, &id)?.join("mods")).map_err(e)?;
    list.push(inst.clone());
    save(app, &list)?;
    Ok(inst)
}

pub fn update(app: &AppHandle, id: &str, patch: &Value) -> Result<Value, String> {
    let _g = LOCK.lock().map_err(e)?;
    let mut list = load(app);
    let inst = list.iter_mut().find(|i| i["id"] == id).ok_or("Instance introuvable")?;
    if let Some(n) = patch["name"].as_str() {
        let n = n.trim();
        if n.is_empty() || n.chars().count() > 40 { return Err("Nom invalide".into()) }
        inst["name"] = json!(n);
    }
    if patch.get("ram_gb").is_some() {
        inst["ram_gb"] = match patch["ram_gb"].as_u64() {
            Some(r) if (1..=32).contains(&r) => json!(r),
            Some(_) => return Err("RAM invalide".into()),
            None => Value::Null,
        };
    }
    if let Some(c) = patch["color"].as_str() {
        if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit()) { inst["color"] = json!(c); }
    }
    let out = inst.clone();
    save(app, &list)?;
    Ok(out)
}

pub fn touch(app: &AppHandle, id: &str) {
    let _g = LOCK.lock();
    let mut list = load(app);
    if let Some(i) = list.iter_mut().find(|i| i["id"] == id) {
        i["last_played"] = json!(now());
        let _ = save(app, &list);
    }
}

pub fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    let _g = LOCK.lock().map_err(e)?;
    let mut list = load(app);
    let before = list.len();
    list.retain(|i| i["id"] != id);
    if list.len() == before { return Err("Instance introuvable".into()) }
    save(app, &list)?;
    let d = dir(app, id)?;
    if d.exists() { std::fs::remove_dir_all(d).map_err(e)?; }
    Ok(())
}

pub fn add_playtime(app: &AppHandle, id: &str, secs: u64) {
    let _g = LOCK.lock();
    let mut list = load(app);
    if let Some(i) = list.iter_mut().find(|i| i["id"] == id) {
        let t = i["playtime"].as_u64().unwrap_or(0) + secs;
        i["playtime"] = json!(t);
        let _ = save(app, &list);
    }
}

// Copie récursive (sans les fichiers temporaires du launcher).
pub fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(e)?;
    for f in std::fs::read_dir(src).map_err(e)?.flatten() {
        let ft = f.file_type().map_err(e)?;
        let name = f.file_name();
        if name == "launcher.log" || name == "plume.jsa" { continue }
        let to = dst.join(&name);
        if ft.is_dir() { copy_dir(&f.path(), &to)?; } else if ft.is_file() { std::fs::copy(f.path(), &to).map_err(e)?; }
    }
    Ok(())
}

// Réglages de départ d'une nouvelle instance : distance d'affichage selon le profil, langue du jeu.
pub fn write_defaults(app: &AppHandle, id: &str, lang: Option<&str>, profile: u64) {
    let Ok(d) = dir(app, id) else { return };
    let p = d.join("options.txt");
    if p.exists() { return }
    let render = match profile { 0 => 6, 2 => 12, _ => 10 };
    let mut t = format!("renderDistance:{render}\nonboardAccessibility:false\ntutorialStep:none\n");
    if let Some(l) = lang {
        if l.len() <= 6 && l.chars().all(|c| c.is_ascii_alphabetic() || c == '_') { t.push_str(&format!("lang:{l}\n")); }
    }
    let _ = std::fs::write(p, t);
}

// ---------- Export et fichiers déposés ----------
fn add_dir(z: &mut zip::ZipWriter<std::fs::File>, base: &std::path::Path, cur: &std::path::Path, opt: zip::write::SimpleFileOptions, worlds: bool) -> Result<(), String> {
    for f in std::fs::read_dir(cur).map_err(e)?.flatten() {
        let ft = f.file_type().map_err(e)?;
        let p = f.path();
        let rel = p.strip_prefix(base).map_err(e)?.to_string_lossy().replace('\\', "/");
        let top = rel.split('/').next().unwrap_or("");
        if matches!(top, "logs" | "crash-reports" | "launcher.log" | "plume.jsa" | "plume-gc.log" | ".fabric" | ".mixin.out") || (!worlds && top == "saves") { continue }
        if ft.is_dir() { add_dir(z, base, &p, opt, worlds)?; }
        else if ft.is_file() {
            z.start_file(rel, opt).map_err(e)?;
            let mut r = std::fs::File::open(&p).map_err(e)?;
            std::io::copy(&mut r, z).map_err(e)?;
        }
    }
    Ok(())
}

/// Exporte une instance dans un .zip prêt à partager (dossier « exports »).
pub fn export(app: &AppHandle, id: &str, worlds: bool) -> Result<PathBuf, String> {
    use std::io::Write;
    let inst = get(app, id).ok_or("Instance introuvable")?;
    let src = dir(app, id)?;
    let out_dir = root(app).join("exports");
    std::fs::create_dir_all(&out_dir).map_err(e)?;
    let out = out_dir.join(format!("{}-{}.zip", id, now()));
    let mut z = zip::ZipWriter::new(std::fs::File::create(&out).map_err(e)?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("plume-instance.json", opt).map_err(e)?;
    z.write_all(serde_json::to_string_pretty(&inst).map_err(e)?.as_bytes()).map_err(e)?;
    add_dir(&mut z, &src, &src, opt, worlds)?;
    z.finish().map_err(e)?;
    Ok(out)
}

/// Copie des fichiers glissés dans la fenêtre : .jar -> mods, .zip -> packs de ressources.
pub fn add_files(app: &AppHandle, id: &str, paths: &[String]) -> Result<usize, String> {
    let d = dir(app, id)?;
    let mut n = 0;
    for p in paths {
        let src = std::path::Path::new(p);
        let Some(name) = src.file_name().and_then(|x| x.to_str()) else { continue };
        if name.len() > 150 || name.contains("..") || !src.is_file() { continue }
        let lower = name.to_lowercase();
        let sub = if lower.ends_with(".jar") { "mods" } else if lower.ends_with(".zip") { "resourcepacks" } else { continue };
        std::fs::create_dir_all(d.join(sub)).map_err(e)?;
        std::fs::copy(src, d.join(sub).join(name)).map_err(e)?;
        n += 1;
    }
    Ok(n)
}
