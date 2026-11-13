//! The context every engine call runs in: storage, network, secrets and a way to talk to the UI.

use crate::db::Db;
use crate::error::{Error, Result};
use crate::net::Http;
use crate::secrets::SecretStore;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What the engine needs from whatever is hosting it (the Tauri app, or a test).
pub trait Host: Send + Sync {
    /// Opens a URL (or `steam://`, `spotify:` link) in the user's default handler.
    fn open_url(&self, url: &str);
    /// Sends an event to the UI.
    fn emit(&self, event: &str, payload: Value);
}

/// A host that records what it was asked to do. Used by tests.
#[derive(Default)]
pub struct RecordingHost {
    pub opened: Mutex<Vec<String>>,
    pub events: Mutex<Vec<(String, Value)>>,
}

impl Host for RecordingHost {
    fn open_url(&self, url: &str) {
        self.opened.lock().unwrap().push(url.to_string());
    }
    fn emit(&self, event: &str, payload: Value) {
        self.events.lock().unwrap().push((event.to_string(), payload));
    }
}

pub struct Ctx {
    pub db: Db,
    pub http: Http,
    pub secrets: Arc<dyn SecretStore>,
    pub host: Arc<dyn Host>,
    pub data_dir: PathBuf,
    pub version: String,
    /// App-level keys from `.env` (baked in at build time). Keys saved in Settings override them.
    pub default_keys: HashMap<String, String>,
    scratch: Mutex<HashMap<String, (String, i64)>>,
    busy: Mutex<Option<String>>,
}

/// name, what it unlocks, where to get it.
pub const API_KEYS: &[(&str, &str, &str)] = &[
    ("tmdb", "Movies and TV: new releases, details, trailers, where to watch", "https://www.themoviedb.org/settings/api"),
    ("igdb_client_id", "Games catalog (Twitch client ID)", "https://dev.twitch.tv/console/apps"),
    ("igdb_client_secret", "Games catalog (Twitch client secret)", "https://dev.twitch.tv/console/apps"),
    ("lastfm_key", "Music: similar artists, tags, your scrobbles", "https://www.last.fm/api/account/create"),
    ("lastfm_secret", "Last.fm sign-in (needed to love tracks)", "https://www.last.fm/api/account/create"),
    ("spotify_client_id", "Spotify sign-in", "https://developer.spotify.com/dashboard"),
    ("trakt_client_id", "Trakt sign-in", "https://trakt.tv/oauth/applications"),
    ("trakt_client_secret", "Trakt sign-in", "https://trakt.tv/oauth/applications"),
    ("steam", "Your Steam library, playtime and wishlist", "https://steamcommunity.com/dev/apikey"),
    ("itad", "Optional: all-time low prices for games", "https://isthereanydeal.com/apps/my/"),
    ("mal_client_id", "Anime: your MyAnimeList list and each season's new anime", "https://myanimelist.net/apiconfig"),
    ("mal_client_secret", "Only for a MyAnimeList app of type \"web\" (type \"other\" has none)", "https://myanimelist.net/apiconfig"),
];

impl Ctx {
    pub fn new(db: Db, http: Http, secrets: Arc<dyn SecretStore>, host: Arc<dyn Host>, data_dir: PathBuf, version: &str, default_keys: HashMap<String, String>) -> Self {
        Ctx { db, http, secrets, host, data_dir, version: version.to_string(), default_keys, scratch: Mutex::new(HashMap::new()), busy: Mutex::new(None) }
    }

    pub fn key(&self, name: &str) -> Option<String> {
        self.secrets.get(&format!("key.{name}")).filter(|v| !v.trim().is_empty()).or_else(|| self.default_keys.get(name).filter(|v| !v.trim().is_empty()).cloned()).map(|v| v.trim().to_string())
    }

    pub fn has_key(&self, name: &str) -> bool {
        self.key(name).is_some()
    }

    pub fn require_key(&self, name: &str) -> Result<String> {
        self.key(name).ok_or_else(|| {
            let what = API_KEYS.iter().find(|k| k.0 == name).map(|k| k.1).unwrap_or(name);
            Error::NotConfigured(format!("{what} needs an API key. Add it in Settings → Connections → API keys."))
        })
    }

    pub fn region(&self) -> String {
        crate::store::setting_or(&self.db, "region", "US".to_string())
    }

    pub fn language(&self) -> String {
        crate::store::setting_or(&self.db, "language", "en-US".to_string())
    }

    pub fn emit(&self, event: &str, payload: Value) {
        self.host.emit(event, payload);
    }

    /// Short-lived values that don't need to survive a restart (the Twitch app token).
    pub fn scratch_get(&self, key: &str) -> Option<String> {
        let now = chrono::Utc::now().timestamp();
        self.scratch.lock().unwrap().get(key).filter(|(_, exp)| *exp > now).map(|(v, _)| v.clone())
    }

    pub fn scratch_set(&self, key: &str, value: &str, ttl_secs: i64) {
        self.scratch.lock().unwrap().insert(key.to_string(), (value.to_string(), chrono::Utc::now().timestamp() + ttl_secs));
    }

    /// What the app is in the middle of ("syncing", "importing", "restoring"), if anything.
    /// Hangar asks before restarting the app for an update.
    pub fn busy_reason(&self) -> Option<String> {
        self.busy.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_busy(&self, reason: Option<&str>) {
        *self.busy.lock().unwrap_or_else(|e| e.into_inner()) = reason.map(String::from);
        self.emit("busy:changed", serde_json::json!({ "reason": reason }));
    }

    pub fn user_agent(&self) -> String {
        format!("Marquee/{} ( https://github.com/larosa023/Marquee )", self.version)
    }
}

/// A context wired to fixtures, for tests.
pub mod testing {
    use super::*;
    use crate::net::testing::FixtureTransport;
    use crate::secrets::MemorySecrets;

    pub struct TestCtx {
        pub ctx: Arc<Ctx>,
        pub net: Arc<FixtureTransport>,
        pub host: Arc<RecordingHost>,
        pub dir: PathBuf,
    }

    pub fn ctx() -> TestCtx {
        let db = Db::memory().unwrap();
        let (http, net) = crate::net::testing::http(&db);
        let host = Arc::new(RecordingHost::default());
        let dir = std::env::temp_dir().join(format!("marquee-test-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut keys = HashMap::new();
        for (name, ..) in API_KEYS {
            keys.insert(name.to_string(), format!("test-{name}"));
        }
        let ctx = Arc::new(Ctx::new(db, http, Arc::new(MemorySecrets::new()), host.clone(), dir.clone(), "0.1.0", keys));
        TestCtx { ctx, net, host, dir }
    }
}
