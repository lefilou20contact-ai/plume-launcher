//! Modrinth : mods, shaders, packs de ressources, modpacks (.mrpack) et fichiers déposés à la main.
use crate::{client, config, e, instances, loaders, prog};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::{collections::HashSet, io::Read, path::{Path, PathBuf}};
use tauri::AppHandle;

// Hébergeurs autorisés pour les fichiers d'un modpack (liste de la spécification .mrpack).
const MP_HOSTS: [&str; 4] = ["https://cdn.modrinth.com/", "https://github.com/", "https://raw.githubusercontent.com/", "https://gitlab.com/"];
const CDN: [&str; 1] = ["https://cdn.modrinth.com/"];
const NO_LOADER: &[&str] = &[];

fn safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
fn kind_ok(k: &str) -> bool { matches!(k, "mod" | "resourcepack" | "shader") }
fn kind_dir(k: &str) -> &'static str { match k { "resourcepack" => "resourcepacks", "shader" => "shaderpacks", _ => "mods" } }
fn kind_ext(k: &str) -> &'static str { if k == "mod" { ".jar" } else { ".zip" } }
fn safe_name(f: &str, ext: &str) -> bool {
    Path::new(f).file_name().and_then(|n| n.to_str()) == Some(f) && f.ends_with(ext) && !f.contains("..") && f.len() <= 150
}
// Chemin relatif sans danger (pas de remontée de dossier, pas de chemin absolu).
fn safe_rel(p: &str) -> bool {
    !p.is_empty() && p.len() <= 220 && !p.starts_with('/') && !p.contains('\\') && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

fn list_file(app: &AppHandle, id: &str) -> Result<PathBuf, String> { Ok(instances::dir(app, id)?.join("plume-mods.json")) }
fn read(app: &AppHandle, id: &str) -> Vec<Value> {
    list_file(app, id).ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
fn write(app: &AppHandle, id: &str, v: &[Value]) -> Result<(), String> {
    std::fs::write(list_file(app, id)?, serde_json::to_string_pretty(v).map_err(e)?).map_err(e)
}
pub fn count(app: &AppHandle, id: &str) -> usize { if instances::valid_id(id) { list(app, id).len() + list_manual(app, id).len() } else { 0 } }

// Version de Minecraft, loaders Modrinth compatibles (vide pour shaders et packs) et dossier de l'instance.
fn ctx(app: &AppHandle, id: &str, kind: &str) -> Result<(String, &'static [&'static str], PathBuf), String> {
    if !kind_ok(kind) { return Err("Type de contenu invalide".into()) }
    let inst = instances::get(app, id).ok_or("Instance introuvable")?;
    let l: &'static [&'static str] = if kind == "mod" {
        loaders::mod_loaders(inst["loader"].as_str().unwrap_or("")).ok_or("Les mods nécessitent une instance avec un loader (Fabric, Quilt, Forge ou NeoForge).")?
    } else { NO_LOADER };
    Ok((inst["version"].as_str().unwrap_or("").to_string(), l, instances::dir(app, id)?))
}

async fn api(c: &reqwest::Client, path: &str, q: &[(&str, &str)]) -> Result<Value, String> {
    c.get(format!("{}{}", config::MODRINTH_API, path)).query(q).send().await
        .map_err(|_| "Modrinth est injoignable (vérifie ta connexion).".to_string())?
        .error_for_status().map_err(e)?
        .json::<Value>().await.map_err(e)
}

async fn best_version(c: &reqwest::Client, project: &str, mc: Option<&str>, ls: &[&str]) -> Result<Value, String> {
    let mut q: Vec<(&str, String)> = vec![];
    if let Some(m) = mc { q.push(("game_versions", json!([m]).to_string())); }
    if !ls.is_empty() { q.push(("loaders", json!(ls).to_string())); }
    let qq: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let v = api(c, &format!("/project/{project}/version"), &qq).await?;
    let arr = v.as_array().ok_or("Réponse invalide")?;
    arr.iter().find(|x| x["version_type"] == "release").or(arr.first()).cloned()
        .ok_or(match mc { Some(m) => format!("Aucune version compatible avec Minecraft {m}"), None => "Aucune version disponible".to_string() })
}

fn primary(ver: &Value) -> Result<&Value, String> {
    let files = ver["files"].as_array().ok_or("Aucun fichier disponible")?;
    files.iter().find(|f| f["primary"] == true).or(files.first()).ok_or("Aucun fichier disponible".to_string())
}

// Télécharge (3 essais), vérifie la somme SHA-512 et refuse tout hébergeur non autorisé.
async fn get_checked(c: &reqwest::Client, url: &str, sha512: &str, hosts: &[&str]) -> Result<Vec<u8>, String> {
    if !hosts.iter().any(|h| url.starts_with(h)) { return Err("Adresse de téléchargement refusée".into()) }
    let mut last = String::new();
    for attempt in 0..3u64 {
        if attempt > 0 { tokio::time::sleep(std::time::Duration::from_millis(600 * attempt)).await; }
        let got = async { Ok::<_, String>(c.get(url).send().await.map_err(e)?.error_for_status().map_err(e)?.bytes().await.map_err(e)?) }.await;
        match got {
            Ok(b) => {
                if format!("{:x}", Sha512::digest(&b)) != sha512.to_lowercase() { return Err("Somme de contrôle invalide".into()) }
                return Ok(b.to_vec());
            }
            Err(m) => last = m,
        }
    }
    Err(format!("Téléchargement impossible : {last}"))
}

async fn download(c: &reqwest::Client, file: &Value, dir: &Path, ext: &str) -> Result<String, String> {
    let url = file["url"].as_str().ok_or("Fichier invalide")?;
    let name = file["filename"].as_str().ok_or("Nom de fichier manquant")?;
    if !safe_name(name, ext) { return Err("Nom de fichier refusé".into()) }
    let want = file["hashes"]["sha512"].as_str().ok_or("Somme de contrôle manquante")?;
    let bytes = match get_checked(c, url, want, &CDN).await {
        Ok(b) => b,
        Err(m) if m == "Somme de contrôle invalide" => return Err(format!("{name} : somme de contrôle invalide, fichier refusé")),
        Err(m) => return Err(m),
    };
    tokio::fs::create_dir_all(dir).await.map_err(e)?;
    tokio::fs::write(dir.join(name), &bytes).await.map_err(e)?;
    Ok(name.to_string())
}

fn hits(r: &Value) -> Value {
    let h: Vec<Value> = r["hits"].as_array().into_iter().flatten().map(|h| json!({
        "id": h["project_id"], "title": h["title"], "description": h["description"],
        "icon_url": h["icon_url"], "downloads": h["downloads"], "author": h["author"]
    })).collect();
    json!({"hits": h, "total": r["total_hits"]})
}

pub async fn search(app: &AppHandle, id: &str, query: &str, offset: u32, kind: &str) -> Result<Value, String> {
    let (mc, ls, _) = ctx(app, id, kind)?;
    let mut facets = vec![json!([format!("project_type:{kind}")]), json!([format!("versions:{mc}")])];
    if !ls.is_empty() { facets.push(json!(ls.iter().map(|l| format!("categories:{l}")).collect::<Vec<_>>())); }
    let facets = Value::Array(facets).to_string();
    let off = offset.to_string();
    let r = api(&client()?, "/search", &[("query", query), ("facets", facets.as_str()), ("limit", "20"), ("offset", off.as_str())]).await?;
    Ok(hits(&r))
}

pub async fn search_modpacks(query: &str, offset: u32) -> Result<Value, String> {
    let facets = json!([["project_type:modpack"]]).to_string();
    let off = offset.to_string();
    let r = api(&client()?, "/search", &[("query", query), ("facets", facets.as_str()), ("limit", "20"), ("offset", off.as_str())]).await?;
    Ok(hits(&r))
}

pub async fn install(app: &AppHandle, id: &str, project: &str, kind: &str) -> Result<Value, String> {
    if !safe(project) { return Err("Projet invalide".into()) }
    let (mc, ls, idir) = ctx(app, id, kind)?;
    let dir = idir.join(kind_dir(kind));
    let c = client()?;
    let mut list = read(app, id);
    let mut queue = vec![(project.to_string(), false)];
    let mut seen: HashSet<String> = HashSet::new();
    let mut added: Vec<Value> = vec![];
    while let Some((p, auto)) = queue.pop() {
        if seen.len() >= 30 || !seen.insert(p.clone()) { continue }
        if list.iter().any(|m| m["project_id"] == p.as_str() || m["slug"] == p.as_str()) { continue }
        let ver = match best_version(&c, &p, Some(mc.as_str()), ls).await {
            Ok(v) => v,
            Err(m) => { if auto { continue } else { return Err(m) } }
        };
        let pid = ver["project_id"].as_str().ok_or("Réponse invalide")?.to_string();
        if list.iter().any(|m| m["project_id"] == pid.as_str()) { continue }
        let proj = api(&c, &format!("/project/{pid}"), &[]).await.unwrap_or(Value::Null);
        let title = proj["title"].as_str().unwrap_or(p.as_str()).to_string();
        prog(app, &format!("Installation de {title}"), 0, 0);
        let filename = download(&c, primary(&ver)?, &dir, kind_ext(kind)).await?;
        let entry = json!({
            "project_id": pid, "slug": proj["slug"], "title": title, "icon_url": proj["icon_url"], "kind": kind,
            "version_id": ver["id"], "version_number": ver["version_number"], "filename": filename, "auto": auto
        });
        list.push(entry.clone());
        added.push(entry);
        write(app, id, &list)?;
        if kind == "mod" {
            for d in ver["dependencies"].as_array().into_iter().flatten() {
                if d["dependency_type"] == "required" {
                    if let Some(dp) = d["project_id"].as_str() { if safe(dp) { queue.push((dp.to_string(), true)); } }
                }
            }
        }
    }
    Ok(json!(added))
}

pub fn list(app: &AppHandle, id: &str) -> Vec<Value> {
    let Ok(d) = instances::dir(app, id) else { return vec![] };
    read(app, id).into_iter().filter_map(|mut m| {
        let kind = m["kind"].as_str().unwrap_or("mod").to_string();
        if !kind_ok(&kind) { return None }
        let f = m["filename"].as_str()?.to_string();
        if !safe_name(&f, kind_ext(&kind)) { return None }
        let md = d.join(kind_dir(&kind));
        let on = md.join(&f).exists();
        let off = md.join(format!("{f}.disabled")).exists();
        if !on && !off { return None }
        m["enabled"] = json!(on);
        m["kind"] = json!(kind);
        Some(m)
    }).collect()
}

fn find_file(app: &AppHandle, id: &str, project: &str) -> Result<(PathBuf, String), String> {
    let list = read(app, id);
    let m = list.iter().find(|m| m["project_id"] == project).ok_or("Mod introuvable")?;
    let kind = m["kind"].as_str().unwrap_or("mod");
    if !kind_ok(kind) { return Err("Type de contenu invalide".into()) }
    let f = m["filename"].as_str().ok_or("Fichier manquant")?;
    if !safe_name(f, kind_ext(kind)) { return Err("Nom de fichier refusé".into()) }
    Ok((instances::dir(app, id)?.join(kind_dir(kind)), f.to_string()))
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
    let c = client()?;
    let mut list = read(app, id);
    let mut out: Vec<Value> = vec![];
    for i in 0..list.len() {
        let kind = list[i]["kind"].as_str().unwrap_or("mod").to_string();
        let Ok((mc, ls, idir)) = ctx(app, id, &kind) else { continue };
        let (dir, ext) = (idir.join(kind_dir(&kind)), kind_ext(&kind));
        let pid = list[i]["project_id"].as_str().unwrap_or("").to_string();
        if !safe(&pid) { continue }
        let Ok(ver) = best_version(&c, &pid, Some(mc.as_str()), ls).await else { continue };
        if ver["id"] == list[i]["version_id"] { continue }
        out.push(json!({"project_id": pid, "title": list[i]["title"], "from": list[i]["version_number"], "to": ver["version_number"]}));
        if !apply { continue }
        let old = list[i]["filename"].as_str().unwrap_or("").to_string();
        if !safe_name(&old, ext) { continue }
        prog(app, &format!("Mise à jour de {}", list[i]["title"].as_str().unwrap_or("mod")), 0, 0);
        let was_off = !dir.join(&old).exists() && dir.join(format!("{old}.disabled")).exists();
        let newname = download(&c, primary(&ver)?, &dir, ext).await?;
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

// ---------- Mods ajoutés à la main ----------
pub fn list_manual(app: &AppHandle, id: &str) -> Vec<Value> {
    let Ok(d) = instances::dir(app, id) else { return vec![] };
    let tracked: HashSet<String> = read(app, id).iter().filter_map(|m| m["filename"].as_str().map(String::from)).collect();
    let mut out: Vec<Value> = vec![];
    if let Ok(rd) = std::fs::read_dir(d.join("mods")) {
        for f in rd.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            let (base, on) = if let Some(b) = n.strip_suffix(".jar.disabled") { (format!("{b}.jar"), false) }
                else if n.ends_with(".jar") { (n.clone(), true) } else { continue };
            if tracked.contains(&base) || !safe_name(&base, ".jar") { continue }
            out.push(json!({"filename": base, "title": base.trim_end_matches(".jar"), "manual": true, "enabled": on, "project_id": Value::Null, "kind": "mod"}));
        }
    }
    out.sort_by(|a, b| a["title"].as_str().cmp(&b["title"].as_str()));
    out
}

pub fn toggle_file(app: &AppHandle, id: &str, filename: &str) -> Result<bool, String> {
    if !safe_name(filename, ".jar") { return Err("Nom de fichier refusé".into()) }
    let md = instances::dir(app, id)?.join("mods");
    let on = md.join(filename);
    let off = md.join(format!("{filename}.disabled"));
    if on.exists() { std::fs::rename(&on, &off).map_err(e)?; Ok(false) }
    else if off.exists() { std::fs::rename(&off, &on).map_err(e)?; Ok(true) }
    else { Err("Fichier manquant".into()) }
}

pub fn remove_file(app: &AppHandle, id: &str, filename: &str) -> Result<(), String> {
    if !safe_name(filename, ".jar") { return Err("Nom de fichier refusé".into()) }
    let md = instances::dir(app, id)?.join("mods");
    let _ = std::fs::remove_file(md.join(filename));
    let _ = std::fs::remove_file(md.join(format!("{filename}.disabled")));
    Ok(())
}

// ---------- Modpacks (.mrpack) ----------
fn extract_overrides(path: &Path, dest: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).map_err(e)?).map_err(e)?;
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(e)?;
        if f.is_dir() { continue }
        let name = f.name().to_string();
        let Some(rel) = name.strip_prefix("overrides/").or_else(|| name.strip_prefix("client-overrides/")) else { continue };
        if !safe_rel(rel) { continue }
        total += f.size();
        if total > 3 * 1024 * 1024 * 1024 || i > 100_000 { return Err("Modpack trop volumineux".into()) }
        let out = dest.join(rel);
        if let Some(p) = out.parent() { std::fs::create_dir_all(p).map_err(e)?; }
        let mut o = std::fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut f, &mut o).map_err(e)?;
    }
    Ok(())
}

async fn fill_instance(app: &AppHandle, c: &reqwest::Client, id: &str, index: &Value, path: &Path) -> Result<(), String> {
    let idir = instances::dir(app, id)?;
    let mut jobs: Vec<(String, PathBuf, String)> = vec![];
    for f in index["files"].as_array().into_iter().flatten() {
        if f["env"]["client"] == "unsupported" { continue }
        let rel = f["path"].as_str().unwrap_or("");
        if !safe_rel(rel) { return Err("Chemin refusé dans le modpack".into()) }
        let Some(url) = f["downloads"][0].as_str() else { continue };
        let Some(h) = f["hashes"]["sha512"].as_str() else { return Err("Somme de contrôle manquante".into()) };
        jobs.push((url.to_string(), idir.join(rel), h.to_string()));
    }
    let total = jobs.len() as u64;
    let mut done = 0u64;
    {
        let mut st = stream::iter(jobs.into_iter().map(|(u, d, h)| {
            let c = c.clone();
            async move {
                let b = get_checked(&c, &u, &h, &MP_HOSTS).await?;
                if let Some(p) = d.parent() { tokio::fs::create_dir_all(p).await.map_err(e)?; }
                tokio::fs::write(&d, &b).await.map_err(e)
            }
        })).buffer_unordered(8);
        while let Some(r) = st.next().await {
            r?; done += 1;
            prog(app, "Téléchargement du modpack", done, total);
        }
    }
    let (p2, d2) = (path.to_path_buf(), idir.clone());
    tokio::task::spawn_blocking(move || extract_overrides(&p2, &d2)).await.map_err(e)??;
    Ok(())
}

/// Crée une instance à partir d'un fichier .mrpack (loader, version de Minecraft, mods et fichiers du modpack).
pub async fn import_mrpack(app: &AppHandle, path: &Path) -> Result<Value, String> {
    let is_pack = path.extension().and_then(|x| x.to_str()).map_or(false, |x| x.eq_ignore_ascii_case("mrpack"));
    if !path.is_file() || !is_pack { return Err("Fichier .mrpack invalide".into()) }
    let index: Value = {
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).map_err(e)?).map_err(e)?;
        let mut f = z.by_name("modrinth.index.json").map_err(|_| "Ce fichier n'est pas un modpack Modrinth (modrinth.index.json introuvable)".to_string())?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(e)?;
        serde_json::from_str(&s).map_err(e)?
    };
    let deps = &index["dependencies"];
    let mc = deps["minecraft"].as_str().ok_or("Version de Minecraft manquante dans le modpack")?.to_string();
    let (loader, lv): (&str, Option<String>) = if let Some(v) = deps["fabric-loader"].as_str() { ("fabric", Some(v.to_string())) }
        else if let Some(v) = deps["quilt-loader"].as_str() { ("quilt", Some(v.to_string())) }
        else if let Some(v) = deps["neoforge"].as_str() { ("neoforge", Some(v.to_string())) }
        else if let Some(v) = deps["forge"].as_str() { ("forge", Some(v.to_string())) }
        else { ("vanilla", None) };
    let okc = |s: &str| !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c));
    if !okc(&mc) || lv.as_deref().map_or(false, |v| !okc(v)) { return Err("Version invalide".into()) }
    let c = client()?;
    let shared = crate::root(app).join("shared");
    prog(app, "Création de l'instance", 0, 0);
    if loader == "forge" || loader == "neoforge" {
        loaders::install_forge_like(app, &c, &shared, loader, &mc, lv.as_deref().unwrap_or("")).await?;
    }
    let name: String = index["name"].as_str().unwrap_or("Modpack").chars().take(40).collect();
    let inst = instances::create(app, &name, &mc, loader, lv)?;
    let id = inst["id"].as_str().unwrap_or("").to_string();
    let res = fill_instance(app, &c, &id, &index, path).await;
    if res.is_err() { let _ = instances::delete(app, &id); }
    res?;
    Ok(inst)
}

/// Télécharge un modpack depuis Modrinth puis l'importe.
pub async fn install_modpack(app: &AppHandle, project: &str) -> Result<Value, String> {
    if !safe(project) { return Err("Projet invalide".into()) }
    let c = client()?;
    let ver = best_version(&c, project, None, NO_LOADER).await?;
    let tmp = crate::root(app).join("shared").join("tmp");
    prog(app, "Téléchargement du modpack", 0, 0);
    let name = download(&c, primary(&ver)?, &tmp, ".mrpack").await?;
    let p = tmp.join(&name);
    let r = import_mrpack(app, &p).await;
    let _ = tokio::fs::remove_file(&p).await;
    r
}
