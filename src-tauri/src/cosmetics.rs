//! Intégration du mod Plume Cosmétiques : installation par instance, catalogue, choix de la tenue.
use crate::{auth, client, config, e, get_json, instances, modrinth, prog, root};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::{sync::Mutex, time::{SystemTime, UNIX_EPOCH}};
use tauri::AppHandle;

const SUPPORTED_MC: &[&str] = &["1.21.4"];
const JAR: &str = "plume-cosmetics.jar";
static TOKEN: Mutex<Option<(String, String, u64)>> = Mutex::new(None);

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

// Adresse du serveur : https://<projet>.supabase.co/functions/v1 (secret PLUME_BACKEND_URL à la compilation).
fn backend() -> Result<String, String> {
    let b = config::BACKEND_URL.trim_end_matches('/');
    if b.is_empty() || !b.starts_with("https://") { return Err("Serveur de cosmétiques non configuré".into()) }
    Ok(format!("{b}/plume"))
}

fn host(url: &str) -> Option<&str> { url.strip_prefix("https://")?.split('/').next() }

async fn send(c: &reqwest::Client, req: reqwest::RequestBuilder, bearer: Option<&str>) -> Result<Value, String> {
    let req = match bearer { Some(t) => req.bearer_auth(t), None => req };
    let _ = c;
    let r = req.send().await.map_err(|_| "Serveur de cosmétiques injoignable".to_string())?;
    if !r.status().is_success() { return Err("Le serveur de cosmétiques a refusé la demande".into()) }
    r.json::<Value>().await.map_err(|_| "Réponse du serveur invalide".to_string())
}

#[tauri::command]
pub fn cosmetics_status(app: AppHandle, instance_id: String) -> Result<Value, String> {
    let inst = instances::get(&app, &instance_id).ok_or("Instance introuvable")?;
    let loader = inst["loader"].as_str().unwrap_or("");
    let mc = inst["version"].as_str().unwrap_or("");
    let supported = (loader == "fabric" || loader == "quilt") && SUPPORTED_MC.contains(&mc);
    let installed = instances::dir(&app, &instance_id)?.join("mods").join(JAR).exists();
    Ok(json!({"supported": supported, "installed": installed, "backend": backend().is_ok(), "mc": SUPPORTED_MC}))
}

// Cherche, dans les Releases du dépôt, la dernière version du mod pour cette version de Minecraft.
async fn find_release(c: &reqwest::Client, mc: &str) -> Result<(String, String, String), String> {
    let v = get_json(c, &format!("https://api.github.com/repos/{}/releases?per_page=30", config::COSMETICS_REPO)).await
        .map_err(|_| "Impossible de contacter GitHub".to_string())?;
    let prefix = format!("https://github.com/{}/releases/download/", config::COSMETICS_REPO);
    let suffix = format!("+mc{mc}.jar");
    for r in v.as_array().into_iter().flatten() {
        let tag = r["tag_name"].as_str().unwrap_or("");
        if !tag.starts_with("mod-v") || r["draft"] == true || r["prerelease"] == true { continue }
        let assets = r["assets"].as_array().cloned().unwrap_or_default();
        let url_of = |name: &str| assets.iter().find(|a| a["name"] == name).and_then(|a| a["browser_download_url"].as_str()).map(String::from);
        let jar = assets.iter().filter_map(|a| a["name"].as_str()).find(|n| n.starts_with("plume-cosmetics-") && n.ends_with(&suffix));
        if let Some(j) = jar {
            if let (Some(ju), Some(su)) = (url_of(j), url_of(&format!("{j}.sha512"))) {
                if ju.starts_with(&prefix) && su.starts_with(&prefix) { return Ok((tag.to_string(), ju, su)); }
            }
        }
    }
    Err("Aucune version du mod pour cette version de Minecraft".into())
}

#[tauri::command]
pub async fn cosmetics_enable(app: AppHandle, instance_id: String, enable: bool) -> Result<Value, String> {
    let inst = instances::get(&app, &instance_id).ok_or("Instance introuvable")?;
    let mods = instances::dir(&app, &instance_id)?.join("mods");
    let jar_path = mods.join(JAR);
    if !enable {
        let _ = std::fs::remove_file(&jar_path);
        modrinth::untrack(&app, &instance_id, "plume-cosmetics")?;
        return Ok(json!({"installed": false}));
    }
    let loader = inst["loader"].as_str().unwrap_or("");
    let mc = inst["version"].as_str().unwrap_or("");
    if !((loader == "fabric" || loader == "quilt") && SUPPORTED_MC.contains(&mc)) { return Err("Mod indisponible pour cette instance".into()) }
    let c = client()?;
    prog(&app, "Installation de Plume Cosmétiques", 0, 0);
    let (tag, jar_url, sha_url) = find_release(&c, mc).await?;
    let want = c.get(&sha_url).send().await.map_err(e)?.error_for_status().map_err(e)?.text().await.map_err(e)?.trim().to_lowercase();
    if want.len() != 128 || !want.chars().all(|ch| ch.is_ascii_hexdigit()) { return Err("Somme de contrôle manquante".into()) }
    let bytes = c.get(&jar_url).send().await.map_err(e)?.error_for_status().map_err(e)?.bytes().await.map_err(e)?;
    if bytes.len() > 20_000_000 || format!("{:x}", Sha512::digest(&bytes)) != want { return Err("Somme de contrôle invalide".into()) }
    tokio::fs::create_dir_all(&mods).await.map_err(e)?;
    tokio::fs::write(&jar_path, &bytes).await.map_err(e)?;
    modrinth::track(&app, &instance_id, json!({
        "project_id": "plume-cosmetics", "slug": "plume-cosmetics", "title": "Plume Cosmétiques", "icon_url": Value::Null, "kind": "mod",
        "version_id": tag, "version_number": tag.trim_start_matches("mod-v"), "filename": JAR, "auto": false
    }))?;
    Ok(json!({"installed": true, "version": tag}))
}

// Preuve d'identité : le launcher « rejoint » un défi auprès de Mojang, le serveur le vérifie puis délivre un jeton d'une heure.
async fn token(app: &AppHandle, c: &reqwest::Client) -> Result<String, String> {
    let s = auth::session(c, &root(app)).await?;
    if s.token == "0" { return Err("Connexion internet requise pour les cosmétiques".into()) }
    if let Ok(g) = TOKEN.lock() {
        if let Some((u, t, exp)) = g.as_ref() { if *u == s.uuid && *exp > now() + 60 { return Ok(t.clone()) } }
    }
    let base = backend()?;
    let ch = send(c, c.post(format!("{base}/auth/challenge")).json(&json!({})), None).await?;
    let sid = ch["server_id"].as_str().ok_or("Réponse du serveur invalide")?.to_string();
    let j = c.post("https://sessionserver.mojang.com/session/minecraft/join")
        .json(&json!({"accessToken": s.token, "selectedProfile": s.uuid, "serverId": sid}))
        .send().await.map_err(e)?;
    if !j.status().is_success() { return Err("Mojang a refusé la vérification de l'identité".into()) }
    let v = send(c, c.post(format!("{base}/auth/verify")).json(&json!({"name": s.name, "server_id": sid})), None).await?;
    let t = v["token"].as_str().ok_or("Réponse du serveur invalide")?.to_string();
    if let Ok(mut g) = TOKEN.lock() { *g = Some((s.uuid.clone(), t.clone(), now() + v["expires_in"].as_u64().unwrap_or(3600))); }
    Ok(t)
}

// Aperçu du cosmétique : téléchargé une fois, gardé en cache, renvoyé sous forme d'image intégrée (aucune ouverture de l'interface vers Internet).
async fn preview(app: &AppHandle, c: &reqwest::Client, base: &str, id: &str, url: Option<&str>) -> Value {
    let Some(url) = url else { return Value::Null };
    let ok_id = !id.is_empty() && id.len() <= 64 && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if !ok_id || host(url).is_none() || host(url) != host(base) { return Value::Null }
    let dir = root(app).join("cosmetics").join("previews");
    let file = dir.join(format!("{id}.png"));
    let bytes = match tokio::fs::read(&file).await {
        Ok(b) => b,
        Err(_) => {
            let Ok(r) = c.get(url).send().await else { return Value::Null };
            let Ok(b) = r.bytes().await else { return Value::Null };
            if b.len() > 150_000 || !b.starts_with(&[0x89, b'P', b'N', b'G']) { return Value::Null }
            let _ = tokio::fs::create_dir_all(&dir).await;
            let _ = tokio::fs::write(&file, &b).await;
            b.to_vec()
        }
    };
    json!(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
pub async fn cosmetics_catalog(app: AppHandle) -> Result<Value, String> {
    let base = backend()?;
    let c = client()?;
    let cat = send(&c, c.get(format!("{base}/catalog")), None).await?;
    let mut items: Vec<Value> = vec![];
    for it in cat["items"].as_array().into_iter().flatten().take(60) {
        let id = it["id"].as_str().unwrap_or("");
        let pv = preview(&app, &c, &base, id, it["preview_url"].as_str()).await;
        items.push(json!({"id": id, "kind": it["kind"], "name": it["name"], "description": it["description"], "price_cents": it["price_cents"], "preview": pv}));
    }
    let (mut owned, mut equipped, mut signed_in) = (json!([]), json!({}), false);
    if let Ok(tok) = token(&app, &c).await {
        if let Ok(me) = send(&c, c.get(format!("{base}/me")), Some(&tok)).await {
            owned = me["owned"].clone();
            equipped = me["equipped"].clone();
            signed_in = true;
        }
    }
    Ok(json!({"items": items, "owned": owned, "equipped": equipped, "signed_in": signed_in}))
}

#[tauri::command]
pub async fn cosmetics_equip(app: AppHandle, kind: String, id: Option<String>) -> Result<(), String> {
    if !matches!(kind.as_str(), "cape" | "hat" | "wings") { return Err("Type de contenu invalide".into()) }
    if let Some(i) = &id { if i.is_empty() || i.len() > 64 { return Err("Réponse du serveur invalide".into()) } }
    let c = client()?;
    let tok = token(&app, &c).await?;
    let base = backend()?;
    send(&c, c.post(format!("{base}/equip")).json(&json!({"kind": kind, "id": id})), Some(&tok)).await?;
    Ok(())
}
