//! Optional local AI through Ollama. Everything here talks to a loopback address only:
//! a non-local endpoint is refused, so nothing about the user's taste leaves the machine.

use crate::ctx::Ctx;
use crate::db::Db;
use crate::error::{Error, Result};
use crate::store;
use crate::util::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub enabled: bool,
    pub endpoint: String,
    /// Model for vibe search and written blurbs. Empty = pick the first chat model installed.
    pub chat_model: String,
    /// Model for embeddings. Empty = pick the first embedding model installed.
    pub embed_model: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings { enabled: true, endpoint: "http://localhost:11434".into(), chat_model: String::new(), embed_model: String::new() }
    }
}

impl AiSettings {
    pub fn load(db: &Db) -> Self {
        store::get_setting(db, "ai").unwrap_or_default()
    }

    /// The endpoint, if it is on this machine.
    pub fn local_endpoint(&self) -> Result<String> {
        let url = url::Url::parse(self.endpoint.trim()).map_err(|_| Error::msg("The Ollama address isn't a valid URL."))?;
        let local = match url.host() {
            Some(url::Host::Domain(d)) => d == "localhost",
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        if !local {
            return Err(Error::msg("Marquee only talks to Ollama on this PC (localhost), so nothing leaves your machine."));
        }
        Ok(self.endpoint.trim().trim_end_matches('/').to_string())
    }
}

fn is_embedding_model(name: &str, details: &Value) -> bool {
    let n = name.to_lowercase();
    n.contains("embed") || n.contains("bge") || n.contains("minilm") || n.contains("e5") || details["family"].as_str().map(|f| f.contains("bert")).unwrap_or(false)
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct AiStatus {
    pub enabled: bool,
    pub running: bool,
    pub endpoint: String,
    pub models: Vec<Value>,
    pub chat_model: Option<String>,
    pub embed_model: Option<String>,
    pub error: Option<String>,
}

/// Is Ollama running, and what's installed?
pub async fn status(ctx: &Ctx) -> AiStatus {
    let s = AiSettings::load(&ctx.db);
    let mut st = AiStatus { enabled: s.enabled, endpoint: s.endpoint.clone(), ..Default::default() };
    let base = match s.local_endpoint() {
        Ok(b) => b,
        Err(e) => {
            st.error = Some(e.to_string());
            return st;
        }
    };
    let Ok(v) = ctx.http.get("Ollama", format!("{base}/api/tags")).json::<Value>().await else { return st };
    st.running = true;
    let mut chat = vec![];
    let mut embed = vec![];
    for m in arr(&v, "models") {
        let Some(name) = crate::util::s(m, "name") else { continue };
        let embedding = is_embedding_model(&name, &m["details"]);
        if embedding {
            embed.push(name.clone())
        } else {
            chat.push(name.clone())
        }
        st.models.push(json!({ "name": name, "size": m["size"], "params": m["details"]["parameter_size"], "embedding": embedding }));
    }
    st.chat_model = if chat.contains(&s.chat_model) { Some(s.chat_model.clone()) } else { chat.first().cloned() };
    st.embed_model = if embed.contains(&s.embed_model) { Some(s.embed_model.clone()) } else { embed.iter().find(|m| m.contains("nomic")).or(embed.first()).cloned() };
    st
}

/// The models to use right now, or `None` when AI is off or Ollama isn't running.
pub async fn ready(ctx: &Ctx) -> Option<(String, AiStatus)> {
    let s = AiSettings::load(&ctx.db);
    if !s.enabled {
        return None;
    }
    let base = s.local_endpoint().ok()?;
    // Remember "not running" for a minute so every search doesn't wait on a dead port.
    if ctx.scratch_get("ollama.down").is_some() {
        return None;
    }
    let st = status(ctx).await;
    if !st.running {
        ctx.scratch_set("ollama.down", "1", 60);
        return None;
    }
    Some((base, st))
}

pub async fn generate(ctx: &Ctx, base: &str, model: &str, system: &str, prompt: &str, as_json: bool) -> Result<String> {
    let mut body = json!({ "model": model, "system": system, "prompt": prompt, "stream": false, "options": { "temperature": 0.2, "num_predict": 400 } });
    if as_json {
        body["format"] = json!("json");
    }
    let v: Value = ctx.http.post("Ollama", format!("{base}/api/generate")).json_body(&body).json().await?;
    crate::util::s(&v, "response").ok_or_else(|| Error::msg("Ollama returned an empty answer."))
}

pub async fn embed(ctx: &Ctx, base: &str, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let v: Value = ctx.http.post("Ollama", format!("{base}/api/embed")).json_body(&json!({ "model": model, "input": texts })).json().await?;
    let out: Vec<Vec<f32>> = arr(&v, "embeddings").iter().map(|e| e.as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default()).collect();
    if out.len() != texts.len() {
        return Err(Error::msg("Ollama returned the wrong number of embeddings."));
    }
    Ok(out)
}

pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        dot += (*x as f64) * (*y as f64);
        na += (*x as f64).powi(2);
        nb += (*y as f64).powi(2);
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Embeds titles that don't have a vector yet (a bounded batch per call).
pub async fn embed_missing(ctx: &Ctx, limit: usize) -> Result<usize> {
    let Some((base, st)) = ready(ctx).await else { return Ok(0) };
    let Some(model) = st.embed_model else { return Ok(0) };
    let todo: Vec<(i64, String)> = {
        let conn = ctx.db.lock();
        let mut stmt = conn.prepare(
            "SELECT i.id, i.media, i.title, COALESCE(i.overview, ''), i.data FROM items i
             WHERE i.media != 'artist' AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.item_id = i.id AND e.model = ?1)
             ORDER BY (EXISTS (SELECT 1 FROM user_events u WHERE u.item_id = i.id)) DESC, i.popularity DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![model, limit as i64], |r| {
                let data: Value = serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null);
                let list = |k: &str| data[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).take(12).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                let text = format!("{} ({}). Genres: {}. Themes: {}. {}", r.get::<_, String>(2)?, r.get::<_, String>(1)?, list("genres"), list("tags"), truncate(&r.get::<_, String>(3)?, 400));
                Ok((r.get::<_, i64>(0)?, text))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut done = 0;
    for chunk in todo.chunks(16) {
        let texts: Vec<String> = chunk.iter().map(|c| c.1.clone()).collect();
        let vectors = embed(ctx, &base, &model, &texts).await?;
        let conn = ctx.db.lock();
        for ((id, _), v) in chunk.iter().zip(vectors) {
            conn.execute("INSERT OR REPLACE INTO embeddings (item_id, model, vector) VALUES (?1, ?2, ?3)", rusqlite::params![id, model, to_blob(&v)])?;
            done += 1;
        }
    }
    Ok(done)
}

/// A per-item bonus in roughly -2..=2: how close each title's embedding is to the centroid of
/// the user's favourites, standardised across the catalog. Empty when no embeddings exist.
pub fn embedding_bonus(db: &Db, affinity: &HashMap<i64, f64>) -> Result<HashMap<i64, f64>> {
    let model = AiSettings::load(db).embed_model;
    let conn = db.lock();
    let model: Option<String> = if model.is_empty() { conn.query_row("SELECT model FROM embeddings GROUP BY model ORDER BY count(*) DESC LIMIT 1", [], |r| r.get(0)).ok() } else { Some(model) };
    let Some(model) = model else { return Ok(HashMap::new()) };
    let mut stmt = conn.prepare("SELECT item_id, vector FROM embeddings WHERE model = ?1")?;
    let all: Vec<(i64, Vec<f32>)> = stmt.query_map([&model], |r| Ok((r.get::<_, i64>(0)?, from_blob(&r.get::<_, Vec<u8>>(1)?))))?.collect::<rusqlite::Result<_>>()?;
    let dim = all.first().map(|a| a.1.len()).unwrap_or(0);
    let mut centroid = vec![0f32; dim];
    let mut total = 0.0;
    for (id, v) in &all {
        if let Some(a) = affinity.get(id).filter(|a| **a > 1.0) {
            if v.len() == dim {
                for (c, x) in centroid.iter_mut().zip(v) {
                    *c += x * *a as f32;
                }
                total += a;
            }
        }
    }
    if total <= 0.0 || all.len() < 8 {
        return Ok(HashMap::new());
    }
    let sims: Vec<(i64, f64)> = all.iter().map(|(id, v)| (*id, cosine(&centroid, v))).collect();
    let mean = sims.iter().map(|s| s.1).sum::<f64>() / sims.len() as f64;
    let sd = (sims.iter().map(|s| (s.1 - mean).powi(2)).sum::<f64>() / sims.len() as f64).sqrt().max(1e-6);
    Ok(sims.into_iter().map(|(id, s)| (id, ((s - mean) / sd).clamp(-2.0, 2.0))).collect())
}

/// A richer written "why you'd like this", when a chat model is available.
pub async fn blurb(ctx: &Ctx, title: &str, media: &str, overview: &str, reasons: &[String], liked: &[String]) -> Option<String> {
    let (base, st) = ready(ctx).await?;
    let model = st.chat_model?;
    let system = "You write two warm, specific sentences telling one person why they might enjoy a title. Use only the facts given. No spoilers, no hype words, no lists, no quotation marks. Address them as \"you\".";
    let prompt =
        format!("Title: {title} ({media})\nWhat it is: {}\nWhy the recommender picked it: {}\nThings this person loves: {}\n\nWrite the two sentences.", truncate(overview, 500), reasons.join("; ").replace('*', ""), liked.join(", "));
    let text = generate(ctx, &base, &model, system, &prompt, false).await.ok()?;
    let text = text.trim().trim_matches('"').to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn only_loopback_endpoints_are_allowed() {
        let ok = |e: &str| AiSettings { endpoint: e.into(), ..Default::default() }.local_endpoint();
        assert_eq!(ok("http://localhost:11434/").unwrap(), "http://localhost:11434");
        assert!(ok("http://127.0.0.1:11434").is_ok());
        assert!(ok("http://[::1]:11434").is_ok());
        assert!(ok("https://ollama.example.com").unwrap_err().to_string().contains("this PC"));
        assert!(ok("http://192.168.1.20:11434").is_err());
        assert!(ok("not a url").is_err());
    }

    #[tokio::test]
    async fn status_detects_models_and_picks_defaults() {
        let t = ctx();
        t.net.on(
            "localhost:11434/api/tags",
            200,
            r#"{"models":[
            {"name":"llama3.2:3b","size":2019393189,"details":{"family":"llama","parameter_size":"3.2B"}},
            {"name":"nomic-embed-text:latest","size":274302450,"details":{"family":"nomic-bert","parameter_size":"137M"}}]}"#,
        );
        let st = status(&t.ctx).await;
        assert!(st.running);
        assert_eq!(st.chat_model.as_deref(), Some("llama3.2:3b"));
        assert_eq!(st.embed_model.as_deref(), Some("nomic-embed-text:latest"));
        assert_eq!(st.models.len(), 2);
    }

    #[tokio::test]
    async fn when_ollama_is_off_everything_falls_back_quietly() {
        let t = ctx();
        t.net.set_offline(true);
        assert!(!status(&t.ctx).await.running);
        assert!(ready(&t.ctx).await.is_none());
        assert_eq!(embed_missing(&t.ctx, 10).await.unwrap(), 0);
        assert!(blurb(&t.ctx, "X", "movie", "", &[], &[]).await.is_none());
        // The "down" marker means the next call doesn't touch the network at all.
        let before = t.net.calls("api/tags");
        assert!(ready(&t.ctx).await.is_none());
        assert_eq!(t.net.calls("api/tags"), before);
    }

    #[test]
    fn vectors_round_trip_and_compare() {
        let v = vec![0.25f32, -1.5, 3.0];
        assert_eq!(from_blob(&to_blob(&v)), v);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }
}
