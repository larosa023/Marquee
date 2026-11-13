//! Bakes the app-level API keys from `.env` (or the environment, in CI) into the binary, so a
//! release build works out of the box. Keys the user enters in Settings always override these.

use std::collections::HashMap;

const KEYS: &[&str] = &["TMDB_API_KEY", "IGDB_CLIENT_ID", "IGDB_CLIENT_SECRET", "LASTFM_API_KEY", "LASTFM_API_SECRET", "SPOTIFY_CLIENT_ID", "TRAKT_CLIENT_ID", "TRAKT_CLIENT_SECRET", "STEAM_API_KEY", "ITAD_API_KEY"];

fn main() {
    let mut file: HashMap<String, String> = HashMap::new();
    for path in ["../.env", ".env"] {
        println!("cargo:rerun-if-changed={path}");
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    file.insert(k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string());
                }
            }
        }
    }
    for key in KEYS {
        println!("cargo:rerun-if-env-changed={key}");
        let value = std::env::var(key).ok().filter(|v| !v.is_empty()).or_else(|| file.get(*key).cloned()).unwrap_or_default();
        println!("cargo:rustc-env=MARQUEE_{key}={value}");
    }
    tauri_build::build()
}
