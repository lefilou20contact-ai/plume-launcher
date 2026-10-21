//! Connexion Microsoft -> Xbox Live -> XSTS -> Minecraft (flux "code d'appareil").
use crate::config;
use serde_json::{json, Value};
use std::{path::Path, time::{Duration, SystemTime, UNIX_EPOCH}};

const TENANT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const SCOPE: &str = "XboxLive.signin offline_access";

pub struct Session { pub name: String, pub uuid: String, pub token: String }

fn e<E: ToString>(x: E) -> String { x.to_string() }
fn net<E: ToString>(x: E) -> String { format!("NET:{}", x.to_string()) }
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

pub fn load(dir: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("account.json")).ok()?).ok()
}

#[cfg(unix)]
fn private(p: &Path) { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)); }
#[cfg(not(unix))]
fn private(_: &Path) {}

fn save(dir: &Path, v: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(e)?;
    let p = dir.join("account.json");
    std::fs::write(&p, v.to_string()).map_err(e)?;
    private(&p);
    Ok(())
}

pub fn logout(dir: &Path) { let _ = std::fs::remove_file(dir.join("account.json")); }

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

async fn profile(c: &reqwest::Client, tok: &str) -> Result<(String, String), String> {
    let r = c.get("https://api.minecraftservices.com/minecraft/profile").bearer_auth(tok).send().await.map_err(net)?;
    if r.status() == 404 { return Err("Ce compte Microsoft ne possède pas Minecraft Java Edition.".into()) }
    let v: Value = r.error_for_status().map_err(e)?.json().await.map_err(e)?;
    Ok((v["name"].as_str().ok_or("profil invalide")?.to_string(), v["id"].as_str().ok_or("profil invalide")?.to_string()))
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
    let (name, uuid) = profile(c, &mc).await?;
    let acc = json!({"name": name, "uuid": uuid, "refresh_token": rt, "mc_token": mc, "mc_exp": exp, "verified": now()});
    save(dir, &acc)?;
    Ok(json!({"name": acc["name"], "uuid": acc["uuid"]}))
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

/// Session pour lancer le jeu : jeton valide, renouvelé en ligne, ou profil en cache si hors ligne.
pub async fn session(c: &reqwest::Client, dir: &Path) -> Result<Session, String> {
    let mut acc = load(dir).ok_or("Connecte-toi avec Microsoft pour jouer.")?;
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
            acc["refresh_token"] = json!(new_rt);
            acc["mc_token"] = json!(mc);
            acc["mc_exp"] = json!(exp);
            acc["verified"] = json!(now());
            save(dir, &acc)?;
            Ok(Session { name, uuid, token: acc["mc_token"].as_str().unwrap_or("0").to_string() })
        }
        // Pas de connexion : on joue avec le profil vérifié lors de la dernière connexion.
        // Au-delà de 30 jours sans vérification en ligne, une reconnexion est exigée.
        Err(m) if m.starts_with("NET:") => {
            if now().saturating_sub(acc["verified"].as_u64().unwrap_or(0)) > 30 * 86400 {
                Err("Connexion internet requise : ton compte n'a pas été vérifié depuis plus de 30 jours.".into())
            } else {
                Ok(Session { name, uuid, token: "0".into() })
            }
        }
        Err(m) => Err(format!("Session expirée, reconnecte-toi ({})", m.trim_start_matches("AUTH:"))),
    }
}
