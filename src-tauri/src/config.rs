//! Toutes les clés et adresses externes du launcher sont ici, et uniquement ici.
//! Les valeurs sont injectées à la compilation : fichier `.env` en local,
//! secrets GitHub dans le workflow (voir build.rs et .github/workflows/release.yml).
#![allow(dead_code)]

macro_rules! key {
    ($name:literal) => {
        match option_env!($name) { Some(v) => v, None => "" }
    };
}

// --- Valeurs injectées (vides si non renseignées) ---
/// ID d'application Azure pour la connexion Microsoft (public par nature).
pub const MS_CLIENT_ID: &str = key!("PLUME_MS_CLIENT_ID");
/// Clé API CurseForge (optionnelle, Modrinth n'en demande pas).
pub const CURSEFORGE_API_KEY: &str = key!("PLUME_CURSEFORGE_API_KEY");
/// Backend des cosmétiques (Supabase ou autre).
pub const BACKEND_URL: &str = key!("PLUME_BACKEND_URL");
pub const BACKEND_ANON_KEY: &str = key!("PLUME_BACKEND_ANON_KEY");

// --- Adresses publiques, sans clé ---
pub const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";

/// Renvoie la valeur, ou une erreur claire si la clé n'a pas été renseignée.
pub fn require<'a>(name: &str, value: &'a str) -> Result<&'a str, String> {
    if value.is_empty() { Err(format!("Clé {name} non configurée (voir .env.example)")) } else { Ok(value) }
}
