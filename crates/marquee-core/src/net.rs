//! The shared request scheduler. Every outbound call in Marquee goes through here, which gives:
//!
//! * a token bucket per host, so no service's rate limit is ever exceeded
//! * retry with backoff on 429 (honouring `Retry-After`) and 5xx
//! * a SQLite response cache with TTLs and `ETag` revalidation
//! * stale-from-cache answers when the machine is offline

use crate::db::Db;
use crate::error::{Error, Result};
use crate::model::now;
use async_trait::async_trait;
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|e| {
            let head = String::from_utf8_lossy(&self.body[..self.body.len().min(160)]).to_string();
            Error::msg(format!("Unexpected response ({e}): {head}"))
        })
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse>;
}

pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new(user_agent: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::msg(format!("Couldn't start the network client: {e}")))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Transport for ReqwestTransport {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes()).unwrap_or(reqwest::Method::GET);
        let mut b = self.client.request(method, &req.url);
        for (k, v) in &req.headers {
            b = b.header(k, v);
        }
        if let Some(body) = &req.body {
            b = b.body(body.clone());
        }
        let host = url::Url::parse(&req.url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        if host == "localhost" || host == "127.0.0.1" || host == "[::1]" {
            // A local model may need to load into memory before it answers.
            b = b.timeout(Duration::from_secs(180));
        }
        let resp = b.send().await.map_err(|_| Error::Offline(host.clone()))?;
        let status = resp.status().as_u16();
        let headers = resp.headers().iter().filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string()))).collect();
        let body = resp.bytes().await.map_err(|_| Error::Offline(host))?.to_vec();
        Ok(HttpResponse { status, headers, body })
    }
}

/// Requests per second and burst size for each host. Numbers sit a little under each
/// service's published limit.
fn limit_for(host: &str) -> (f64, f64) {
    match host {
        "api.themoviedb.org" => (20.0, 20.0), // ~50 rps allowed
        "api.igdb.com" => (3.5, 4.0),         // 4 rps
        "id.twitch.tv" => (1.0, 2.0),
        "musicbrainz.org" => (0.9, 1.0), // 1 rps, strictly
        "coverartarchive.org" => (6.0, 12.0),
        "ws.audioscrobbler.com" => (4.0, 5.0), // ~5 rps
        "api.spotify.com" => (4.0, 8.0),       // rolling 30 s window, undisclosed
        "accounts.spotify.com" => (1.0, 2.0),
        "api.trakt.tv" => (3.0, 6.0),           // 1000 per 5 min
        "api.steampowered.com" => (2.0, 4.0),   // 100k per day
        "store.steampowered.com" => (0.6, 3.0), // ~200 per 5 min
        "letterboxd.com" => (0.5, 2.0),
        // Artwork CDNs: a screen of posters loads at once.
        "image.tmdb.org" | "images.igdb.com" | "i.scdn.co" | "lastfm.freetls.fastly.net" | "i.ytimg.com" | "cdn.myanimelist.net" => (150.0, 200.0), // CDNs: no published limit
        "api.myanimelist.net" => (2.0, 4.0),
        "datasets.imdbws.com" => (2.0, 2.0),
        h if h.ends_with("steamstatic.com") => (150.0, 200.0),
        "archive.org" => (6.0, 12.0),
        "localhost" | "127.0.0.1" => (50.0, 50.0), // Ollama
        _ => (4.0, 6.0),
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
    rate: f64,
    burst: f64,
    /// Set after a 429: nothing goes to this host before this instant.
    blocked_until: Option<Instant>,
}

struct Inner {
    transport: Box<dyn Transport>,
    db: Db,
    buckets: Mutex<HashMap<String, Bucket>>,
}

#[derive(Clone)]
pub struct Http {
    inner: Arc<Inner>,
}

pub struct Call<'a> {
    http: &'a Http,
    service: &'static str,
    req: HttpRequest,
    ttl: Option<i64>,
    cache_vary: String,
}

impl Http {
    pub fn new(transport: Box<dyn Transport>, db: Db) -> Self {
        Http { inner: Arc::new(Inner { transport, db, buckets: Mutex::new(HashMap::new()) }) }
    }

    pub fn get(&self, service: &'static str, url: impl Into<String>) -> Call<'_> {
        Call { http: self, service, req: HttpRequest { method: "GET".into(), url: url.into(), ..Default::default() }, ttl: None, cache_vary: String::new() }
    }

    pub fn post(&self, service: &'static str, url: impl Into<String>) -> Call<'_> {
        Call { http: self, service, req: HttpRequest { method: "POST".into(), url: url.into(), ..Default::default() }, ttl: None, cache_vary: String::new() }
    }

    pub fn request(&self, service: &'static str, method: &str, url: impl Into<String>) -> Call<'_> {
        Call { http: self, service, req: HttpRequest { method: method.into(), url: url.into(), ..Default::default() }, ttl: None, cache_vary: String::new() }
    }

    /// Waits until this host's bucket has a token.
    async fn acquire(&self, host: &str) {
        loop {
            let wait = {
                let mut buckets = self.inner.buckets.lock().unwrap_or_else(|e| e.into_inner());
                let b = buckets.entry(host.to_string()).or_insert_with(|| {
                    let (rate, burst) = limit_for(host);
                    Bucket { tokens: burst, last: Instant::now(), rate, burst, blocked_until: None }
                });
                let now = Instant::now();
                if let Some(until) = b.blocked_until {
                    if until > now {
                        Some(until - now)
                    } else {
                        b.blocked_until = None;
                        continue;
                    }
                } else {
                    b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * b.rate).min(b.burst);
                    b.last = now;
                    if b.tokens >= 1.0 {
                        b.tokens -= 1.0;
                        None
                    } else {
                        Some(Duration::from_secs_f64((1.0 - b.tokens) / b.rate))
                    }
                }
            };
            match wait {
                None => return,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }

    fn block_host(&self, host: &str, d: Duration) {
        if let Some(b) = self.inner.buckets.lock().unwrap_or_else(|e| e.into_inner()).get_mut(host) {
            b.blocked_until = Some(Instant::now() + d);
            b.tokens = 0.0;
        }
    }

    fn cache_get(&self, key: &str) -> Option<(Vec<u8>, Option<String>, bool)> {
        let conn = self.inner.db.lock();
        conn.query_row("SELECT body, etag, expires_at > ?2 FROM cache WHERE key = ?1", params![key, now()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().ok().flatten()
    }

    fn cache_put(&self, key: &str, body: &[u8], etag: Option<&str>, ttl: i64) {
        let expires = (chrono::Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let _ = self.inner.db.lock().execute(
            "INSERT INTO cache (key, body, etag, fetched_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (key) DO UPDATE SET body = excluded.body, etag = excluded.etag, fetched_at = excluded.fetched_at, expires_at = excluded.expires_at",
            params![key, body, etag, now(), expires],
        );
    }

    fn cache_touch(&self, key: &str, ttl: i64) {
        let expires = (chrono::Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let _ = self.inner.db.lock().execute("UPDATE cache SET expires_at = ?2, fetched_at = ?3 WHERE key = ?1", params![key, expires, now()]);
    }

    /// Drops cached responses whose key contains `fragment` (used when a 200 turned out to be an error).
    pub fn forget(&self, fragment: &str) {
        let _ = self.inner.db.lock().execute("DELETE FROM cache WHERE instr(key, ?1) > 0", [fragment]);
    }

    pub fn clear_cache(&self) -> Result<usize> {
        Ok(self.inner.db.lock().execute("DELETE FROM cache", [])?)
    }

    pub fn prune_cache(&self) -> Result<usize> {
        // Keep expired rows for a month: they're what the app shows while offline.
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        Ok(self.inner.db.lock().execute("DELETE FROM cache WHERE expires_at < ?1", [cutoff])?)
    }
}

impl<'a> Call<'a> {
    pub fn header(mut self, k: &str, v: impl Into<String>) -> Self {
        self.req.headers.push((k.to_string(), v.into()));
        self
    }

    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }

    pub fn query(mut self, pairs: &[(&str, &str)]) -> Self {
        let mut sep = if self.req.url.contains('?') { '&' } else { '?' };
        for (k, v) in pairs {
            self.req.url.push(sep);
            self.req.url.push_str(&urlencoding::encode(k));
            self.req.url.push('=');
            self.req.url.push_str(&urlencoding::encode(v));
            sep = '&';
        }
        self
    }

    pub fn json_body<T: serde::Serialize>(mut self, body: &T) -> Self {
        self.req.body = serde_json::to_vec(body).ok();
        self.header("Content-Type", "application/json")
    }

    pub fn form(mut self, pairs: &[(&str, &str)]) -> Self {
        let body = pairs.iter().map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v))).collect::<Vec<_>>().join("&");
        self.req.body = Some(body.into_bytes());
        self.header("Content-Type", "application/x-www-form-urlencoded")
    }

    pub fn text_body(mut self, body: impl Into<String>) -> Self {
        self.req.body = Some(body.into().into_bytes());
        self
    }

    /// Cache the response for this many seconds. GET (and IGDB-style query POSTs) only.
    pub fn ttl(mut self, secs: i64) -> Self {
        self.ttl = Some(secs);
        self
    }

    /// Extra text mixed into the cache key (for example the account a response belongs to).
    pub fn vary(mut self, v: impl Into<String>) -> Self {
        self.cache_vary = v.into();
        self
    }

    fn cache_key(&self) -> String {
        let mut key = format!("{} {}", self.req.method, self.req.url);
        if let Some(body) = &self.req.body {
            key.push(' ');
            key.push_str(&String::from_utf8_lossy(body));
        }
        if !self.cache_vary.is_empty() {
            key.push_str(" #");
            key.push_str(&self.cache_vary);
        }
        key
    }

    pub async fn json<T: DeserializeOwned>(self) -> Result<T> {
        self.send().await?.json()
    }

    pub async fn text(self) -> Result<String> {
        Ok(self.send().await?.text())
    }

    /// Sends the call. Non-2xx statuses become errors.
    pub async fn send(self) -> Result<HttpResponse> {
        let service = self.service;
        let resp = self.send_raw().await?;
        if (200..300).contains(&resp.status) {
            return Ok(resp);
        }
        let detail = {
            let t = resp.text();
            let t = t.trim();
            if t.len() > 300 {
                format!("{}…", &t[..t.char_indices().nth(300).map(|c| c.0).unwrap_or(t.len())])
            } else {
                t.to_string()
            }
        };
        match resp.status {
            401 => Err(Error::Reauth(service.to_string())),
            status => Err(Error::Api { service: service.to_string(), status, detail }),
        }
    }

    /// Sends the call and returns whatever status came back (after retries).
    pub async fn send_raw(mut self) -> Result<HttpResponse> {
        let http = self.http;
        let host = url::Url::parse(&self.req.url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        let key = self.cache_key();
        let mut cached = None;
        if let Some(ttl) = self.ttl {
            if let Some((body, etag, fresh)) = http.cache_get(&key) {
                if fresh {
                    return Ok(HttpResponse { status: 200, headers: vec![("x-marquee-cache".into(), "hit".into())], body });
                }
                if let Some(e) = &etag {
                    self.req.headers.push(("If-None-Match".into(), e.clone()));
                }
                cached = Some((body, ttl));
            }
        }

        let mut attempt = 0;
        loop {
            attempt += 1;
            http.acquire(&host).await;
            match http.inner.transport.send(&self.req).await {
                Err(e) => {
                    // Offline: serve what we have, however old.
                    if let Some((body, _)) = cached {
                        tracing::debug!(url = %self.req.url, "offline, serving stale cache");
                        return Ok(HttpResponse { status: 200, headers: vec![("x-marquee-cache".into(), "stale".into())], body });
                    }
                    if attempt < 2 && e.is_offline() {
                        tokio::time::sleep(Duration::from_millis(400)).await;
                        continue;
                    }
                    return Err(e);
                }
                Ok(resp) => {
                    if resp.status == 304 {
                        if let Some((body, ttl)) = cached {
                            http.cache_touch(&key, ttl);
                            return Ok(HttpResponse { status: 200, headers: vec![("x-marquee-cache".into(), "revalidated".into())], body });
                        }
                    }
                    if resp.status == 429 && attempt <= 3 {
                        let wait = resp.header("retry-after").and_then(|v| v.parse::<u64>().ok()).unwrap_or(2u64.pow(attempt)).min(30);
                        tracing::warn!(host = %host, wait, "rate limited");
                        http.block_host(&host, Duration::from_secs(wait));
                        continue;
                    }
                    if resp.status >= 500 && attempt <= 3 {
                        tokio::time::sleep(Duration::from_millis(400 * 2u64.pow(attempt - 1))).await;
                        continue;
                    }
                    if resp.status >= 500 {
                        if let Some((body, _)) = cached {
                            return Ok(HttpResponse { status: 200, headers: vec![("x-marquee-cache".into(), "stale".into())], body });
                        }
                    }
                    if let Some(ttl) = self.ttl {
                        if (200..300).contains(&resp.status) {
                            http.cache_put(&key, &resp.body, resp.header("etag"), ttl);
                        }
                    }
                    return Ok(resp);
                }
            }
        }
    }
}

/// A transport that answers from canned responses. Used by every API-client test, so CI never
/// makes a live call.
pub mod testing {
    use super::*;

    /// A canned answer: what the URL must contain, then status, headers and body.
    type Route = (String, u16, Vec<(String, String)>, Vec<u8>);

    #[derive(Default)]
    pub struct FixtureTransport {
        routes: Mutex<Vec<Route>>,
        pub log: Mutex<Vec<HttpRequest>>,
        pub offline: Mutex<bool>,
    }

    impl FixtureTransport {
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        /// Any request whose "METHOD url" contains `pattern` gets this response.
        /// Later routes win, so a test can override a default.
        pub fn on(&self, pattern: &str, status: u16, body: &str) -> &Self {
            self.routes.lock().unwrap().push((pattern.to_string(), status, vec![], body.as_bytes().to_vec()));
            self
        }

        pub fn on_with_headers(&self, pattern: &str, status: u16, headers: &[(&str, &str)], body: &str) -> &Self {
            let h = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            self.routes.lock().unwrap().push((pattern.to_string(), status, h, body.as_bytes().to_vec()));
            self
        }

        /// A binary response (images).
        pub fn routes_bytes(&self, pattern: &str, body: Vec<u8>) {
            self.routes.lock().unwrap().push((pattern.to_string(), 200, vec![], body));
        }

        pub fn set_offline(&self, offline: bool) {
            *self.offline.lock().unwrap() = offline;
        }

        pub fn calls(&self, pattern: &str) -> usize {
            self.log.lock().unwrap().iter().filter(|r| format!("{} {}", r.method, r.url).contains(pattern)).count()
        }

        pub fn last(&self, pattern: &str) -> Option<HttpRequest> {
            self.log.lock().unwrap().iter().rev().find(|r| format!("{} {}", r.method, r.url).contains(pattern)).cloned()
        }
    }

    pub struct Shared(pub Arc<FixtureTransport>);

    #[async_trait]
    impl Transport for Shared {
        async fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
            self.0.log.lock().unwrap().push(req.clone());
            if *self.0.offline.lock().unwrap() {
                return Err(Error::Offline("test".into()));
            }
            let line = format!("{} {}", req.method, req.url);
            let routes = self.0.routes.lock().unwrap();
            match routes.iter().rev().find(|(p, ..)| line.contains(p.as_str())) {
                Some((_, status, headers, body)) => Ok(HttpResponse { status: *status, headers: headers.clone(), body: body.clone() }),
                None => Ok(HttpResponse { status: 404, headers: vec![], body: format!("no fixture for {line}").into_bytes() }),
            }
        }
    }

    pub fn http(db: &Db) -> (Http, Arc<FixtureTransport>) {
        let t = FixtureTransport::new();
        (Http::new(Box::new(Shared(t.clone())), db.clone()), t)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[tokio::test]
    async fn fresh_cache_hits_skip_the_network() {
        let db = Db::memory().unwrap();
        let (http, t) = http(&db);
        t.on("GET https://example.com/a", 200, r#"{"n":1}"#);
        let a: serde_json::Value = http.get("Test", "https://example.com/a").ttl(60).json().await.unwrap();
        let b: serde_json::Value = http.get("Test", "https://example.com/a").ttl(60).json().await.unwrap();
        assert_eq!(a, b);
        assert_eq!(t.calls("example.com/a"), 1);
    }

    #[tokio::test]
    async fn expired_entries_revalidate_with_the_etag() {
        let db = Db::memory().unwrap();
        let (http, t) = http(&db);
        t.on_with_headers("example.com/e", 200, &[("ETag", "\"v1\"")], "first");
        assert_eq!(http.get("Test", "https://example.com/e").ttl(60).text().await.unwrap(), "first");
        db.lock().execute("UPDATE cache SET expires_at = '2000-01-01T00:00:00Z'", []).unwrap();
        t.on("example.com/e", 304, "");
        assert_eq!(http.get("Test", "https://example.com/e").ttl(60).text().await.unwrap(), "first");
        let sent = t.last("example.com/e").unwrap();
        assert!(sent.headers.iter().any(|(k, v)| k == "If-None-Match" && v == "\"v1\""));
        // Revalidation made it fresh again.
        http.get("Test", "https://example.com/e").ttl(60).text().await.unwrap();
        assert_eq!(t.calls("example.com/e"), 2);
    }

    #[tokio::test]
    async fn offline_serves_stale_cache_and_errors_without_one() {
        let db = Db::memory().unwrap();
        let (http, t) = http(&db);
        t.on("example.com/o", 200, "cached");
        http.get("Test", "https://example.com/o").ttl(60).text().await.unwrap();
        db.lock().execute("UPDATE cache SET expires_at = '2000-01-01T00:00:00Z'", []).unwrap();
        t.set_offline(true);
        assert_eq!(http.get("Test", "https://example.com/o").ttl(60).text().await.unwrap(), "cached");
        let err = http.get("Test", "https://example.com/never").ttl(60).text().await.unwrap_err();
        assert_eq!(err.kind(), "offline");
    }

    #[tokio::test(start_paused = true)]
    async fn a_429_waits_for_retry_after_then_succeeds() {
        let db = Db::memory().unwrap();
        let (http, t) = http(&db);
        t.on_with_headers("example.com/r", 429, &[("Retry-After", "3")], "slow down");
        let handle = {
            let http = http.clone();
            tokio::spawn(async move { http.get("Test", "https://example.com/r").text().await })
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        t.on("example.com/r", 200, "ok");
        let start = tokio::time::Instant::now();
        assert_eq!(handle.await.unwrap().unwrap(), "ok");
        assert!(start.elapsed() >= Duration::from_secs(1));
        assert_eq!(t.calls("example.com/r"), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn the_bucket_spaces_out_a_burst() {
        let db = Db::memory().unwrap();
        let (http, t) = http(&db);
        t.on("musicbrainz.org", 200, "{}");
        let start = tokio::time::Instant::now();
        for _ in 0..4 {
            http.get("MusicBrainz", "https://musicbrainz.org/ws/2/x").text().await.unwrap();
        }
        // 1 token of burst, then ~0.9/s: three more calls need at least three seconds.
        assert!(start.elapsed() >= Duration::from_secs(3), "{:?}", start.elapsed());
    }

    #[tokio::test]
    async fn errors_say_what_happened() {
        let db = Db::memory().unwrap();
        let (http, t) = http(&db);
        t.on("example.com/401", 401, "nope");
        t.on("example.com/403", 403, "forbidden for you");
        assert_eq!(http.get("Trakt", "https://example.com/401").text().await.unwrap_err().kind(), "reauth");
        let e = http.get("Spotify", "https://example.com/403").text().await.unwrap_err();
        assert!(e.to_string().contains("Spotify") && e.to_string().contains("403"));
    }
}
