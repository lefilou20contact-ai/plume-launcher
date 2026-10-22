//! Mods Modrinth : recherche, installation (avec dépendances), activation, mises à jour.
use crate::{client, config, e, instances, prog};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::{collections::HashSet, path::{Path, PathBuf}};
use tauri::AppHandle;

fn safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn safe_name(f: &str) -> bool {
    Path::new(f).file_name().and_then(|n| n.to_str()) == Some(f) && f.ends_with(".jar") && !f.contains("..") && f.len() <= 150
}

fn list_file(app: &AppHandle, id: &str) -> Result<PathBuf, String> { Ok(instances::dir(app, id)?.join("plume-mods.json")) }

fn read(app: &AppHandle, id: &str) -> Vec<Value> {
    list_file(app, id).ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write(app: &AppHandle, id: &str, v: &[Value]) -> Result<(), String> {
    std::fs::write(list_file(app, id)?, serde_json::to_string_pretty(v).map_err(e)?).map_err(e)
}

pub fn count(app: &AppHandle, id: &str) -> usize { if instances::valid_id(id) { list(app, id).len() + list_manual(app, id).len() } else { 0 } }

// Version de Minecraft + dossier des mods d'une instance Fabric.
fn ctx(app: &AppHandle, id: &str) -> Result<(String, PathBuf), String> {
    let inst = instances::get(app, id).ok_or("Instance introuvable")?;
    if inst["loader"] != "fabric" { return Err("Les mods nécessitent une instance Fabric.".into()) }
    Ok((inst["version"].as_str().unwrap_or("").to_string(), instances::dir(app, id)?.join("mods")))
}

async fn api(c: &reqwest::Client, path: &str, q: &[(&str, &str)]) -> Result<Value, String> {
    c.get(format!("{}{}", config::MODRINTH_API, path)).query(q).send().await
        .map_err(|_| "Modrinth est injoignable (vérifie ta connexion).".to_string())?
        .error_for_status().map_err(e)?
        .json::<Value>().await.map_err(e)
}

async fn best_version(c: &reqwest::Client, project: &str, mc: &str) -> Result<Value, String> {
    let gv = json!([mc]).to_string();
    let lv = json!(["fabric"]).to_string();
    let v = api(c, &format!("/project/{project}/version"), &[("game_versions", gv.as_str()), ("loaders", lv.as_str())]).await?;
    let arr = v.as_array().ok_or("Réponse invalide")?;
    arr.iter().find(|x| x["version_type"] == "release").or(arr.first()).cloned()
        .ok_or(format!("Aucune version compatible avec Minecraft {mc} et Fabric"))
}

fn primary(ver: &Value) -> Result<&Value, String> {
    let files = ver["files"].as_array().ok_or("Aucun fichier disponible")?;
    files.iter().find(|f| f["primary"] == true).or(files.first()).ok_or("Aucun fichier disponible".to_string())
}

// Télécharge, vérifie la somme SHA-512, puis écrit le fichier.
async fn download(c: &reqwest::Client, file: &Value, dir: &Path) -> Result<String, String> {
    let url = file["url"].as_str().ok_or("Fichier invalide")?;
    if !url.starts_with("https://cdn.modrinth.com/") { return Err("Adresse de téléchargement refusée".into()) }
    let name = file["filename"].as_str().ok_or("Nom de fichier manquant")?;
    if !safe_name(name) { return Err("Nom de fichier refusé".into()) }
    let want = file["hashes"]["sha512"].as_str().ok_or("Somme de contrôle manquante")?.to_lowercase();
    let bytes = c.get(url).send().await.map_err(e)?.error_for_status().map_err(e)?.bytes().await.map_err(e)?;
    if format!("{:x}", Sha512::digest(&bytes)) != want { return Err(format!("{name} : somme de contrôle invalide, fichier refusé")) }
    tokio::fs::create_dir_all(dir).await.map_err(e)?;
    tokio::fs::write(dir.join(name), &bytes).await.map_err(e)?;
    Ok(name.to_string())
}

pub async fn search(app: &AppHandle, id: &str, query: &str, offset: u32) -> Result<Value, String> {
    let (mc, _) = ctx(app, id)?;
    let facets = json!([["project_type:mod"], [format!("versions:{mc}")], ["categories:fabric"]]).to_string();
    let off = offset.to_string();
    let r = api(&client()?, "/search", &[("query", query), ("facets", facets.as_str()), ("limit", "20"), ("offset", off.as_str())]).await?;
    let hits: Vec<Value> = r["hits"].as_array().into_iter().flatten().map(|h| json!({
        "id": h["project_id"], "title": h["title"], "description": h["description"],
        "icon_url": h["icon_url"], "downloads": h["downloads"], "author": h["author"]
    })).collect();
    Ok(json!({"hits": hits, "total": r["total_hits"]}))
}

pub async fn install(app: &AppHandle, id: &str, project: &str) -> Result<Value, String> {
    if !safe(project) { return Err("Projet invalide".into()) }
    let (mc, dir) = ctx(app, id)?;
    let c = client()?;
    let mut list = read(app, id);
    let mut queue = vec![(project.to_string(), false)];
    let mut seen: HashSet<String> = HashSet::new();
    let mut added: Vec<Value> = vec![];
    while let Some((p, auto)) = queue.pop() {
        if seen.len() >= 30 || !seen.insert(p.clone()) { continue }
        if list.iter().any(|m| m["project_id"] == p.as_str() || m["slug"] == p.as_str()) { continue }
        let ver = match best_version(&c, &p, &mc).await {
            Ok(v) => v,
            Err(m) => { if auto { continue } else { return Err(m) } }
        };
        let pid = ver["project_id"].as_str().ok_or("Réponse invalide")?.to_string();
        if list.iter().any(|m| m["project_id"] == pid.as_str()) { continue }
        let proj = api(&c, &format!("/project/{pid}"), &[]).await.unwrap_or(Value::Null);
        let title = proj["title"].as_str().unwrap_or(p.as_str()).to_string();
        prog(app, &format!("Installation de {title}"), 0, 0);
        let filename = download(&c, primary(&ver)?, &dir).await?;
        let entry = json!({
            "project_id": pid, "slug": proj["slug"], "title": title, "icon_url": proj["icon_url"],
            "version_id": ver["id"], "version_number": ver["version_number"], "filename": filename, "auto": auto
        });
        list.push(entry.clone());
        added.push(entry);
        write(app, id, &list)?;
        for d in ver["dependencies"].as_array().into_iter().flatten() {
            if d["dependency_type"] == "required" {
                if let Some(dp) = d["project_id"].as_str() { if safe(dp) { queue.push((dp.to_string(), true)); } }
            }
        }
    }
    Ok(json!(added))
}

pub fn list(app: &AppHandle, id: &str) -> Vec<Value> {
    let Ok(d) = instances::dir(app, id) else { return vec![] };
    let md = d.join("mods");
    read(app, id).into_iter().filter_map(|mut m| {
        let f = m["filename"].as_str()?.to_string();
        if !safe_name(&f) { return None }
        let on = md.join(&f).exists();
        let off = md.join(format!("{f}.disabled")).exists();
        if !on && !off { return None }
        m["enabled"] = json!(on);
        Some(m)
    }).collect()
}

fn find_file(app: &AppHandle, id: &str, project: &str) -> Result<(PathBuf, String), String> {
    let md = instances::dir(app, id)?.join("mods");
    let list = read(app, id);
    let m = list.iter().find(|m| m["project_id"] == project).ok_or("Mod introuvable")?;
    let f = m["filename"].as_str().ok_or("Entrée invalide")?;
    if !safe_name(f) { return Err("Nom de fichier refusé".into()) }
    Ok((md, f.to_string()))
}

pub fn toggle(app: &AppHandle, id: &str, project: &str) -> Result<bool, String> {
    let (md, f) = find_file(app, id, project)?;
    let on = md.join(&f);
    let off = md.join(format!("{f}.disabled"));
    if on.exists() { std::fs::rename(&on, &off).map_err(e)?; Ok(false) }
    else if off.exists() { std::fs::rename(&off, &on).map_err(e)?; Ok(true) }
    else { Err("Fichier manquant".into()) }
}

pub fn remove(app: &AppHandle, id: &str, project: &str) -> Result<(), String> {
    let (md, f) = find_file(app, id, project)?;
    let _ = std::fs::remove_file(md.join(&f));
    let _ = std::fs::remove_file(md.join(format!("{f}.disabled")));
    let mut list = read(app, id);
    list.retain(|m| m["project_id"] != project);
    write(app, id, &list)
}

// Liste les mises à jour disponibles ; les installe si `apply` est vrai.
pub async fn updates(app: &AppHandle, id: &str, apply: bool) -> Result<Vec<Value>, String> {
    let (mc, dir) = ctx(app, id)?;
    let c = client()?;
    let mut list = read(app, id);
    let mut out: Vec<Value> = vec![];
    for i in 0..list.len() {
        let pid = list[i]["project_id"].as_str().unwrap_or("").to_string();
        if !safe(&pid) { continue }
        let Ok(ver) = best_version(&c, &pid, &mc).await else { continue };
        if ver["id"] == list[i]["version_id"] { continue }
        out.push(json!({"project_id": pid, "title": list[i]["title"], "from": list[i]["version_number"], "to": ver["version_number"]}));
        if !apply { continue }
        let old = list[i]["filename"].as_str().unwrap_or("").to_string();
        if !safe_name(&old) { continue }
        prog(app, &format!("Mise à jour de {}", list[i]["title"].as_str().unwrap_or("mod")), 0, 0);
        let was_off = !dir.join(&old).exists() && dir.join(format!("{old}.disabled")).exists();
        let newname = download(&c, primary(&ver)?, &dir).await?;
        if was_off { std::fs::rename(dir.join(&newname), dir.join(format!("{newname}.disabled"))).map_err(e)?; }
        if old != newname {
            let _ = std::fs::remove_file(dir.join(&old));
            let _ = std::fs::remove_file(dir.join(format!("{old}.disabled")));
        }
        list[i]["version_id"] = ver["id"].clone();
        list[i]["version_number"] = ver["version_number"].clone();
        list[i]["filename"] = json!(newname);
        write(app, id, &list)?;
    }
    Ok(out)
}

// Mods ajoutés à la main dans le dossier (non installés via Plume).
pub fn list_manual(app: &AppHandle, id: &str) -> Vec<Value> {
    let Ok(d) = instances::dir(app, id) else { return vec![] };
    let tracked: HashSet<String> = read(app, id).iter().filter_map(|m| m["filename"].as_str().map(String::from)).collect();
    let mut out: Vec<Value> = vec![];
    if let Ok(rd) = std::fs::read_dir(d.join("mods")) {
        for f in rd.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            let (base, on) = if let Some(b) = n.strip_suffix(".jar.disabled") { (format!("{b}.jar"), false) }
                else if n.ends_with(".jar") { (n.clone(), true) } else { continue };
            if tracked.contains(&base) || !safe_name(&base) { continue }
            out.push(json!({"filename": base, "title": base.trim_end_matches(".jar"), "manual": true, "enabled": on, "project_id": Value::Null}));
        }
    }
    out.sort_by(|a, b| a["title"].as_str().cmp(&b["title"].as_str()));
    out
}

pub fn toggle_file(app: &AppHandle, id: &str, filename: &str) -> Result<bool, String> {
    if !safe_name(filename) { return Err("Nom de fichier refusé".into()) }
    let md = instances::dir(app, id)?.join("mods");
    let on = md.join(filename);
    let off = md.join(format!("{filename}.disabled"));
    if on.exists() { std::fs::rename(&on, &off).map_err(e)?; Ok(false) }
    else if off.exists() { std::fs::rename(&off, &on).map_err(e)?; Ok(true) }
    else { Err("Fichier manquant".into()) }
}

pub fn remove_file(app: &AppHandle, id: &str, filename: &str) -> Result<(), String> {
    if !safe_name(filename) { return Err("Nom de fichier refusé".into()) }
    let md = instances::dir(app, id)?.join("mods");
    let _ = std::fs::remove_file(md.join(filename));
    let _ = std::fs::remove_file(md.join(format!("{filename}.disabled")));
    Ok(())
}
