//! Candidate scoring, "why" reasons and diversity re-ranking.

use super::profile::{self, Profile};
use super::{kind_multiplier, vocab, TasteSettings};
use crate::db::Db;
use crate::error::Result;
use crate::model::*;
use rusqlite::params;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Cosine-style similarity between the profile and one item's features.
/// Positive means "like things you like"; negative means "like things you panned".
pub fn similarity(profile: &Profile, features: &[(String, f64)], dial: f64) -> f64 {
    let mut dot = 0.0;
    let mut norm = 0.0;
    for (f, w) in features {
        let x = w * kind_multiplier(f, dial);
        norm += x * x;
        if let Some(p) = profile.weights.get(f) {
            dot += p * x;
        }
    }
    if norm <= 0.0 {
        return 0.0;
    }
    // Dividing by the plain norm would let a title with a single matching tag score a perfect 1.
    // The +1 asks for a few features of evidence before a title can score highly.
    dot / (norm + 1.0).sqrt()
}

/// Maps a raw score onto 1..=99, calibrated so one of the user's own favourites lands near 95.
pub fn match_pct(score: f64, reference: f64) -> i64 {
    let r = reference.max(0.05);
    (50.0 + 49.0 * (1.5 * score / r).tanh()).round().clamp(1.0, 99.0) as i64
}

/// The user's history, indexed for writing reasons.
struct Evidence {
    /// feature -> strongest (contribution, item id) pairs
    by_feature: HashMap<String, Vec<(f64, i64)>>,
    /// item id -> how to describe what the user did with it
    phrase: HashMap<i64, (String, Media)>,
    creator_names: HashMap<String, String>,
}

fn stars(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}★", v as i64)
    } else {
        format!("{}½★", v.floor() as i64)
    }
}

fn phrase_for(title: &str, media: Media, events: &[&profile::Event]) -> Option<String> {
    let find = |k: EventKind| events.iter().find(|e| e.kind == k);
    let t = if media == Media::Artist { title.to_string() } else { format!("*{title}*") };
    if let Some(e) = find(EventKind::Rating).filter(|e| e.value >= 3.5) {
        return Some(format!("rated {t} {}", stars(e.value)));
    }
    if find(EventKind::Pin).is_some() {
        return Some(format!("pinned {t}"));
    }
    if find(EventKind::MoreLike).is_some() {
        return Some(format!("asked for more like {t}"));
    }
    if find(EventKind::Like).is_some() {
        return Some(format!("love {t}"));
    }
    if let Some(e) = find(EventKind::Play) {
        return Some(match media {
            Media::Game if e.value >= 20.0 * 60.0 => format!("play a lot of {t}"),
            Media::Game => format!("played {t}"),
            Media::Artist | Media::Album if e.value >= 50.0 => format!("listen to {t} a lot"),
            Media::Artist | Media::Album => format!("listen to {t}"),
            _ => format!("watched {t}"),
        });
    }
    if find(EventKind::Watched).is_some() {
        return Some(format!("{} {t}", if matches!(media, Media::Album | Media::Artist) { "listened to" } else if media == Media::Game { "played" } else { "watched" }));
    }
    if find(EventKind::Wishlist).is_some() {
        return Some(format!("saved {t}"));
    }
    if find(EventKind::Owned).is_some() {
        return Some(format!("own {t}"));
    }
    None
}

fn build_evidence(db: &Db, settings: &TasteSettings) -> Result<Evidence> {
    let events = profile::load_events(db)?;
    let now = chrono::Utc::now();
    let mut by_item: HashMap<i64, Vec<&profile::Event>> = HashMap::new();
    for e in &events {
        by_item.entry(e.item_id).or_default().push(e);
    }
    let conn = db.lock();
    let mut ev = Evidence { by_feature: HashMap::new(), phrase: HashMap::new(), creator_names: HashMap::new() };
    let mut title_stmt = conn.prepare_cached("SELECT title, data FROM items WHERE id = ?1")?;
    let mut feat_stmt = conn.prepare_cached("SELECT feature, weight FROM item_features WHERE item_id = ?1")?;
    for (id, evs) in &by_item {
        let a = profile::item_affinity(evs, settings, now);
        if a < 0.5 {
            continue;
        }
        let (title, data): (String, String) = title_stmt.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let media = evs[0].media;
        let Some(phrase) = phrase_for(&title, media, evs) else { continue };
        ev.phrase.insert(*id, (phrase, media));
        let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
        for c in data["creators"].as_array().into_iter().flatten() {
            if let Some(name) = c["name"].as_str() {
                ev.creator_names.entry(format!("c:{}", slug(name))).or_insert_with(|| name.to_string());
            }
        }
        for row in feat_stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (f, w) = row?;
            // Era, language and length are too generic to be a reason.
            if matches!(f.as_bytes().first(), Some(b'e' | b'l' | b'n')) {
                continue;
            }
            let list = ev.by_feature.entry(f).or_default();
            list.push((a * w, *id));
        }
    }
    for list in ev.by_feature.values_mut() {
        list.sort_by(|a, b| b.0.total_cmp(&a.0));
        list.truncate(4);
    }
    Ok(ev)
}

fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        many => format!("{}, and {}", many[..many.len() - 1].join(", "), many[many.len() - 1]),
    }
}

/// One or two short reasons, citing the user's actual history.
fn reasons(item_id: i64, media: Media, features: &[(String, f64)], profile: &Profile, ev: &Evidence, dial: f64, creators: &HashMap<String, String>) -> Vec<String> {
    let mut contrib: Vec<(f64, &String)> = features
        .iter()
        .filter_map(|(f, w)| profile.weights.get(f).map(|p| (p * w * kind_multiplier(f, dial), f)))
        .filter(|(c, f)| *c > 0.02 && !matches!(f.as_bytes().first(), Some(b'e' | b'l' | b'n')))
        .collect();
    contrib.sort_by(|a, b| b.0.total_cmp(&a.0));
    if contrib.is_empty() {
        return vec![];
    }
    let mut out = vec![];

    // Reason 1: things the user did that share this title's strongest features.
    // Prefer evidence from a different medium: that's the cross-media signature.
    let mut picked: Vec<(i64, f64, bool)> = vec![];
    for (c, f) in contrib.iter().take(6) {
        for (strength, id) in ev.by_feature.get(*f).into_iter().flatten() {
            if *id == item_id || picked.iter().any(|p| p.0 == *id) {
                continue;
            }
            let Some((_, m)) = ev.phrase.get(id) else { continue };
            let other_medium = *m != media && !(matches!(m, Media::Artist) && media == Media::Album);
            picked.push((*id, c * strength * if other_medium { 1.35 } else { 1.0 }, other_medium));
        }
    }
    picked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut cited: Vec<String> = vec![];
    let mut cited_media: HashSet<Media> = HashSet::new();
    for (id, _, _) in &picked {
        let (phrase, m) = &ev.phrase[id];
        // Two examples at most, from different media where possible.
        if cited.len() == 1 && cited_media.contains(m) && picked.iter().any(|p| !cited_media.contains(&ev.phrase[&p.0].1)) {
            continue;
        }
        cited.push(phrase.clone());
        cited_media.insert(*m);
        if cited.len() == 2 {
            break;
        }
    }
    if !cited.is_empty() {
        out.push(format!("Because you {}", join_and(&cited)));
    }

    // Reason 2: the features themselves, in words.
    let top_creator = contrib.iter().find(|(_, f)| f.starts_with("c:")).filter(|(c, _)| *c >= contrib[0].0 * 0.5);
    if let Some((_, f)) = top_creator {
        if let Some(name) = creators.get(*f).or_else(|| ev.creator_names.get(*f)) {
            let n = profile.support.get(*f).copied().unwrap_or(0);
            out.push(if n >= 2 { format!("More from {name}, behind {n} things you like") } else { format!("More from {name}") });
        }
    }
    if out.len() < 2 {
        let labels: Vec<String> = contrib.iter().filter(|(_, f)| f.starts_with("t:") || f.starts_with("g:")).take(2).map(|(_, f)| vocab::label(f)).collect();
        if !labels.is_empty() {
            out.push(format!("Matches your taste for {}", join_and(&labels)));
        }
    }
    out.truncate(2);
    out
}

/// Scores every title that could appear in a feed and stores the results.
pub fn score_all(db: &Db) -> Result<usize> {
    let settings = TasteSettings::load(db);
    let profile = Profile::load(db)?;
    let evidence = build_evidence(db, &settings)?;
    let affinity = profile::affinities(db, &settings)?;
    let blocked: HashSet<&str> = profile.blocked.iter().map(String::as_str).collect();
    // When local embeddings exist (Ollama), they refine the tag-based score.
    let embed_bonus = crate::ai::embedding_bonus(db, &affinity).unwrap_or_default();
    let today = today();
    let now_ts = now();

    let conn = db.lock();
    // Titles similar to things the user loves get a boost.
    let mut similar_boost: HashMap<i64, f64> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT item_id, similar_id FROM item_similar")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (src, sim) = row?;
            if let Some(a) = affinity.get(&src).filter(|a| **a > 1.0) {
                let b = similar_boost.entry(sim).or_insert(0.0);
                *b = (*b + a / 3.0).min(2.0);
            }
        }
    }
    // Popularity percentile within each medium, for the hype boost.
    let mut pop_rank: HashMap<i64, f64> = HashMap::new();
    for media in Media::FEED {
        let mut stmt = conn.prepare("SELECT id FROM items WHERE media = ?1 ORDER BY popularity")?;
        let ids: Vec<i64> = stmt.query_map([media.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let n = ids.len().max(1) as f64;
        for (i, id) in ids.into_iter().enumerate() {
            pop_rank.insert(id, (i as f64 + 1.0) / n);
        }
    }
    let dismissed: HashSet<i64> = {
        let mut stmt = conn.prepare("SELECT item_id FROM user_events WHERE kind = 'dismiss'")?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        rows
    };

    let mut items: Vec<(i64, Media, Option<String>, String)> = vec![];
    {
        let mut stmt = conn.prepare("SELECT id, media, release_date, data FROM items WHERE media != 'artist'")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?)))? {
            let (id, media, date, data) = row?;
            if let Some(m) = Media::parse(&media) {
                items.push((id, m, date, data));
            }
        }
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM scores", [])?;
    let mut count = 0;
    {
        let mut feat_stmt = tx.prepare_cached("SELECT feature, weight FROM item_features WHERE item_id = ?1")?;
        let mut ins = tx.prepare_cached("INSERT INTO scores (item_id, score, match_pct, why, computed_at) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for (id, media, date, data) in &items {
            let feats: Vec<(String, f64)> = feat_stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            if dismissed.contains(id) || feats.iter().any(|(f, _)| blocked.contains(f.as_str())) {
                continue; // no score row = never recommended
            }
            let mut score = similarity(&profile, &feats, settings.dial);
            let r = profile.reference;
            if let Some(b) = similar_boost.get(id) {
                score += 0.12 * r * b;
            }
            if let Some(z) = embed_bonus.get(id) {
                score += 0.12 * r * z;
            }
            // New and about-to-release titles get a nudge, scaled by how anticipated they are.
            if let Some(d) = date {
                let days = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().zip(chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").ok()).map(|(a, b)| (a - b).num_days());
                if let Some(days) = days.filter(|d| (-21..=60).contains(d)) {
                    let closeness = 1.0 - (days.abs() as f64 / 60.0);
                    score += 0.08 * r * closeness * pop_rank.get(id).copied().unwrap_or(0.5);
                }
            }
            // Adventurous: reward titles that match on something but bring tags you haven't explored.
            if settings.dial > 0.0 && score > 0.0 {
                let tags: Vec<&(String, f64)> = feats.iter().filter(|(f, _)| f.starts_with("t:")).collect();
                if !tags.is_empty() {
                    let unexplored = tags.iter().filter(|(f, _)| profile.support.get(f).copied().unwrap_or(0) == 0).count() as f64 / tags.len() as f64;
                    score += settings.dial * 0.10 * r * unexplored;
                }
            }
            let creators: HashMap<String, String> = serde_json::from_str::<Value>(data)
                .ok()
                .and_then(|d| d["creators"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|c| c["name"].as_str().map(|n| (format!("c:{}", slug(n)), n.to_string())))
                .collect();
            let why = if profile.is_empty() { vec![] } else { reasons(*id, *media, &feats, &profile, &evidence, settings.dial, &creators) };
            let pct = if profile.is_empty() || feats.is_empty() { None } else { Some(match_pct(score, r)) };
            ins.execute(params![id, score, pct.unwrap_or(-1), serde_json::to_string(&why)?, now_ts])?;
            count += 1;
        }
    }
    tx.commit()?;
    Ok(count)
}

/// Scores one item on demand (the detail page, after a fresh fetch).
pub fn score_item(db: &Db, item_id: i64) -> Result<(i64, Vec<String>)> {
    let settings = TasteSettings::load(db);
    let profile = Profile::load(db)?;
    if profile.is_empty() {
        return Ok((-1, vec![]));
    }
    let evidence = build_evidence(db, &settings)?;
    let conn = db.lock();
    let feats = crate::store::item_features(&conn, item_id)?;
    let item = crate::store::get_item(&conn, item_id)?;
    let creators: HashMap<String, String> = item.data["creators"].as_array().cloned().unwrap_or_default().iter().filter_map(|c| c["name"].as_str().map(|n| (format!("c:{}", slug(n)), n.to_string()))).collect();
    let score = similarity(&profile, &feats, settings.dial);
    let pct = match_pct(score, profile.reference);
    let why = reasons(item_id, item.media, &feats, &profile, &evidence, settings.dial, &creators);
    conn.execute(
        "INSERT INTO scores (item_id, score, match_pct, why, computed_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (item_id) DO UPDATE SET score = max(score, excluded.score), match_pct = max(match_pct, excluded.match_pct), why = excluded.why, computed_at = excluded.computed_at",
        params![item_id, score, pct, serde_json::to_string(&why)?, now()],
    )?;
    Ok((pct, why))
}

/// What `diversify` needs to know about a candidate.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub score: f64,
    pub media: String,
    pub features: HashSet<String>,
}

fn overlap(a: &Candidate, b: &Candidate) -> f64 {
    if a.features.is_empty() || b.features.is_empty() {
        return if a.media == b.media { 0.2 } else { 0.0 };
    }
    let inter = a.features.intersection(&b.features).count() as f64;
    let union = (a.features.len() + b.features.len()) as f64 - inter;
    let shared_creator = a.features.iter().any(|f| f.starts_with("c:") && b.features.contains(f));
    (inter / union + if a.media == b.media { 0.15 } else { 0.0 } + if shared_creator { 0.3 } else { 0.0 }).min(1.0)
}

/// Maximal-marginal-relevance re-ranking: each pick is the best remaining title after
/// subtracting how much it repeats the last few picks. `lambda` is how hard to penalise repeats.
pub fn diversify(mut pool: Vec<Candidate>, lambda: f64, take: usize) -> Vec<i64> {
    pool.sort_by(|a, b| b.score.total_cmp(&a.score));
    // Scores are compared relative to the best one, so the penalty means the same thing in any list.
    let top = pool.first().map(|c| c.score).unwrap_or(0.0);
    let scale = if top > 0.0 { top } else { 1.0 };
    let mut picked: Vec<Candidate> = vec![];
    while !pool.is_empty() && picked.len() < take {
        let window = &picked[picked.len().saturating_sub(6)..];
        let (best, _) = pool
            .iter()
            .enumerate()
            .take(60) // only the head of the pool can win, which keeps this fast
            .map(|(i, c)| {
                let repeat = window.iter().map(|p| overlap(c, p)).fold(0.0, f64::max);
                (i, c.score / scale - lambda * repeat)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        picked.push(pool.remove(best));
    }
    picked.into_iter().map(|c| c.id).collect()
}

/// Loads candidates (with their genre/tag/creator features) for a set of item ids.
pub fn candidates(conn: &rusqlite::Connection, rows: &[(i64, f64, String)]) -> Result<Vec<Candidate>> {
    let mut stmt = conn.prepare_cached("SELECT feature FROM item_features WHERE item_id = ?1 AND (feature LIKE 'g:%' OR feature LIKE 't:%' OR feature LIKE 'c:%')")?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, score, media) in rows {
        let features = stmt.query_map([id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<HashSet<_>>>()?;
        out.push(Candidate { id: *id, score: *score, media: media.clone(), features });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{record_event, set_setting, upsert_item};
    use crate::taste::profile::rebuild;

    struct World {
        db: Db,
    }

    fn add(db: &Db, media: Media, title: &str, genres: &[&str], tags: &[&str], creator: (&str, &str)) -> i64 {
        let mut d = ItemDraft::new(media, title).id("test", title);
        d.genres = genres.iter().map(|s| s.to_string()).collect();
        d.tags = tags.iter().map(|s| s.to_string()).collect();
        if !creator.0.is_empty() {
            d.creators = vec![Creator::new(creator.0, creator.1)];
            if media == Media::Album {
                d.set("artist", creator.0);
            }
        }
        d.release_date = Some("2020-01-01".into());
        upsert_item(&db.lock(), &d).unwrap()
    }

    fn rate(db: &Db, id: i64, stars: f64) {
        record_event(&db.lock(), id, "marquee", EventKind::Rating, stars, None, None, &Value::Null).unwrap();
    }

    /// A moody sci-fi fan: loves Arrival and Blade Runner, plays a lot of Outer Wilds,
    /// listens to Tycho, and panned a superhero film.
    fn world() -> (World, HashMap<&'static str, i64>) {
        let db = Db::memory().unwrap();
        let mut ids = HashMap::new();
        let arrival = add(&db, Media::Movie, "Arrival", &["Science Fiction", "Drama"], &["alien contact", "slow burn", "melancholy", "space"], ("Denis Villeneuve", "director"));
        let br = add(&db, Media::Movie, "Blade Runner 2049", &["Science Fiction"], &["cyberpunk", "neo noir", "slow burn", "dystopia"], ("Denis Villeneuve", "director"));
        let ow = add(&db, Media::Game, "Outer Wilds", &["Adventure", "Indie"], &["space", "exploration", "mystery", "atmospheric"], ("Mobius Digital", "developer"));
        let tycho = add(&db, Media::Artist, "Tycho", &[], &["ambient", "chillwave", "synthwave"], ("Tycho", "artist"));
        let cape = add(&db, Media::Movie, "Cape Punch 4", &["Action"], &["superhero"], ("Loud Studio", "studio"));
        rate(&db, arrival, 5.0);
        rate(&db, br, 4.5);
        rate(&db, cape, 1.0);
        record_event(&db.lock(), ow, "steam", EventKind::Play, 60.0 * 45.0, None, None, &Value::Null).unwrap();
        record_event(&db.lock(), tycho, "lastfm", EventKind::Play, 1800.0, None, None, &Value::Null).unwrap();
        // Candidates.
        ids.insert("dune", add(&db, Media::Movie, "Dune: Part Three", &["Science Fiction", "Adventure"], &["space", "epic", "slow burn"], ("Denis Villeneuve", "director")));
        ids.insert("scifi_game", add(&db, Media::Game, "Signal Deep", &["Adventure"], &["space", "atmospheric", "mystery", "melancholy"], ("Tiny Team", "developer")));
        ids.insert("synth_album", add(&db, Media::Album, "Neon Tides", &[], &["synthwave", "ambient", "melancholic"], ("Lumen Drift", "artist")));
        ids.insert("cape5", add(&db, Media::Movie, "Cape Punch 5", &["Action"], &["superhero"], ("Loud Studio", "studio")));
        ids.insert("romcom", add(&db, Media::Movie, "Meet Cute", &["Comedy", "Romance"], &["romantic comedy"], ("Someone Else", "director")));
        ids.insert("cozy_farm", add(&db, Media::Game, "Turnip Hollow", &["Simulator"], &["farming sim", "cozy"], ("Soft Games", "developer")));
        for n in 0..30 {
            add(&db, Media::Movie, &format!("Filler {n}"), &["Drama"], &["family"], ("", ""));
        }
        rebuild(&db).unwrap();
        score_all(&db).unwrap();
        (World { db }, ids)
    }

    fn row(db: &Db, id: i64) -> (f64, i64, Vec<String>) {
        db.lock()
            .query_row("SELECT score, match_pct, why FROM scores WHERE item_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&r.get::<_, String>(2)?).unwrap())))
            .unwrap()
    }

    #[test]
    fn taste_in_film_recommends_games_and_albums() {
        let (w, ids) = world();
        let game = row(&w.db, ids["scifi_game"]);
        let album = row(&w.db, ids["synth_album"]);
        let romcom = row(&w.db, ids["romcom"]);
        let farm = row(&w.db, ids["cozy_farm"]);
        assert!(game.1 >= 75, "moody space game should match: {game:?}");
        assert!(album.1 >= 70, "atmospheric synth album should match: {album:?}");
        assert!(game.0 > romcom.0 && album.0 > romcom.0);
        assert!(game.0 > farm.0);
        assert!(romcom.1 <= 55, "{romcom:?}");
    }

    #[test]
    fn more_of_what_you_panned_scores_below_neutral() {
        let (w, ids) = world();
        let cape = row(&w.db, ids["cape5"]);
        assert!(cape.0 < 0.0);
        assert!(cape.1 < 40, "{cape:?}");
        assert!(cape.2.is_empty(), "no positive reason is invented for it");
    }

    #[test]
    fn every_pick_says_why_using_real_history() {
        let (w, ids) = world();
        let dune = row(&w.db, ids["dune"]);
        assert!(dune.1 >= 90, "{dune:?}");
        assert!((1..=2).contains(&dune.2.len()));
        assert!(dune.2[0].starts_with("Because you "), "{:?}", dune.2);
        assert!(dune.2[0].contains("*Arrival* 5★") || dune.2[0].contains("*Blade Runner 2049* 4½★"), "{:?}", dune.2);
        assert!(dune.2.iter().any(|r| r.contains("Denis Villeneuve")), "{:?}", dune.2);

        // The album's reason reaches across media.
        let album = row(&w.db, ids["synth_album"]);
        assert!(album.2[0].contains("Tycho") || album.2[0].contains("*Arrival*"), "{:?}", album.2);
        // The game cites the film and the game the user actually plays.
        let game = row(&w.db, ids["scifi_game"]);
        assert!(game.2[0].contains("play a lot of *Outer Wilds*") || game.2[0].contains("*Arrival*"), "{:?}", game.2);
    }

    #[test]
    fn the_dial_trades_safe_bets_for_discovery() {
        let (w, ids) = world();
        let comfort_gap = {
            set_setting(&w.db, "taste", &serde_json::json!({ "dial": 0.0 })).unwrap();
            rebuild(&w.db).unwrap();
            score_all(&w.db).unwrap();
            row(&w.db, ids["dune"]).0 - row(&w.db, ids["scifi_game"]).0
        };
        let adventurous_gap = {
            set_setting(&w.db, "taste", &serde_json::json!({ "dial": 1.0 })).unwrap();
            rebuild(&w.db).unwrap();
            score_all(&w.db).unwrap();
            row(&w.db, ids["dune"]).0 - row(&w.db, ids["scifi_game"]).0
        };
        assert!(comfort_gap > adventurous_gap, "the known director matters less when adventurous: {comfort_gap} vs {adventurous_gap}");
    }

    #[test]
    fn blocked_and_dismissed_titles_are_never_scored() {
        let (w, ids) = world();
        w.db.lock().execute("INSERT INTO taste_overrides (feature, mode) VALUES ('t:space', 'block')", []).unwrap();
        record_event(&w.db.lock(), ids["synth_album"], "marquee", EventKind::Dismiss, 1.0, None, None, &serde_json::json!({ "reason": "genre" })).unwrap();
        rebuild(&w.db).unwrap();
        score_all(&w.db).unwrap();
        let has = |id: i64| w.db.lock().query_row("SELECT count(*) FROM scores WHERE item_id = ?1", [id], |r| r.get::<_, i64>(0)).unwrap() == 1;
        assert!(!has(ids["dune"]) && !has(ids["scifi_game"]), "blocked tag");
        assert!(!has(ids["synth_album"]), "dismissed");
        assert!(has(ids["romcom"]));
    }

    #[test]
    fn not_interested_lowers_similar_titles() {
        let (w, ids) = world();
        let before = row(&w.db, ids["cozy_farm"]).0;
        let other = add(&w.db, Media::Game, "Carrot Creek", &["Simulator"], &["farming sim", "cozy"], ("Soft Games", "developer"));
        record_event(&w.db.lock(), other, "marquee", EventKind::Dismiss, 1.0, None, None, &serde_json::json!({ "reason": "genre" })).unwrap();
        rebuild(&w.db).unwrap();
        score_all(&w.db).unwrap();
        assert!(row(&w.db, ids["cozy_farm"]).0 < before);
    }

    #[test]
    fn match_percent_is_calibrated_and_bounded() {
        assert_eq!(match_pct(0.0, 0.3), 50);
        assert!(match_pct(0.3, 0.3) >= 93);
        assert!(match_pct(-0.3, 0.3) <= 7);
        assert_eq!(match_pct(100.0, 0.3), 99);
        assert_eq!(match_pct(-100.0, 0.3), 1);
    }

    #[test]
    fn a_single_matching_tag_is_not_a_perfect_score() {
        let mut p = Profile::default();
        p.weights.insert("t:space".into(), 1.0);
        p.weights.insert("t:slow-burn".into(), 1.0);
        p.weights.insert("g:sci-fi".into(), 1.0);
        let thin = similarity(&p, &[("t:space".into(), 1.0)], 0.35);
        let rich = similarity(&p, &[("t:space".into(), 1.0), ("t:slow-burn".into(), 1.0), ("g:sci-fi".into(), 1.0)], 0.35);
        assert!(rich > thin * 1.4, "{rich} vs {thin}");
    }

    fn cand(id: i64, score: f64, media: &str, feats: &[&str]) -> Candidate {
        Candidate { id, score, media: media.into(), features: feats.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn diversity_breaks_up_ten_copies_of_the_same_thing() {
        let mut pool: Vec<Candidate> = (0..10).map(|i| cand(i, 1.0 - i as f64 * 0.01, "movie", &["g:sci-fi", "t:space", "c:same-director"])).collect();
        pool.push(cand(100, 0.80, "game", &["g:adventure", "t:mystery"]));
        pool.push(cand(101, 0.78, "album", &["g:ambient", "t:dreamy"]));
        let plain: Vec<i64> = diversify(pool.clone(), 0.0, 6);
        assert_eq!(plain, vec![0, 1, 2, 3, 4, 5], "with no penalty it is just the top scores");
        let mixed = diversify(pool, 0.6, 6);
        assert_eq!(mixed[0], 0, "the best title still leads");
        assert!(mixed.contains(&100) && mixed.contains(&101), "{mixed:?}");
        assert!(mixed.iter().position(|i| *i == 100).unwrap() <= 2);
    }

    #[test]
    fn with_no_history_nothing_claims_to_match() {
        let db = Db::memory().unwrap();
        let id = add(&db, Media::Movie, "Anything", &["Drama"], &[], ("", ""));
        rebuild(&db).unwrap();
        score_all(&db).unwrap();
        let (_, pct, why) = row(&db, id);
        assert_eq!(pct, -1, "-1 means there is nothing to base a match on, and the UI shows no ring");
        assert!(why.is_empty());
    }
}
