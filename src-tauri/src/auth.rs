//! Comptes Microsoft -> Xbox Live -> XSTS -> Minecraft (flux "code d'appareil"), avec plusieurs comptes.
use crate::config;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex, time::{Duration, SystemTime, UNIX_EPOCH}};

const TENANT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const SCOPE: &str = "XboxLive.signin offline_access";
static LOCK: Mutex<()> = Mutex::new(());

pub struct Session { pub name: String, pub uuid: String, pub token: String }

fn e<E: ToString>(x: E) -> String { x.to_string() }
fn net<E: ToString>(x: E) -> String { format!("NET:{}", x.to_string()) }
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }
fn valid_uuid(u: &str) -> bool { u.len() == 32 && u.chars().all(|c| c.is_ascii_hexdigit()) }

#[cfg(unix)]
fn private(p: &Path) { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)); }
#[cfg(not(unix))]
fn private(_: &Path) {}

// ---------- Stockage : accounts.json = {"active": uuid, "accounts": [...]} ----------
fn store_path(dir: &Path) -> std::path::PathBuf { dir.join("accounts.json") }

fn write_store(dir: &Path, s: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(e)?;
    let tmp = dir.join("accounts.json.tmp");
    std::fs::write(&tmp, s.to_string()).map_err(e)?;
    private(&tmp);
    std::fs::rename(&tmp, store_path(dir)).map_err(e)
}

fn read_store(dir: &Path) -> Value {
    if let Some(v) = std::fs::read_to_string(store_path(dir)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) { return v }
    // Migration depuis l'ancien fichier à compte unique.
    if let Some(a) = std::fs::read_to_string(dir.join("account.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        let s = json!({"active": a["uuid"], "accounts": [a]});
        if write_store(dir, &s).is_ok() { let _ = std::fs::remove_file(dir.join("account.json")); }
        return s;
    }
    json!({"active": Value::Null, "accounts": []})
}

fn modify<R>(dir: &Path, f: impl FnOnce(&mut Value) -> R) -> Result<R, String> {
    let _g = LOCK.lock().map_err(e)?;
    let mut s = read_store(dir);
    if !s["accounts"].is_array() { s["accounts"] = json!([]); }
    let r = f(&mut s);
    write_store(dir, &s)?;
    Ok(r)
}

fn upsert(dir: &Path, acc: Value, make_active: bool) -> Result<(), String> {
    modify(dir, |s| {
        let arr = s["accounts"].as_array_mut().unwrap();
        if let Some(p) = arr.iter().position(|x| x["uuid"] == acc["uuid"]) { arr[p] = acc.clone(); } else { arr.push(acc.clone()); }
        if make_active { s["active"] = acc["uuid"].clone(); }
    })
}

pub fn active(dir: &Path) -> Option<Value> {
    let s = read_store(dir);
    let id = s["active"].as_str()?.to_string();
    s["accounts"].as_array()?.iter().find(|a| a["uuid"] == id.as_str()).cloned()
}

fn skin_data(dir: &Path, uuid: &str) -> Option<String> {
    if !valid_uuid(uuid) { return None }
    let b = std::fs::read(dir.join("avatars").join(format!("{uuid}.png"))).ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(b)))
}

pub fn list(dir: &Path) -> Vec<Value> {
    let s = read_store(dir);
    let act = s["active"].as_str().unwrap_or("").to_string();
    s["accounts"].as_array().into_iter().flatten().map(|a| {
        let u = a["uuid"].as_str().unwrap_or("");
        json!({"name": a["name"], "uuid": u, "active": u == act, "skin": skin_data(dir, u)})
    }).collect()
}

pub fn switch(dir: &Path, uuid: &str) -> Result<(), String> {
    let ok = modify(dir, |s| {
        let has = s["accounts"].as_array().map_or(false, |a| a.iter().any(|x| x["uuid"] == uuid));
        if has { s["active"] = json!(uuid); }
        has
    })?;
    if ok { Ok(()) } else { Err("Compte introuvable".into()) }
}

pub fn remove(dir: &Path, uuid: &str) -> Result<(), String> {
    modify(dir, |s| {
        if let Some(a) = s["accounts"].as_array_mut() { a.retain(|x| x["uuid"] != uuid); }
        if s["active"] == uuid { s["active"] = s["accounts"][0]["uuid"].clone(); }
    })?;
    if valid_uuid(uuid) { let _ = std::fs::remove_file(dir.join("avatars").join(format!("{uuid}.png"))); }
    Ok(())
}

pub fn logout(dir: &Path) {
    if let Some(a) = active(dir) { let _ = remove(dir, a["uuid"].as_str().unwrap_or("")); }
}

// ---------- Connexion ----------
fn explain(v: &Value, status: u16) -> String {
    match v["XErr"].as_u64() {
        Some(2148916233) => return "Ce compte Microsoft n'a pas de profil Xbox : crée-en un sur xbox.com puis réessaie.".into(),
        Some(2148916238) => return "Compte enfant : il doit être ajouté à une famille Microsoft pour jouer.".into(),
        Some(2148916235) => return "Xbox Live n'est pas disponible dans ton pays.".into(),
        _ => {}
    }
    if v.to_string().contains("Invalid app registration") {
        return "Application non approuvée par Mojang (formulaire https://aka.ms/mce-reviewappid).".into();
    }
    format!("Échec de la connexion (code {status})")
}

async fn post_json(c: &reqwest::Client, url: &str, body: Value) -> Result<Value, String> {
    let r = c.post(url).header("Accept", "application/json").json(&body).send().await.map_err(net)?;
    let st = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !st.is_success() { return Err(explain(&v, st.as_u16())) }
    Ok(v)
}

async fn minecraft_token(c: &reqwest::Client, ms: &str) -> Result<(String, u64), String> {
    let xbl = post_json(c, "https://user.auth.xboxlive.com/user/authenticate", json!({
        "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={ms}")},
        "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"})).await?;
    let xt = xbl["Token"].as_str().ok_or("jeton Xbox manquant")?.to_string();
    let xsts = post_json(c, "https://xsts.auth.xboxlive.com/xsts/authorize", json!({
        "Properties": {"SandboxId": "RETAIL", "UserTokens": [xt]},
        "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"})).await?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"].as_str().ok_or("jeton XSTS invalide")?;
    let st = xsts["Token"].as_str().ok_or("jeton XSTS manquant")?;
    let mc = post_json(c, "https://api.minecraftservices.com/authentication/login_with_xbox",
        json!({"identityToken": format!("XBL3.0 x={uhs};{st}")})).await?;
    let tok = mc["access_token"].as_str().ok_or("jeton Minecraft manquant")?.to_string();
    Ok((tok, now() + mc["expires_in"].as_u64().unwrap_or(86400)))
}

// Pseudo, identifiant et adresse de la peau active.
async fn profile(c: &reqwest::Client, tok: &str) -> Result<(String, String, Option<String>), String> {
    let r = c.get("https://api.minecraftservices.com/minecraft/profile").bearer_auth(tok).send().await.map_err(net)?;
    if r.status() == 404 { return Err("Ce compte Microsoft ne possède pas Minecraft Java Edition.".into()) }
    let v: Value = r.error_for_status().map_err(e)?.json().await.map_err(e)?;
    let skin = v["skins"].as_array().into_iter().flatten().find(|s| s["state"] == "ACTIVE")
        .and_then(|s| s["url"].as_str()).map(String::from);
    Ok((v["name"].as_str().ok_or("profil invalide")?.to_string(), v["id"].as_str().ok_or("profil invalide")?.to_string(), skin))
}

// Enregistre la peau pour afficher la tête du joueur, même hors ligne (échec sans conséquence).
async fn save_skin(c: &reqwest::Client, dir: &Path, uuid: &str, url: &str) {
    let url = url.replacen("http://", "https://", 1);
    if !valid_uuid(uuid) || !url.starts_with("https://textures.minecraft.net/texture/") { return }
    let Ok(r) = c.get(&url).send().await else { return };
    let Ok(b) = r.bytes().await else { return };
    if b.len() > 200_000 || !b.starts_with(&[0x89, b'P', b'N', b'G']) { return }
    let d = dir.join("avatars");
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::write(d.join(format!("{uuid}.png")), &b);
}

pub async fn start(c: &reqwest::Client) -> Result<Value, String> {
    let id = config::require("PLUME_MS_CLIENT_ID", config::MS_CLIENT_ID)?;
    let r = c.post(format!("{TENANT}/devicecode")).form(&[("client_id", id), ("scope", SCOPE)]).send().await.map_err(net)?;
    let v: Value = r.json().await.map_err(e)?;
    if v["device_code"].is_null() {
        return Err(v["error_description"].as_str().unwrap_or("Connexion Microsoft impossible").to_string());
    }
    Ok(v)
}

pub async fn finish(c: &reqwest::Client, dir: &Path, device_code: &str, interval: u64, expires_in: u64) -> Result<Value, String> {
    let id = config::require("PLUME_MS_CLIENT_ID", config::MS_CLIENT_ID)?;
    let begin = now();
    let mut wait = interval.max(1);
    let (ms, rt) = loop {
        tokio::time::sleep(Duration::from_secs(wait)).await;
        if now() > begin + expires_in { return Err("Code expiré, recommence la connexion.".into()) }
        let v: Value = c.post(format!("{TENANT}/token"))
            .form(&[("client_id", id), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("device_code", device_code)])
            .send().await.map_err(net)?.json().await.map_err(e)?;
        if let (Some(a), Some(r)) = (v["access_token"].as_str(), v["refresh_token"].as_str()) { break (a.to_string(), r.to_string()) }
        match v["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => wait += 5,
            _ => return Err(v["error_description"].as_str().unwrap_or("Connexion refusée").to_string()),
        }
    };
    let (mc, exp) = minecraft_token(c, &ms).await?;
    let (name, uuid, skin) = profile(c, &mc).await?;
    if let Some(u) = &skin { save_skin(c, dir, &uuid, u).await; }
    let acc = json!({"name": name, "uuid": uuid, "refresh_token": rt, "mc_token": mc, "mc_exp": exp, "verified": now()});
    upsert(dir, acc, true)?;
    Ok(json!({"name": name, "uuid": uuid}))
}

async fn refresh(c: &reqwest::Client, rt: &str) -> Result<String, String> {
    let id = config::require("PLUME_MS_CLIENT_ID", config::MS_CLIENT_ID)?;
    let v: Value = c.post(format!("{TENANT}/token"))
        .form(&[("client_id", id), ("grant_type", "refresh_token"), ("refresh_token", rt), ("scope", SCOPE)])
        .send().await.map_err(net)?.json().await.map_err(net)?;
    match (v["access_token"].as_str(), v["refresh_token"].as_str()) {
        (Some(a), Some(n)) => Ok(format!("{a}\n{n}")),
        _ => Err(format!("AUTH:{}", v["error_description"].as_str().unwrap_or("refusé"))),
    }
}

/// Session du compte actif : jeton valide, renouvelé en ligne, ou profil en cache hors ligne (30 jours max).
pub async fn session(c: &reqwest::Client, dir: &Path) -> Result<Session, String> {
    let acc = active(dir).ok_or("Connecte-toi avec Microsoft pour jouer.")?;
    let name = acc["name"].as_str().unwrap_or("Joueur").to_string();
    let uuid = acc["uuid"].as_str().unwrap_or("").to_string();
    if acc["mc_exp"].as_u64().unwrap_or(0) > now() + 120 {
        return Ok(Session { name, uuid, token: acc["mc_token"].as_str().unwrap_or("0").to_string() });
    }
    let rt = acc["refresh_token"].as_str().unwrap_or("").to_string();
    let online = async {
        let both = refresh(c, &rt).await?;
        let (ms, new_rt) = both.split_once('\n').ok_or("réponse invalide")?;
        let (mc, exp) = minecraft_token(c, ms).await?;
        Ok::<_, String>((new_rt.to_string(), mc, exp))
    }.await;
    match online {
        Ok((new_rt, mc, exp)) => {
            let mut acc2 = acc.clone();
            acc2["refresh_token"] = json!(new_rt);
            acc2["mc_token"] = json!(mc);
            acc2["mc_exp"] = json!(exp);
            acc2["verified"] = json!(now());
            // Met à jour pseudo et peau si le joueur les a changés (sans bloquer le lancement en cas d'échec).
            if let Ok((n, u, skin)) = profile(c, &mc).await {
                if u == uuid {
                    acc2["name"] = json!(n);
                    if let Some(s) = skin { save_skin(c, dir, &uuid, &s).await; }
                }
            }
            upsert(dir, acc2.clone(), false)?;
            Ok(Session { name: acc2["name"].as_str().unwrap_or("Joueur").to_string(), uuid, token: mc })
        }
        Err(m) if m.starts_with("NET:") && now().saturating_sub(acc["verified"].as_u64().unwrap_or(0)) < 30 * 86400 => {
            Ok(Session { name, uuid, token: "0".into() })
        }
        Err(m) if m.starts_with("NET:") => Err("Connexion internet requise : le compte n'a pas été vérifié depuis plus de 30 jours.".into()),
        Err(m) => Err(format!("Session expirée, reconnecte-toi ({})", m.trim_start_matches("AUTH:"))),
    }
}
