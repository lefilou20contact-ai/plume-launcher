//! Loaders de mods : Fabric, Quilt (profil JSON) et Forge, NeoForge (installateur officiel).
use crate::{config, e, ensure_java, fetch, get_json, load_version_json, prog};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path, process::Stdio, time::Duration};
use tauri::AppHandle;

pub fn supported(l: &str) -> bool { matches!(l, "vanilla" | "fabric" | "quilt" | "forge" | "neoforge") }

pub fn label(l: &str) -> &'static str {
    match l { "fabric" => "Fabric", "quilt" => "Quilt", "forge" => "Forge", "neoforge" => "NeoForge", _ => "Vanilla" }
}

const FABRIC: &[&str] = &["fabric"];
const QUILT: &[&str] = &["quilt", "fabric"];
const FORGE: &[&str] = &["forge"];
const NEOFORGE: &[&str] = &["neoforge"];

// Loaders Modrinth compatibles avec un type d'instance (Quilt sait aussi charger les mods Fabric).
pub fn mod_loaders(l: &str) -> Option<&'static [&'static str]> {
    match l { "fabric" => Some(FABRIC), "quilt" => Some(QUILT), "forge" => Some(FORGE), "neoforge" => Some(NEOFORGE), _ => None }
}

pub fn lib_key(name: &str) -> String { name.split(':').take(2).collect::<Vec<_>>().join(":") }

pub fn maven_path(name: &str) -> Option<String> {
    let p: Vec<&str> = name.split(':').collect();
    if p.len() < 3 || p.iter().any(|s| s.is_empty() || s.contains('/') || s.contains("..")) { return None }
    let cl = p.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Some(format!("{}/{}/{}/{}-{}{}.jar", p[0].replace('.', "/"), p[1], p[2], p[1], p[2], cl))
}

fn ver_key(s: &str) -> Vec<u64> { s.split(|c: char| c == '.' || c == '-' || c == '+').map(|p| p.parse::<u64>().unwrap_or(0)).collect() }

// NeoForge numérote ses versions d'après Minecraft : 1.21.1 -> 21.1.x ; il n'existe qu'à partir de 1.20.2.
fn neo_prefix(mc: &str) -> Option<String> {
    let p: Vec<&str> = mc.split('.').collect();
    if p.len() < 2 || p[0] != "1" { return None }
    let a: u64 = p[1].parse().ok()?;
    let b: u64 = p.get(2).and_then(|x| x.parse().ok()).unwrap_or(0);
    if a < 20 || (a == 20 && b < 2) { return None }
    Some(format!("{a}.{b}."))
}

/// Dernière version stable du loader pour une version de Minecraft.
pub async fn resolve(c: &reqwest::Client, loader: &str, mc: &str) -> Result<String, String> {
    let name = label(loader);
    let unsupported = || format!("{name} ne supporte pas Minecraft {mc}");
    let prepare = || format!("Impossible de préparer {name} pour Minecraft {mc} (version non supportée ou hors ligne)");
    match loader {
        "fabric" | "quilt" => {
            let base = if loader == "fabric" { config::FABRIC_META } else { config::QUILT_META };
            let v = get_json(c, &format!("{base}/versions/loader/{mc}")).await.map_err(|_| prepare())?;
            let arr = v.as_array().cloned().unwrap_or_default();
            let stable = |x: &&Value| if loader == "fabric" { x["loader"]["stable"] == true } else { !x["loader"]["version"].as_str().unwrap_or("-").contains('-') };
            arr.iter().find(stable).or(arr.first()).and_then(|x| x["loader"]["version"].as_str()).map(String::from).ok_or_else(unsupported)
        }
        "forge" => {
            let v = get_json(c, config::FORGE_PROMOS).await.map_err(|_| prepare())?;
            let p = &v["promos"];
            p[format!("{mc}-recommended")].as_str().or(p[format!("{mc}-latest")].as_str()).map(String::from).ok_or_else(unsupported)
        }
        "neoforge" => {
            let prefix = neo_prefix(mc).ok_or_else(unsupported)?;
            let t = c.get(config::NEOFORGE_META).send().await.map_err(|_| prepare())?
                .error_for_status().map_err(|_| prepare())?.text().await.map_err(|_| prepare())?;
            let all: Vec<String> = t.split("<version>").skip(1).filter_map(|s| s.split("</version>").next()).map(String::from).filter(|s| s.starts_with(&prefix)).collect();
            let stable = all.iter().filter(|s| !s.contains('-')).max_by_key(|s| ver_key(s)).cloned();
            stable.or_else(|| all.iter().max_by_key(|s| ver_key(s)).cloned()).ok_or_else(unsupported)
        }
        _ => Err("Type d'instance inconnu".into()),
    }
}

// Version déjà installée par l'installateur (dossier versions/<id>).
fn find_installed(shared: &Path, loader: &str, lv: &str) -> Option<String> {
    let rd = std::fs::read_dir(shared.join("versions")).ok()?;
    rd.flatten().filter_map(|f| f.file_name().into_string().ok()).find(|n| {
        n.contains(lv) && shared.join("versions").join(n).join(format!("{n}.json")).exists()
            && ((loader == "forge" && n.contains("forge") && !n.contains("neoforge")) || (loader == "neoforge" && n.contains("neoforge")))
    })
}

/// Lance l'installateur officiel de Forge ou NeoForge (une seule fois) et renvoie l'identifiant de la version installée.
pub async fn install_forge_like(app: &AppHandle, c: &reqwest::Client, shared: &Path, loader: &str, mc: &str, lv: &str) -> Result<String, String> {
    let ok = |s: &str| !s.is_empty() && s.len() <= 40 && s.chars().all(|ch| ch.is_ascii_alphanumeric() || ".-_+".contains(ch));
    if !ok(mc) || !ok(lv) { return Err("Version du loader invalide".into()) }
    if let Some(id) = find_installed(shared, loader, lv) { return Ok(id) }
    let name = label(loader);
    prog(app, &format!("Installation de {name}"), 0, 0);
    let (url, fname) = if loader == "forge" {
        let full = format!("{mc}-{lv}");
        (format!("{}/{full}/forge-{full}-installer.jar", config::FORGE_MAVEN), format!("forge-{full}-installer.jar"))
    } else {
        (format!("{}/{lv}/neoforge-{lv}-installer.jar", config::NEOFORGE_MAVEN), format!("neoforge-{lv}-installer.jar"))
    };
    let dir = shared.join("loaders").join("installers");
    tokio::fs::create_dir_all(&dir).await.map_err(e)?;
    let jar = dir.join(&fname);
    fetch(c, &url, &jar).await?;
    let vjson = load_version_json(c, shared, mc).await?;
    let java = ensure_java(app, c, shared, &vjson).await.unwrap_or_else(|_| "java".to_string());
    // L'installateur exige un fichier launcher_profiles.json dans le dossier cible.
    let lp = shared.join("launcher_profiles.json");
    if !lp.exists() { tokio::fs::write(&lp, r#"{"profiles":{},"settings":{},"version":3}"#).await.map_err(e)?; }
    let log = shared.join("loaders").join(format!("installer-{loader}-{lv}.log"));
    let f1 = std::fs::File::create(&log).map_err(e)?;
    let f2 = f1.try_clone().map_err(e)?;
    let mut cmd = tokio::process::Command::new(&java);
    cmd.arg("-jar").arg(&jar).arg("--installClient").arg(shared).current_dir(shared).stdout(Stdio::from(f1)).stderr(Stdio::from(f2));
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let status = tokio::time::timeout(Duration::from_secs(900), async { cmd.spawn().map_err(e)?.wait().await.map_err(e) })
        .await.map_err(|_| "Installateur trop long".to_string())??;
    if !status.success() { return Err(format!("L'installateur de {name} a échoué (voir {})", log.display())) }
    let _ = tokio::fs::remove_file(&jar).await;
    find_installed(shared, loader, lv).ok_or_else(|| format!("Version de {name} introuvable après installation"))
}

/// Profil JSON du loader de l'instance (téléchargé et mis en cache, ou installé pour Forge/NeoForge).
pub async fn profile(app: &AppHandle, c: &reqwest::Client, shared: &Path, inst: &Value) -> Result<Value, String> {
    let loader = inst["loader"].as_str().unwrap_or("vanilla");
    let mc = inst["version"].as_str().unwrap_or("");
    let lv = inst["loader_version"].as_str().ok_or("Version du loader manquante")?;
    if !lv.chars().all(|ch| ch.is_ascii_alphanumeric() || ".-_+".contains(ch)) { return Err("Version du loader invalide".into()) }
    match loader {
        "fabric" | "quilt" => {
            let base = if loader == "fabric" { config::FABRIC_META } else { config::QUILT_META };
            let p = shared.join("loaders").join(format!("{loader}-{mc}-{lv}.json"));
            if p.exists() { return serde_json::from_str(&tokio::fs::read_to_string(&p).await.map_err(e)?).map_err(e) }
            let t = c.get(format!("{base}/versions/loader/{mc}/{lv}/profile/json")).send().await
                .map_err(|_| "Connexion requise pour télécharger le loader".to_string())?
                .error_for_status().map_err(e)?.text().await.map_err(e)?;
            tokio::fs::create_dir_all(p.parent().unwrap()).await.map_err(e)?;
            tokio::fs::write(&p, &t).await.map_err(e)?;
            serde_json::from_str(&t).map_err(e)
        }
        "forge" | "neoforge" => {
            let id = install_forge_like(app, c, shared, loader, mc, lv).await?;
            serde_json::from_str(&tokio::fs::read_to_string(shared.join("versions").join(&id).join(format!("{id}.json"))).await.map_err(e)?).map_err(e)
        }
        _ => Err("Type d'instance inconnu".into()),
    }
}

/// Fusionne le profil du loader avec celui de Minecraft : bibliothèques du loader d'abord, classe principale remplacée, arguments ajoutés.
pub fn merge(v: &mut Value, prof: &Value) {
    let mut libs: Vec<Value> = vec![];
    let mut keys: HashSet<String> = HashSet::new();
    for l in prof["libraries"].as_array().into_iter().flatten() {
        let Some(name) = l["name"].as_str() else { continue };
        keys.insert(lib_key(name));
        if l["downloads"]["artifact"].is_object() { libs.push(l.clone()); continue }
        let Some(path) = maven_path(name) else { continue };
        let url = l["url"].as_str().filter(|b| b.starts_with("https://"))
            .map(|b| format!("{}{}{}", b, if b.ends_with('/') { "" } else { "/" }, path)).unwrap_or_default();
        libs.push(json!({"name": name, "rules": l.get("rules").cloned().unwrap_or(Value::Null), "downloads": {"artifact": {"path": path, "url": url}}}));
    }
    let vanilla: Vec<Value> = v["libraries"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|l| !keys.contains(&lib_key(l["name"].as_str().unwrap_or("")))).collect();
    libs.extend(vanilla);
    v["libraries"] = json!(libs);
    if let Some(m) = prof["mainClass"].as_str() { v["mainClass"] = json!(m); }
    for k in ["jvm", "game"] {
        if let Some(extra) = prof["arguments"][k].as_array() {
            let mut a = v["arguments"][k].as_array().cloned().unwrap_or_default();
            a.extend(extra.iter().cloned());
            v["arguments"][k] = json!(a);
        }
    }
    // Anciens Forge (1.12 et avant) : arguments sous forme de texte.
    if let Some(m) = prof["minecraftArguments"].as_str() { v["minecraftArguments"] = json!(m); }
}
