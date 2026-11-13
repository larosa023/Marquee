//! Vibe search: "a short cozy game for a rainy Sunday", "albums that sound like Blade Runner".
//!
//! With Ollama running, a local model turns the sentence into media types, shared tags and
//! reference titles. Without it, the same structure is pulled out with the tag vocabulary.
//! Either way the search itself runs on the shared tags, so results cross media.

use super::vocab;
use crate::ai;
use crate::catalog;
use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::store;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct VibeQuery {
    pub media: Vec<Media>,
    /// Shared features: "t:cozy", "g:sci-fi"...
    pub features: Vec<String>,
    pub short: bool,
    /// Titles the query compares to ("like Blade Runner").
    pub like: Vec<String>,
}

fn media_in_text(text: &str) -> Vec<Media> {
    let t = format!(" {} ", text.to_lowercase());
    let mut out = vec![];
    for (words, m) in [
        (&["movie", "movies", "film", "films"][..], Media::Movie),
        (&["show", "shows", "series", "tv"][..], Media::Tv),
        (&["game", "games"][..], Media::Game),
        (&["album", "albums", "music", "record", "records", "soundtrack", "ep"][..], Media::Album),
    ] {
        if words.iter().any(|w| t.contains(&format!(" {w} ")) || t.contains(&format!(" {w},")) || t.contains(&format!(" {w}."))) {
            out.push(m);
        }
    }
    out
}

/// "…like Blade Runner", "similar to Outer Wilds", "sounds like *Dive*".
fn references(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = vec![];
    for marker in [" like ", " similar to ", " reminds me of ", " in the style of "] {
        if let Some(pos) = lower.find(marker) {
            let rest = text.get(pos + marker.len()..).unwrap_or("").trim();
            let rest = rest.split(&[',', '.', ';', '?', '!'][..]).next().unwrap_or(rest);
            let rest = rest.split(" but ").next().unwrap_or(rest).split(" for ").next().unwrap_or(rest).split(" with ").next().unwrap_or(rest);
            let cleaned = rest.trim().trim_matches(&['*', '"', '\'', '“', '”'][..]).trim();
            let cleaned = cleaned.strip_prefix("the movie ").or(cleaned.strip_prefix("the game ")).or(cleaned.strip_prefix("the album ")).unwrap_or(cleaned);
            if cleaned.len() >= 2 && cleaned.split_whitespace().count() <= 6 {
                out.push(cleaned.to_string());
            }
        }
    }
    out
}

/// The no-AI parser.
pub fn parse_local(text: &str) -> VibeQuery {
    let like = references(text);
    // Don't read tags out of the reference title itself ("like Dark Souls" isn't a request for t:dark).
    let mut without_refs = text.to_string();
    for r in &like {
        without_refs = without_refs.replace(r.as_str(), " ");
    }
    let features: Vec<String> = vocab::tags_in_text(&without_refs).into_iter().map(String::from).collect();
    let lower = format!(" {} ", text.to_lowercase());
    VibeQuery { media: media_in_text(text), short: lower.contains(" short ") || lower.contains(" quick ") || lower.contains(" bite sized ") || features.iter().any(|f| f == "t:short"), features, like }
}

/// Asks the local model for the same structure. Returns `None` if Ollama is off or the answer is unusable.
pub async fn parse_llm(ctx: &Ctx, text: &str) -> Option<VibeQuery> {
    let (base, st) = ai::ready(ctx).await?;
    let model = st.chat_model?;
    let tags: Vec<&str> = vocab::TAGS.iter().map(|t| t.0).collect();
    let system = format!(
        "You turn a request for something to watch, play or listen to into JSON. Reply with JSON only, in this shape: \
{{\"media\": [\"movie\"|\"tv\"|\"game\"|\"album\"], \"tags\": [...], \"genres\": [...], \"short\": true|false, \"like\": [titles the request compares to]}}. \
\"tags\" must come only from this list, choosing the 3 to 6 that best fit the mood and themes: {}. \
\"genres\" are plain genre names such as sci-fi, horror, rpg, ambient, jazz. Leave \"media\" empty if the request doesn't say.",
        tags.join(", ")
    );
    let raw = ai::generate(ctx, &base, &model, &system, text, true).await.ok()?;
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let strings = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    let mut features: Vec<String> = vec![];
    for t in strings("tags") {
        let dashed = t.trim().to_lowercase().replace(' ', "-");
        if tags.contains(&dashed.as_str()) {
            features.push(format!("t:{dashed}"));
        } else {
            features.extend(vocab::map_term(&t).into_iter().map(String::from));
        }
    }
    for g in strings("genres") {
        features.extend(vocab::map_term(&g).into_iter().map(String::from));
    }
    features.dedup();
    let q = VibeQuery { media: strings("media").iter().filter_map(|m| Media::parse(m)).filter(|m| *m != Media::Artist).collect(), features, short: v["short"] == true, like: strings("like") };
    (!q.features.is_empty() || !q.like.is_empty()).then_some(q)
}

#[derive(Debug, Serialize)]
pub struct VibeResult {
    pub cards: Vec<Card>,
    /// "ollama" or "tags": shown under the search box so it's clear which engine answered.
    pub engine: &'static str,
    pub understood: Vec<String>,
    pub media: Vec<Media>,
    pub like: Vec<String>,
}

/// Looks up a reference title in the local library, then the catalogs, and returns its features.
async fn reference_features(ctx: &Ctx, title: &str) -> Vec<(String, f64)> {
    let local = {
        let conn = ctx.db.lock();
        conn.query_row("SELECT id FROM items WHERE sort_title = ?1 AND media != 'artist' ORDER BY detail_level DESC, popularity DESC LIMIT 1", [sort_title(title)], |r| r.get::<_, i64>(0)).ok()
    };
    let id = match local {
        Some(id) => Some(id),
        None => catalog::search_everything(ctx, title, None, 3).await.ok().and_then(|ids| ids.first().copied()),
    };
    let Some(id) = id else { return vec![] };
    let _ = catalog::ensure_detail(ctx, id).await;
    let conn = ctx.db.lock();
    store::item_features(&conn, id).unwrap_or_default().into_iter().filter(|(f, _)| f.starts_with("t:") || f.starts_with("g:")).collect()
}

pub async fn search(ctx: &Ctx, text: &str, limit: usize) -> Result<VibeResult> {
    let text = text.trim();
    let (q, engine) = match parse_llm(ctx, text).await {
        Some(q) => (q, "ollama"),
        None => (parse_local(text), "tags"),
    };
    let mut wanted: HashMap<String, f64> = q.features.iter().map(|f| (f.clone(), 1.0)).collect();
    for title in &q.like {
        for (f, w) in reference_features(ctx, title).await {
            // A reference title's genre is less important than its mood when crossing media.
            let weight = if f.starts_with("t:") { 0.8 * w } else { 0.35 * w };
            let e = wanted.entry(f).or_insert(0.0);
            *e = e.max(weight);
        }
    }
    if q.short {
        wanted.insert("n:short".into(), 0.8);
        wanted.entry("t:short".into()).or_insert(0.6);
    }

    // For music, make sure the catalog actually holds albums carrying these tags.
    if (q.media.is_empty() || q.media.contains(&Media::Album)) && ctx.has_key("lastfm_key") {
        let mut top: Vec<(&String, &f64)> = wanted.iter().filter(|(f, _)| f.starts_with("t:") || f.starts_with("g:")).collect();
        top.sort_by(|a, b| b.1.total_cmp(a.1));
        for (f, _) in top.into_iter().take(2) {
            let tag = f[2..].replace('-', " ");
            let _ = catalog::albums_for_tag(ctx, &tag, 15).await;
        }
    }

    let understood: Vec<String> = {
        let mut fs: Vec<(&String, &f64)> = wanted.iter().collect();
        fs.sort_by(|a, b| b.1.total_cmp(a.1));
        fs.into_iter().take(8).map(|(f, _)| vocab::label(f)).collect()
    };
    if wanted.is_empty() {
        return Ok(VibeResult { cards: vec![], engine, understood, media: q.media, like: q.like });
    }

    let conn = ctx.db.lock();
    let marks = vec!["?"; wanted.len()].join(",");
    let keys: Vec<&String> = wanted.keys().collect();
    let sql = format!(
        "SELECT f.item_id, f.feature, f.weight, i.media, COALESCE(s.score, 0) FROM item_features f
         JOIN items i ON i.id = f.item_id LEFT JOIN scores s ON s.item_id = i.id
         WHERE f.feature IN ({marks}) AND i.media != 'artist'"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut hits: HashMap<i64, (f64, usize, String, f64)> = HashMap::new();
    for row in stmt.query_map(rusqlite::params_from_iter(keys.iter()), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, String>(3)?, r.get::<_, f64>(4)?)))? {
        let (id, feature, weight, media, taste) = row?;
        let e = hits.entry(id).or_insert((0.0, 0, media, taste));
        e.0 += weight * wanted.get(&feature).copied().unwrap_or(0.0);
        e.1 += 1;
    }
    let want_media: Vec<&str> = q.media.iter().map(|m| m.as_str()).collect();
    let n_wanted = wanted.keys().filter(|f| !f.starts_with("n:")).count().max(1) as f64;
    let mut ranked: Vec<(i64, f64)> = hits
        .into_iter()
        .filter(|(_, (_, _, media, _))| want_media.is_empty() || want_media.contains(&media.as_str()))
        // Matching several of the requested tags beats matching one strongly; taste breaks ties.
        .map(|(id, (sum, matched, _, taste))| (id, sum * (0.5 + matched as f64 / n_wanted) + taste * 0.5))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let ids: Vec<i64> = ranked.into_iter().take(limit).map(|r| r.0).collect();
    let cards = store::cards_by_ids(&conn, &ids)?;
    Ok(VibeResult { cards, engine, understood, media: q.media, like: q.like })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn the_local_parser_reads_media_mood_and_length() {
        let q = parse_local("a short cozy game for a rainy Sunday");
        assert_eq!(q.media, vec![Media::Game]);
        assert!(q.short);
        assert!(q.features.contains(&"t:cozy".to_string()));
        assert!(q.like.is_empty());
    }

    #[test]
    fn like_phrases_become_reference_titles_not_tags() {
        let q = parse_local("albums that sound like Blade Runner");
        assert_eq!(q.media, vec![Media::Album]);
        assert_eq!(q.like, vec!["Blade Runner"]);
        let q = parse_local("something similar to Dark Souls but cozy");
        assert_eq!(q.like, vec!["Dark Souls"]);
        assert!(q.features.contains(&"t:cozy".to_string()));
        assert!(!q.features.contains(&"t:dark".to_string()), "the reference title isn't mined for tags");
    }

    fn seed(t: &crate::ctx::testing::TestCtx) -> (i64, i64, i64) {
        let conn = t.ctx.db.lock();
        let mut cozy = ItemDraft::new(Media::Game, "Turnip Hollow").id("test", 1);
        cozy.tags = vec!["cozy".into(), "farming sim".into()];
        cozy.runtime = Some(5 * 60);
        cozy.detail_level = 1;
        let mut synth = ItemDraft::new(Media::Album, "Neon Tides").id("test", 2);
        synth.tags = vec!["synthwave".into(), "dark ambient".into()];
        synth.detail_level = 1;
        let mut br = ItemDraft::new(Media::Movie, "Blade Runner").id("test", 3);
        br.genres = vec!["Science Fiction".into()];
        br.tags = vec!["cyberpunk".into(), "neo noir".into(), "dystopia".into()];
        br.detail_level = 1;
        let mut farm_film = ItemDraft::new(Media::Movie, "A Quiet Harvest").id("test", 4);
        farm_film.tags = vec!["cozy".into()];
        farm_film.detail_level = 1;
        store::upsert_item(&conn, &farm_film).unwrap();
        (store::upsert_item(&conn, &cozy).unwrap(), store::upsert_item(&conn, &synth).unwrap(), store::upsert_item(&conn, &br).unwrap())
    }

    #[tokio::test]
    async fn without_ollama_search_runs_on_tags() {
        let t = ctx();
        let (cozy, _, _) = seed(&t);
        t.net.on("localhost:11434", 500, "");
        t.net.set_offline(true);
        let r = search(&t.ctx, "a short cozy game for a rainy Sunday", 10).await.unwrap();
        assert_eq!(r.engine, "tags");
        assert_eq!(r.cards.len(), 1, "only games, so the cozy film is left out");
        assert_eq!(r.cards[0].id, cozy);
        assert!(r.understood.iter().any(|u| u.contains("cozy")));
    }

    #[tokio::test]
    async fn a_reference_title_pulls_matches_from_another_medium() {
        let t = ctx();
        let (_, synth, _) = seed(&t);
        t.net.set_offline(true);
        let r = search(&t.ctx, "albums that sound like Blade Runner", 10).await.unwrap();
        assert_eq!(r.like, vec!["Blade Runner"]);
        assert_eq!(r.cards.first().map(|c| c.id), Some(synth), "the film's cyberpunk mood finds the synthwave album");
    }

    #[tokio::test]
    async fn with_ollama_the_model_picks_the_tags() {
        let t = ctx();
        let (cozy, _, _) = seed(&t);
        t.net.on("localhost:11434/api/tags", 200, r#"{"models":[{"name":"llama3.2:3b","details":{"family":"llama"}}]}"#);
        t.net.on("localhost:11434/api/generate", 200, r#"{"response":"{\"media\":[\"game\"],\"tags\":[\"cozy\",\"relaxing\",\"not-a-real-tag\"],\"genres\":[\"simulation\"],\"short\":true,\"like\":[]}","done":true}"#);
        let r = search(&t.ctx, "something gentle to unwind with after work", 10).await.unwrap();
        assert_eq!(r.engine, "ollama");
        assert_eq!(r.cards[0].id, cozy);
        let sent: Value = serde_json::from_slice(&t.net.last("api/generate").unwrap().body.unwrap()).unwrap();
        assert_eq!(sent["format"], "json");
        assert_eq!(sent["stream"], false);
    }

    #[tokio::test]
    async fn a_garbled_model_answer_falls_back_to_tags() {
        let t = ctx();
        seed(&t);
        t.net.on("localhost:11434/api/tags", 200, r#"{"models":[{"name":"llama3.2:3b","details":{"family":"llama"}}]}"#);
        t.net.on("localhost:11434/api/generate", 200, r#"{"response":"Sure! Here are some ideas...","done":true}"#);
        let r = search(&t.ctx, "cozy games", 10).await.unwrap();
        assert_eq!(r.engine, "tags");
        assert!(!r.cards.is_empty());
    }
}
