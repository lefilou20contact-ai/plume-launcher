use std::{env, fs};

const KEYS: [&str; 4] = ["PLUME_MS_CLIENT_ID", "PLUME_CURSEFORGE_API_KEY", "PLUME_BACKEND_URL", "PLUME_BACKEND_ANON_KEY"];

fn main() {
    // En local : lit ../.env. Sur GitHub : les variables viennent des secrets.
    println!("cargo:rerun-if-changed=../.env");
    for k in KEYS { println!("cargo:rerun-if-env-changed={k}"); }
    if let Ok(t) = fs::read_to_string("../.env") {
        for l in t.lines() {
            let l = l.trim();
            if l.starts_with('#') { continue }
            if let Some((k, v)) = l.split_once('=') {
                let (k, v) = (k.trim(), v.trim().trim_matches('"'));
                if KEYS.contains(&k) && env::var(k).is_err() && !v.is_empty() {
                    println!("cargo:rustc-env={k}={v}");
                }
            }
        }
    }
    tauri_build::build()
}
