//! Where OAuth tokens and API keys live. On Windows the app supplies a store backed by
//! Credential Manager; nothing secret is ever written to SQLite, settings files or backups.

use crate::error::Result;
use std::collections::HashMap;
use std::sync::Mutex;

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// Every secret Marquee can store. "Delete all my data" walks this list.
pub const SECRET_KEYS: &[&str] = &[
    "trakt.access",
    "trakt.refresh",
    "spotify.access",
    "spotify.refresh",
    "lastfm.session",
    "key.tmdb",
    "key.igdb_client_id",
    "key.igdb_client_secret",
    "key.spotify_client_id",
    "key.trakt_client_id",
    "key.trakt_client_secret",
    "key.lastfm_key",
    "key.lastfm_secret",
    "key.steam",
    "key.itad",
];

/// In-memory store for tests and for running the engine outside the app.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl MemorySecrets {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0.lock().unwrap().insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}
