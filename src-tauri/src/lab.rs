//! Plume Lab : lance une instance, mesure et analyse (pour les créateurs de mods, modpacks et packs).
use crate::{instances, LAST_EXIT, RUNNING};
use serde_json::{json, Value};
use std::{path::Path, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter};

// Mémoire (Ko) et temps processeur (secondes) d'un processus.
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
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(p) else { return String::new() };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(size.saturating_sub(max)));
    let mut b = Vec::new();
    let _ = f.take(max).read_to_end(&mut b);
    String::from_utf8_lossy(&b).to_string()
}

// Le jeu est « prêt » quand le moteur de son démarre : les ressources sont chargées, le menu s'affiche.
fn is_ready(d: &Path) -> bool {
    ["logs/latest.log", "launcher.log"].iter().any(|r| {
        let t = read_tail(&d.join(r), 400_000);
        t.contains("Sound engine started") || t.contains("OpenAL initialized")
    })
}

fn cut(l: &str) -> String { l.chars().take(220).collect() }

fn analyze(d: &Path, began: u64) -> Value {
    let log = read_tail(&d.join("logs/latest.log"), 4_000_000) + "\n" + &read_tail(&d.join("launcher.log"), 1_000_000);
    let (mut errors, mut warns) = (0u32, 0u32);
    let (mut mixin, mut deps, mut samples): (Vec<String>, Vec<String>, Vec<String>) = (vec![], vec![], vec![]);
    for l in log.lines() {
        let low = l.to_lowercase();
        if l.contains("/ERROR]") || l.contains("[ERROR]") || l.contains("/FATAL]") || l.contains("Exception in thread") {
            errors += 1;
            if samples.len() < 12 { samples.push(cut(l)); }
        } else if l.contains("/WARN]") || l.contains("[WARN]") { warns += 1; }
        if (l.contains("Mixin apply failed") || l.contains("InvalidMixinException")) && mixin.len() < 8 { mixin.push(cut(l)); }
        let dep_line = (low.contains("requires") && (low.contains("mod ") || low.contains("mod '")))
            || l.contains("Missing or unsupported mandatory dependencies") || l.contains("Incompatible mod set") || l.contains("Incompatible mods found");
        if dep_line && deps.len() < 8 { deps.push(cut(l)); }
    }
    let crash = std::fs::read_dir(d.join("crash-reports")).ok().and_then(|rd| rd.flatten().filter_map(|f| {
        let m = f.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
        if m + 5 >= began { f.file_name().into_string().ok() } else { None }
    }).max());
    let mods = std::fs::read_dir(d.join("mods")).map(|rd| rd.flatten().filter(|f| f.file_name().to_string_lossy().ends_with(".jar")).count()).unwrap_or(0);
    let pauses: Vec<f64> = read_tail(&d.join("plume-gc.log"), 8_000_000).lines().filter(|l| l.contains("Pause"))
        .filter_map(|l| l.trim_end().strip_suffix("ms")?.rsplit(' ').next()?.parse::<f64>().ok()).collect();
    let gc = if pauses.is_empty() { Value::Null } else {
        json!({"count": pauses.len(), "total_ms": pauses.iter().sum::<f64>().round(), "max_ms": pauses.iter().cloned().fold(0.0, f64::max).round()})
    };
    json!({"errors": errors, "warns": warns, "mixin": mixin, "deps": deps, "samples": samples, "crash_report": crash, "mods": mods, "gc": gc})
}

/// Test de lancement : démarre l'instance, mesure, la ferme au bout de `seconds` secondes après le menu, puis analyse les logs.
#[tauri::command]
pub async fn lab_run(app: AppHandle, instance_id: String, seconds: Option<u64>) -> Result<Value, String> {
    let inst = instances::get(&app, &instance_id).ok_or("Instance introuvable")?;
    let settle = seconds.unwrap_or(20).clamp(5, 300) as f64;
    let dir = instances::dir(&app, &instance_id)?;
    let began = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for f in ["logs/latest.log", "plume-gc.log"] { let _ = std::fs::remove_file(dir.join(f)); }
    if let Ok(mut m) = LAST_EXIT.lock() { m.remove(&instance_id); }
    let opts = json!({"ramGb": inst["ram_gb"].as_u64().unwrap_or(4), "nice": 0, "profile": 1, "hide": false, "lab": true, "fullscreen": false, "width": 854, "height": 480});
    crate::launch(app.clone(), instance_id.clone(), opts).await?;
    let t0 = Instant::now();
    let (mut ready_at, mut peak_kb, mut first_cpu, mut last_cpu) = (None::<f64>, 0u64, None::<f64>, 0f64);
    let (mut stopped, mut timed_out) = (false, false);
    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let pid = RUNNING.lock().ok().and_then(|r| r.get(&instance_id).copied());
        let Some(pid) = pid else { break };
        let el = t0.elapsed().as_secs_f64();
        if pid != 0 {
            if let Some((rss, cpu)) = proc_stats(pid) { peak_kb = peak_kb.max(rss); first_cpu.get_or_insert(cpu); last_cpu = cpu; }
        }
        if ready_at.is_none() && is_ready(&dir) { ready_at = Some(el); }
        let _ = app.emit("lab-progress", json!({"id": instance_id, "elapsed": el, "ready": ready_at, "rss_mb": peak_kb / 1024}));
        if !stopped {
            if let Some(r) = ready_at { if el - r >= settle { stopped = true; let _ = crate::stop_game(instance_id.clone()); } }
            else if el > 420.0 { stopped = true; timed_out = true; let _ = crate::stop_game(instance_id.clone()); }
        }
    }
    let wall = t0.elapsed().as_secs_f64().max(1.0);
    let code = LAST_EXIT.lock().ok().and_then(|m| m.get(&instance_id).copied());
    let mut r = analyze(&dir, began);
    let crashed = r["crash_report"].is_string() || (!stopped && code.map_or(true, |c| c != 0));
    let verdict = if timed_out { "timeout" } else if crashed { "crash" } else if ready_at.is_some() { "ok" } else { "unknown" };
    let cpu_secs = first_cpu.map(|f| (last_cpu - f).max(0.0)).unwrap_or(0.0);
    r["verdict"] = json!(verdict);
    r["ready_secs"] = json!(ready_at);
    r["peak_rss_mb"] = json!(peak_kb / 1024);
    r["cpu_cores"] = if first_cpu.is_some() { json!((cpu_secs / wall * 10.0).round() / 10.0) } else { Value::Null };
    r["exit_code"] = json!(code);
    r["when"] = json!(began);
    r["settle"] = json!(settle);
    Ok(r)
}
