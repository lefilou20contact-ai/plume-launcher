//! Plume Lab : teste un mod (.jar) sans ouvrir Minecraft.
//! 1) analyse du fichier (loader, dépendances, versions, Java, mixins) ; 2) démarrage INVISIBLE d'un serveur de test
//! (aucune fenêtre) pour mesurer le démarrage, la mémoire, le processeur et repérer les erreurs.
use crate::{client, config, e, ensure_java, fetch, get_json, load_version_json, loaders, modrinth, root};
use serde_json::{json, Value};
use std::{cmp::Ordering, io::Read, path::{Path, PathBuf}, process::Stdio, sync::atomic::{AtomicBool, Ordering as AO}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

static BUSY: AtomicBool = AtomicBool::new(false);

// ---------- Mesures du processus (mémoire en Ko, temps processeur en secondes) ----------
#[cfg(target_os = "linux")]
fn proc_stats(pid: u32) -> Option<(u64, f64)> {
    let st = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let rss: u64 = st.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()?;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let f: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks = f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?;
    Some((rss, ticks / 100.0))
}
#[cfg(target_os = "macos")]
fn proc_stats(pid: u32) -> Option<(u64, f64)> {
    let o = std::process::Command::new("ps").args(["-o", "rss=,time=", "-p", &pid.to_string()]).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    let mut it = s.split_whitespace();
    let rss: u64 = it.next()?.parse().ok()?;
    let secs = it.next()?.split(':').fold(0.0, |acc, p| acc * 60.0 + p.parse::<f64>().unwrap_or(0.0));
    Some((rss, secs))
}
#[cfg(windows)]
fn proc_stats(pid: u32) -> Option<(u64, f64)> {
    use std::os::windows::process::CommandExt;
    let script = format!("$p=Get-Process -Id {pid}; [string]::Format([Globalization.CultureInfo]::InvariantCulture,'{{0}} {{1}}',[int]($p.WorkingSet64/1024),$p.CPU)");
    let o = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).creation_flags(0x0800_0000).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    let mut it = s.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn proc_stats(_pid: u32) -> Option<(u64, f64)> { None }

fn read_tail(p: &Path, max: u64) -> String {
    use std::io::{Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(p) else { return String::new() };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(size.saturating_sub(max)));
    let mut b = Vec::new();
    let _ = f.take(max).read_to_end(&mut b);
    String::from_utf8_lossy(&b).to_string()
}

// ---------- Versions : Fabric (>=1.21 <1.22, ~1.21.4, 1.21.x, ||) et Maven ([1.20,1.21)) ----------
fn parse_ver(s: &str) -> Option<Vec<u64>> {
    let core = s.trim().split(|c| c == '-' || c == '+').next()?;
    core.split('.').map(|p| p.parse::<u64>().ok()).collect()
}
fn cmp_ver(a: &[u64], b: &[u64]) -> Ordering {
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (*a.get(i).unwrap_or(&0), *b.get(i).unwrap_or(&0));
        if x != y { return x.cmp(&y) }
    }
    Ordering::Equal
}
fn tok_ok(tok: &str, v: &[u64]) -> Option<bool> {
    if tok.is_empty() || tok == "*" || tok.eq_ignore_ascii_case("x") { return Some(true) }
    let (op, rest) = ["<=", ">=", "<", ">", "=", "~", "^"].iter().find_map(|o| tok.strip_prefix(o).map(|r| (*o, r))).unwrap_or(("=", tok));
    if rest.ends_with(".x") || rest.ends_with(".*") {
        let base = parse_ver(&rest[..rest.len() - 2])?;
        return Some(v.len() >= base.len() && v[..base.len()] == base[..]);
    }
    let t = parse_ver(rest)?;
    let c = cmp_ver(v, &t);
    Some(match op {
        "<=" => c != Ordering::Greater,
        ">=" => c != Ordering::Less,
        "<" => c == Ordering::Less,
        ">" => c == Ordering::Greater,
        "~" => { let n = t.len().min(2); c != Ordering::Less && v.iter().take(n).eq(t.iter().take(n)) }
        "^" => c != Ordering::Less && v.first() == t.first(),
        _ => c == Ordering::Equal,
    })
}
fn fabric_match(range: &str, v: &[u64]) -> Option<bool> {
    let mut res = false;
    for alt in range.split("||") {
        let mut all = true;
        for tok in alt.split_whitespace() { all &= tok_ok(tok, v)?; }
        res |= all;
    }
    Some(res)
}
fn interval(s: &str, v: &[u64]) -> Option<bool> {
    let s = s.trim().trim_start_matches(',').trim();
    if s.len() < 2 { return None }
    let (lo_inc, hi_inc) = (s.starts_with('['), s.ends_with(']'));
    let inner = &s[1..s.len() - 1];
    let Some((a, b)) = inner.split_once(',') else { return Some(cmp_ver(v, &parse_ver(inner)?) == Ordering::Equal) };
    let ok_lo = if a.trim().is_empty() { true } else { let c = cmp_ver(v, &parse_ver(a)?); if lo_inc { c != Ordering::Less } else { c == Ordering::Greater } };
    let ok_hi = if b.trim().is_empty() { true } else { let c = cmp_ver(v, &parse_ver(b)?); if hi_inc { c != Ordering::Greater } else { c == Ordering::Less } };
    Some(ok_lo && ok_hi)
}
fn maven_match(r: &str, v: &[u64]) -> Option<bool> {
    let r = r.trim();
    if !r.starts_with('[') && !r.starts_with('(') { return Some(cmp_ver(v, &parse_ver(r)?) != Ordering::Less) }
    let (mut res, mut cur) = (false, String::new());
    for ch in r.chars() {
        cur.push(ch);
        if ch == ']' || ch == ')' { res |= interval(&cur, v)?; cur.clear(); }
    }
    Some(res)
}

// ---------- Lecture du descripteur du mod ----------
struct Meta { loader: &'static str, id: String, name: String, version: String, env: String, mc_range: Option<String>, deps: Vec<(String, String, bool)>, mixins: Vec<String> }

fn strs(v: &Value) -> Vec<String> {
    match v { Value::String(x) => vec![x.clone()], Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(), _ => vec![] }
}

fn parse_fabric(s: &str) -> Option<Meta> {
    let j: Value = serde_json::from_str(s).ok()?;
    let (mut deps, mut mc) = (vec![], None);
    for (k, v) in j["depends"].as_object().into_iter().flatten() {
        let r = strs(v).join(" || ");
        if k == "minecraft" { mc = Some(r.clone()); }
        deps.push((k.clone(), r, true));
    }
    for (k, v) in j["recommends"].as_object().into_iter().flatten() { deps.push((k.clone(), strs(v).join(" || "), false)); }
    let mixins = j["mixins"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(String::from).or_else(|| m["config"].as_str().map(String::from))).collect();
    let id = j["id"].as_str()?.to_string();
    Some(Meta { loader: "fabric", name: j["name"].as_str().unwrap_or(&id).to_string(), id, version: j["version"].as_str().unwrap_or("?").to_string(), env: j["environment"].as_str().unwrap_or("*").to_string(), mc_range: mc, deps, mixins })
}

fn parse_quilt(s: &str) -> Option<Meta> {
    let j: Value = serde_json::from_str(s).ok()?;
    let q = &j["quilt_loader"];
    let (mut deps, mut mc) = (vec![], None);
    for d in q["depends"].as_array().into_iter().flatten() {
        let (id, range, required) = if let Some(x) = d.as_str() { (x.to_string(), "*".to_string(), true) }
            else { (d["id"].as_str()?.to_string(), { let r = strs(&d["versions"]).join(" || "); if r.is_empty() { "*".to_string() } else { r } }, d["optional"] != true) };
        if id == "minecraft" { mc = Some(range.clone()); }
        deps.push((id, range, required));
    }
    let id = q["id"].as_str()?.to_string();
    Some(Meta { loader: "quilt", name: q["metadata"]["name"].as_str().unwrap_or(&id).to_string(), id, version: q["version"].as_str().unwrap_or("?").to_string(),
        env: j["minecraft"]["environment"].as_str().unwrap_or("*").to_string(), mc_range: mc, deps, mixins: strs(&j["mixin"]) })
}

fn toml_blocks(s: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = vec![(String::new(), vec![])];
    for l in s.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') { continue }
        if l.starts_with('[') { out.push((l.trim_matches(|c| c == '[' || c == ']').trim().to_string(), vec![])); continue }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.split('#').next().unwrap_or("").trim().trim_matches('"').trim_matches('\'').to_string();
            if let Some(last) = out.last_mut() { last.1.push((k.trim().to_string(), v)); }
        }
    }
    out
}
fn kv(b: &[(String, String)], k: &str) -> Option<String> { b.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()) }

fn parse_toml(s: &str, loader: &'static str) -> Option<Meta> {
    let blocks = toml_blocks(s);
    let m = blocks.iter().find(|(h, _)| h == "mods")?;
    let id = kv(&m.1, "modId")?;
    let (mut deps, mut mc) = (vec![], None);
    for (h, b) in &blocks {
        if !h.starts_with("dependencies.") { continue }
        let Some(did) = kv(b, "modId") else { continue };
        let range = kv(b, "versionRange").unwrap_or_else(|| "*".to_string());
        let required = kv(b, "mandatory").map_or(false, |x| x == "true") || kv(b, "type").map_or(false, |x| x == "required");
        if did == "minecraft" { mc = Some(range.clone()); }
        deps.push((did, range, required));
    }
    Some(Meta { loader, name: kv(&m.1, "displayName").unwrap_or_else(|| id.clone()), version: kv(&m.1, "version").unwrap_or_else(|| "?".into()), id, env: "*".into(), mc_range: mc, deps, mixins: vec![] })
}

fn java_major(z: &mut zip::ZipArchive<std::fs::File>) -> Option<u16> {
    let (mut max, mut n) = (0u16, 0);
    for i in 0..z.len() {
        if n >= 40 { break }
        let Ok(mut f) = z.by_index(i) else { continue };
        let name = f.name().to_string();
        if !name.ends_with(".class") || name.starts_with("META-INF/") { continue }
        let mut b = [0u8; 8];
        if f.read_exact(&mut b).is_ok() && b[..4] == [0xCA, 0xFE, 0xBA, 0xBE] { max = max.max(u16::from_be_bytes([b[6], b[7]])); n += 1; }
    }
    if max > 0 { Some(max) } else { None }
}

fn read_entry(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut f = z.by_name(name).ok()?;
    if f.size() > 2_000_000 { return None }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

const BUILTIN: [&str; 12] = ["minecraft", "java", "fabricloader", "fabric-loader", "quilt_loader", "forge", "neoforge", "fml", "javafml", "lowcodefml", "mclanguage", "minecraft-version"];

// Analyse sans lancer le jeu : métadonnées, compatibilité de version, Java requis.
fn inspect(path: &Path, mc: &str) -> Result<(Meta, Value), String> {
    let size = std::fs::metadata(path).map_err(e)?.len();
    if size > 200_000_000 { return Err("Fichier trop volumineux".into()) }
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).map_err(e)?).map_err(|_| "Fichier .jar invalide".to_string())?;
    let meta = read_entry(&mut z, "fabric.mod.json").and_then(|s| parse_fabric(&s))
        .or_else(|| read_entry(&mut z, "quilt.mod.json").and_then(|s| parse_quilt(&s)))
        .or_else(|| read_entry(&mut z, "META-INF/neoforge.mods.toml").and_then(|s| parse_toml(&s, "neoforge")))
        .or_else(|| read_entry(&mut z, "META-INF/mods.toml").and_then(|s| parse_toml(&s, "forge")))
        .ok_or("Aucun descripteur de mod trouvé (fabric.mod.json, quilt.mod.json, mods.toml ou neoforge.mods.toml)")?;
    let major = java_major(&mut z);
    let classes = (0..z.len()).filter(|i| z.name_for_index(*i).map_or(false, |n| n.ends_with(".class"))).count();
    let nested = (0..z.len()).filter(|i| z.name_for_index(*i).map_or(false, |n| n.starts_with("META-INF/jars/") && n.ends_with(".jar"))).count();
    let target = parse_ver(mc);
    let mc_ok = match (&meta.mc_range, &target) {
        (Some(r), Some(v)) if r.trim() != "*" => if meta.loader == "fabric" || meta.loader == "quilt" { fabric_match(r, v) } else { maven_match(r, v) },
        _ => None,
    };
    let deps: Vec<Value> = meta.deps.iter().filter(|(id, _, _)| !BUILTIN.contains(&id.as_str()))
        .map(|(id, r, req)| json!({"id": id, "range": r, "required": req})).collect();
    let info = json!({
        "id": meta.id, "name": meta.name, "version": meta.version, "loader": meta.loader, "env": meta.env,
        "mc_range": meta.mc_range, "mc_ok": mc_ok, "java": major.map(|m| m.saturating_sub(44)), "size": size,
        "classes": classes, "nested": nested, "mixins": meta.mixins, "deps": deps
    });
    Ok((meta, info))
}

// ---------- Analyse des journaux ----------
fn cut(l: &str) -> String { l.chars().take(220).collect() }

fn analyze(d: &Path, began: u64) -> Value {
    let log = read_tail(&d.join("logs/latest.log"), 4_000_000) + "\n" + &read_tail(&d.join("plume-server.log"), 1_000_000);
    let (mut errors, mut warns, mut lag) = (0u32, 0u32, 0u32);
    let (mut mixin, mut deps, mut samples): (Vec<String>, Vec<String>, Vec<String>) = (vec![], vec![], vec![]);
    for l in log.lines() {
        let low = l.to_lowercase();
        if l.contains("/ERROR]") || l.contains("[ERROR]") || l.contains("/FATAL]") || l.contains("Exception in thread") {
            errors += 1;
            if samples.len() < 12 { samples.push(cut(l)); }
        } else if l.contains("/WARN]") || l.contains("[WARN]") { warns += 1; }
        if l.contains("Can't keep up") { lag += 1; }
        if (l.contains("Mixin apply failed") || l.contains("InvalidMixinException")) && mixin.len() < 8 { mixin.push(cut(l)); }
        let dep_line = (low.contains("requires") && (low.contains("mod ") || low.contains("mod '")))
            || l.contains("Missing or unsupported mandatory dependencies") || l.contains("Incompatible mod set") || l.contains("Incompatible mods found");
        if dep_line && deps.len() < 8 { deps.push(cut(l)); }
    }
    let crash = std::fs::read_dir(d.join("crash-reports")).ok().and_then(|rd| rd.flatten().filter_map(|f| {
        let m = f.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
        if m + 5 >= began { f.file_name().into_string().ok() } else { None }
    }).max());
    let pauses: Vec<f64> = read_tail(&d.join("plume-gc.log"), 8_000_000).lines().filter(|l| l.contains("Pause"))
        .filter_map(|l| l.trim_end().strip_suffix("ms")?.rsplit(' ').next()?.parse::<f64>().ok()).collect();
    let gc = if pauses.is_empty() { Value::Null } else {
        json!({"count": pauses.len(), "total_ms": pauses.iter().sum::<f64>().round(), "max_ms": pauses.iter().cloned().fold(0.0, f64::max).round()})
    };
    json!({"errors": errors, "warns": warns, "lag": lag, "mixin": mixin, "deps": deps, "samples": samples, "crash_report": crash, "gc": gc})
}

fn parse_done(t: &str) -> Option<f64> {
    let i = t.find("Done (")?;
    let rest = &t[i + 6..];
    rest[..rest.find("s)")?].replace(',', ".").parse().ok()
}

enum Launch { Jar(String), Args(String) }

fn phase(app: &AppHandle, p: &str, el: f64) { let _ = app.emit("lab-progress", json!({"phase": p, "elapsed": el})); }

// Prépare (une seule fois, puis en cache) un serveur de test pour ce loader et cette version de Minecraft.
async fn prepare_base(app: &AppHandle, c: &reqwest::Client, loader: &str, mc: &str, vjson: &Value) -> Result<(PathBuf, Launch), String> {
    let shared = root(app).join("shared");
    let lv = loaders::resolve(c, loader, mc).await?;
    let dir = root(app).join("lab").join(format!("{loader}-{mc}-{lv}"));
    tokio::fs::create_dir_all(&dir).await.map_err(e)?;
    match loader {
        "fabric" => {
            let jar = dir.join("fabric-server-launch.jar");
            if !jar.exists() {
                let inst = get_json(c, &format!("{}/versions/installer", config::FABRIC_META)).await.map_err(|_| "Connexion requise pour préparer le serveur de test".to_string())?;
                let iv = inst.as_array().and_then(|a| a.iter().find(|x| x["stable"] == true)).and_then(|x| x["version"].as_str()).ok_or("Installateur Fabric introuvable")?.to_string();
                fetch(c, &format!("{}/versions/loader/{mc}/{lv}/{iv}/server/jar", config::FABRIC_META), &jar).await?;
            }
            Ok((dir, Launch::Jar("fabric-server-launch.jar".into())))
        }
        "forge" | "neoforge" => {
            let rel = if loader == "forge" { format!("libraries/net/minecraftforge/forge/{mc}-{lv}/") } else { format!("libraries/net/neoforged/neoforge/{lv}/") };
            let args = format!("{rel}{}", if cfg!(windows) { "win_args.txt" } else { "unix_args.txt" });
            if !dir.join(&args).exists() {
                let (url, name) = if loader == "forge" {
                    (format!("{}/{mc}-{lv}/forge-{mc}-{lv}-installer.jar", config::FORGE_MAVEN), "installer.jar")
                } else { (format!("{}/{lv}/neoforge-{lv}-installer.jar", config::NEOFORGE_MAVEN), "installer.jar") };
                let jar = dir.join(name);
                fetch(c, &url, &jar).await?;
                let java = ensure_java(app, c, &shared, vjson).await.unwrap_or_else(|_| "java".to_string());
                let mut cmd = tokio::process::Command::new(&java);
                cmd.arg("-jar").arg(&jar).arg("--installServer").current_dir(&dir).stdout(Stdio::null()).stderr(Stdio::null());
                #[cfg(windows)]
                cmd.creation_flags(0x0800_0000);
                let st = tokio::time::timeout(Duration::from_secs(900), async { cmd.spawn().map_err(e)?.wait().await.map_err(e) })
                    .await.map_err(|_| "Installateur trop long".to_string())??;
                let _ = tokio::fs::remove_file(&jar).await;
                if !st.success() || !dir.join(&args).exists() { return Err("Ce Forge ou NeoForge ne se lance pas par fichier d'arguments (avant Minecraft 1.17 : non pris en charge)".into()) }
            }
            Ok((dir, Launch::Args(args)))
        }
        _ => Err("Test dynamique indisponible pour ce loader".into()),
    }
}

async fn serve_test(app: &AppHandle, meta: &Meta, jar: &Path, mc: &str, settle: u64) -> Result<Value, String> {
    let c = client()?;
    let shared = root(app).join("shared");
    phase(app, "prepare", 0.0);
    let vjson = load_version_json(&c, &shared, mc).await?;
    let (dir, launch) = prepare_base(app, &c, meta.loader, mc, &vjson).await?;
    let java = ensure_java(app, &c, &shared, &vjson).await.unwrap_or_else(|_| "java".to_string());
    for d in ["mods", "config", "world", "logs", "crash-reports"] { let _ = tokio::fs::remove_dir_all(dir.join(d)).await; }
    let _ = tokio::fs::remove_file(dir.join("plume-gc.log")).await;
    let mods = dir.join("mods");
    tokio::fs::create_dir_all(&mods).await.map_err(e)?;
    let fname = jar.file_name().and_then(|n| n.to_str()).unwrap_or("mod.jar").to_string();
    tokio::fs::copy(jar, mods.join(&fname)).await.map_err(e)?;

    // Dépendances obligatoires : récupérées sur Modrinth quand c'est possible.
    phase(app, "deps", 0.0);
    let ls: &[&str] = loaders::mod_loaders(meta.loader).unwrap_or(&[]);
    let (mut installed, mut missing): (Vec<String>, Vec<String>) = (vec![], vec![]);
    for (id, _, required) in &meta.deps {
        if !*required || BUILTIN.contains(&id.as_str()) { continue }
        let slug = if id == "fabric" || id.starts_with("fabric-") { "fabric-api".to_string() } else { id.clone() };
        let mut got = None;
        for cand in [slug.clone(), slug.replace('_', "-")] {
            if let Ok(v) = modrinth::fetch_into(&c, &cand, mc, ls, &mods).await { got = Some(v); break }
        }
        match got { Some(v) => installed.extend(v), None => missing.push(id.clone()) }
    }
    installed.sort(); installed.dedup();

    let port = std::net::TcpListener::bind("127.0.0.1:0").map(|l| l.local_addr().map(|a| a.port()).unwrap_or(25599)).unwrap_or(25599);
    tokio::fs::write(dir.join("eula.txt"), "eula=true\n").await.map_err(e)?;
    tokio::fs::write(dir.join("server.properties"), format!("online-mode=false\nserver-ip=127.0.0.1\nserver-port={port}\nmax-players=1\nview-distance=4\nsimulation-distance=4\nspawn-protection=0\ngenerate-structures=false\nenable-status=false\nmotd=Plume Lab\nsync-chunk-writes=false\n")).await.map_err(e)?;
    let modern = vjson["javaVersion"]["majorVersion"].as_u64().unwrap_or(17) >= 9;
    let mut jvm: Vec<String> = vec!["-Xmx2G".into(), "-Xms512M".into()];
    if modern { jvm.push("-Xlog:gc:file=plume-gc.log:time".into()); }
    tokio::fs::write(dir.join("user_jvm_args.txt"), jvm.join("\n") + "\n").await.map_err(e)?;

    let log_path = dir.join("plume-server.log");
    let f1 = std::fs::File::create(&log_path).map_err(e)?;
    let f2 = f1.try_clone().map_err(e)?;
    let mut cmd = tokio::process::Command::new(&java);
    match &launch {
        Launch::Jar(j) => { cmd.args(&jvm).arg("-jar").arg(j).arg("nogui"); }
        Launch::Args(a) => { cmd.arg("@user_jvm_args.txt").arg(format!("@{a}")).arg("nogui"); }
    }
    cmd.current_dir(&dir).stdin(Stdio::piped()).stdout(Stdio::from(f1)).stderr(Stdio::from(f2));
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let began = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut child = cmd.spawn().map_err(|_| format!("Impossible de lancer Java ({java})."))?;
    let pid = child.id().unwrap_or(0);

    let t0 = Instant::now();
    let (mut ready_at, mut done_secs, mut stop_at): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let (mut peak_kb, mut cpu_ready, mut cpu_last, mut last_el) = (0u64, 0f64, 0f64, 0f64);
    let (mut stopped, mut timed_out, mut exit_code) = (false, false, None::<i32>);
    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        if let Ok(Some(st)) = child.try_wait() { exit_code = st.code(); break }
        let el = t0.elapsed().as_secs_f64();
        last_el = el;
        if pid != 0 { if let Some((rss, cpu)) = proc_stats(pid) { peak_kb = peak_kb.max(rss); cpu_last = cpu; } }
        if ready_at.is_none() {
            if let Some(d) = parse_done(&read_tail(&log_path, 300_000)) { ready_at = Some(el); done_secs = Some(d); cpu_ready = cpu_last; }
        }
        phase(app, if stopped { "stop" } else if ready_at.is_some() { "observe" } else { "boot" }, el - ready_at.unwrap_or(0.0));
        if !stopped {
            if let Some(r) = ready_at {
                if el - r >= settle as f64 {
                    stopped = true; stop_at = Some(el);
                    if let Some(mut si) = child.stdin.take() { let _ = si.write_all(b"stop\n").await; let _ = si.flush().await; }
                }
            } else if el > 300.0 { stopped = true; timed_out = true; let _ = child.start_kill(); }
        } else if stop_at.map_or(false, |s| el - s > 40.0) { let _ = child.start_kill(); }
    }
    let _ = child.wait().await;
    phase(app, "report", 0.0);
    let mut r = analyze(&dir, began);
    let observed = (last_el - ready_at.unwrap_or(last_el)).max(1.0);
    let crashed = r["crash_report"].is_string() || ready_at.is_none() || (!stopped && exit_code.map_or(true, |c| c != 0));
    let issues = r["errors"].as_u64().unwrap_or(0) > 0 || !r["mixin"].as_array().map_or(true, |a| a.is_empty()) || !r["deps"].as_array().map_or(true, |a| a.is_empty()) || !missing.is_empty();
    let verdict = if timed_out { "timeout" } else if crashed { "crash" } else if issues { "warn" } else { "ok" };
    let _ = tokio::fs::remove_dir_all(dir.join("world")).await;
    r["verdict"] = json!(verdict);
    r["done_secs"] = json!(done_secs);
    r["ready_secs"] = json!(ready_at);
    r["peak_rss_mb"] = json!(peak_kb / 1024);
    r["boot_cpu_secs"] = if cpu_ready > 0.0 { json!((cpu_ready * 10.0).round() / 10.0) } else { Value::Null };
    r["idle_cpu_pct"] = if ready_at.is_some() && cpu_last >= cpu_ready { json!(((cpu_last - cpu_ready) / observed * 1000.0).round() / 10.0) } else { Value::Null };
    r["exit_code"] = json!(exit_code);
    r["deps_installed"] = json!(installed);
    r["deps_missing"] = json!(missing);
    r["settle"] = json!(settle);
    Ok(r)
}

/// Teste un mod : analyse du fichier, puis (si possible) démarrage invisible d'un serveur de test.
#[tauri::command]
pub async fn lab_test(app: AppHandle, path: String, mc: String, seconds: Option<u64>, accept_eula: bool, client_only: bool) -> Result<Value, String> {
    if BUSY.swap(true, AO::SeqCst) { return Err("Un test est déjà en cours".into()) }
    let res = lab_inner(&app, &path, &mc, seconds.unwrap_or(15).clamp(5, 120), accept_eula, client_only).await;
    BUSY.store(false, AO::SeqCst);
    res
}

async fn lab_inner(app: &AppHandle, path: &str, mc: &str, settle: u64, eula: bool, client_only: bool) -> Result<Value, String> {
    let ok_mc = !mc.is_empty() && mc.len() <= 40 && mc.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c));
    let p = Path::new(path);
    if !ok_mc || !p.is_file() || !path.to_lowercase().ends_with(".jar") { return Err("Fichier .jar invalide".into()) }
    phase(app, "inspect", 0.0);
    let (meta, info) = inspect(p, mc)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let skipped = if info["mc_ok"] == false { Some("mc") }
        else if client_only || meta.env == "client" { Some("client") }
        else if meta.loader == "quilt" { Some("quilt") }
        else if !eula { Some("eula") }
        else { None };
    let dynamic = if skipped.is_none() { Some(serve_test(app, &meta, p, mc, settle).await?) } else { None };
    let verdict = if info["mc_ok"] == false { "incompatible" } else { dynamic.as_ref().and_then(|d| d["verdict"].as_str()).unwrap_or("static") };
    Ok(json!({"inspect": info, "dynamic": dynamic, "skipped": skipped, "verdict": verdict, "mc": mc, "when": now}))
}
