//! Builds the unified taste profile from user events.

use super::TasteSettings;
use crate::db::Db;
use crate::error::Result;
use crate::model::*;
use rusqlite::params;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// One stored event, with the item's media type.
#[derive(Debug, Clone)]
pub struct Event {
    pub item_id: i64,
    pub media: Media,
    pub source: String,
    pub kind: EventKind,
    pub value: f64,
    pub at: String,
    pub first_at: Option<String>,
    pub meta: Value,
}

fn days_between(earlier: &str, later: chrono::DateTime<chrono::Utc>) -> f64 {
    chrono::DateTime::parse_from_rfc3339(earlier)
        .map(|d| (later - d.with_timezone(&chrono::Utc)).num_hours() as f64 / 24.0)
        .or_else(|_| chrono::NaiveDate::parse_from_str(earlier.get(..10).unwrap_or(""), "%Y-%m-%d").map(|d| (later.date_naive() - d).num_days() as f64))
        .unwrap_or(365.0)
        .max(0.0)
}

/// How strongly one event speaks, before recency. The tiers follow the order
/// explicit rating > like/love > playtime/scrobbles > watched/owned > wishlisted.
pub fn event_strength(kind: EventKind, value: f64, media: Media, meta: &Value) -> f64 {
    match kind {
        // 5★ = +3.0, 4★ = +1.67, 3★ = +0.33, 2★ = -1.0, 1★ = -2.33, ½★ = -3.0
        EventKind::Rating => ((value - 2.75) * 4.0 / 3.0).clamp(-3.0, 3.0),
        EventKind::Pin => 3.0,
        EventKind::Like | EventKind::MoreLike => 2.0,
        EventKind::Play => {
            // Log scale: the first hours say more than the five-hundredth.
            let (amount, cap): (f64, f64) = match media {
                Media::Game => (value / 60.0, 50.0), // minutes -> hours; 50 h saturates
                Media::Artist => (value, 500.0),     // scrobbles
                Media::Album => (value, 120.0),
                Media::Movie | Media::Tv => (value, 5.0),
            };
            1.6 * ((1.0 + amount.max(0.0)).ln() / (1.0 + cap).ln()).min(1.0)
        }
        EventKind::Watched => {
            let plays = meta["plays"].as_f64().unwrap_or(value).max(1.0);
            let rewatch = if meta["rewatch"] == true { 1.25 } else { 1.0 };
            (0.8 * (1.0 + 0.25 * plays.ln()) * rewatch).min(1.4)
        }
        EventKind::Owned => 0.5,
        EventKind::Wishlist => 0.5,
        EventKind::Dismiss => match meta["reason"].as_str() {
            Some("seen") => 0.0, // already seen it: says nothing about taste
            _ => -1.5,
        },
    }
}

/// Recency multiplier in 0.35..=1. Sustained activity (still happening long after it started)
/// fades slowly; a one-off spike fades fast. Ratings are deliberate statements, so they hold longer.
pub fn recency(kind: EventKind, at: &str, first_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> f64 {
    let age = days_between(at, now);
    let span = first_at.map(|f| (days_between(f, now) - age).max(0.0)).unwrap_or(0.0);
    let mut half_life = if span >= 60.0 { 900.0 } else { 300.0 };
    if matches!(kind, EventKind::Rating | EventKind::Pin | EventKind::Like) {
        half_life *= 2.0;
    }
    0.5f64.powf(age / half_life).max(0.35)
}

/// Combines every event on one item into a single affinity (positive = likes it).
pub fn item_affinity(events: &[&Event], settings: &TasteSettings, now: chrono::DateTime<chrono::Utc>) -> f64 {
    let mut rating: Option<f64> = None;
    let mut explicit = 0.0; // likes, pins, more-like, dismissals
    let mut play: f64 = 0.0;
    let mut passive: f64 = 0.0; // watched / owned / wishlist: strongest one counts
    for e in events {
        let w = event_strength(e.kind, e.value, e.media, &e.meta) * recency(e.kind, &e.at, e.first_at.as_deref(), now) * settings.source_weight(&e.source);
        match e.kind {
            EventKind::Rating => {
                // Marquee's own rating wins; otherwise the most recent one.
                if rating.is_none() || e.source == "marquee" {
                    rating = Some(w);
                }
            }
            EventKind::Like | EventKind::MoreLike | EventKind::Pin | EventKind::Dismiss => explicit += w,
            EventKind::Play => play = play.max(w),
            EventKind::Watched | EventKind::Owned | EventKind::Wishlist => passive = passive.max(w),
        }
    }
    match rating {
        // A bad rating overrides the fact that you watched or own it.
        Some(r) if r < 0.0 => r + explicit.min(0.0),
        Some(r) => r + explicit + 0.3 * play,
        None => explicit + play + if play > 0.0 { 0.25 * passive } else { passive },
    }
}

pub fn load_events(db: &Db) -> Result<Vec<Event>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT e.item_id, i.media, e.source, e.kind, e.value, e.at, e.first_at, e.meta
         FROM user_events e JOIN items i ON i.id = e.item_id ORDER BY e.item_id, e.at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, f64>(4)?, r.get::<_, String>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, String>(7)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(item_id, media, source, kind, value, at, first_at, meta)| {
            Some(Event { item_id, media: Media::parse(&media)?, source, kind: EventKind::parse(&kind)?, value, at, first_at, meta: serde_json::from_str(&meta).unwrap_or(Value::Null) })
        })
        .collect())
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct ProfileStats {
    pub events: usize,
    pub items: usize,
    pub features: usize,
    /// The score a typical favourite gets; match percentages are calibrated against it.
    pub reference: f64,
}

/// The profile in memory: feature -> weight in -1..=1.
#[derive(Debug, Default, Clone)]
pub struct Profile {
    pub weights: HashMap<String, f64>,
    pub support: HashMap<String, i64>,
    pub blocked: Vec<String>,
    pub reference: f64,
}

impl Profile {
    pub fn load(db: &Db) -> Result<Profile> {
        let mut p = Profile::default();
        {
            let conn = db.lock();
            let mut stmt = conn.prepare("SELECT feature, weight, support FROM taste_profile")?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?)))? {
                let (f, w, s) = row?;
                p.support.insert(f.clone(), s);
                p.weights.insert(f, w);
            }
            let mut stmt = conn.prepare("SELECT feature FROM taste_overrides WHERE mode = 'block'")?;
            p.blocked = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        }
        p.reference = store_reference(db);
        Ok(p)
    }

    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }
}

fn store_reference(db: &Db) -> f64 {
    crate::store::get_setting::<f64>(db, "taste.reference").filter(|r| *r > 0.0).unwrap_or(0.25)
}

/// Per-item affinities, for everything the user has touched.
pub fn affinities(db: &Db, settings: &TasteSettings) -> Result<HashMap<i64, f64>> {
    let events = load_events(db)?;
    let now = chrono::Utc::now();
    let mut by_item: HashMap<i64, Vec<&Event>> = HashMap::new();
    for e in &events {
        by_item.entry(e.item_id).or_default().push(e);
    }
    Ok(by_item.into_iter().map(|(id, evs)| (id, item_affinity(&evs, settings, now))).collect())
}

/// Rebuilds `taste_profile` from scratch. Cheap enough to run after every sync and rating.
pub fn rebuild(db: &Db) -> Result<ProfileStats> {
    let settings = TasteSettings::load(db);
    let events = load_events(db)?;
    let now = chrono::Utc::now();
    let mut by_item: HashMap<i64, Vec<&Event>> = HashMap::new();
    for e in &events {
        by_item.entry(e.item_id).or_default().push(e);
    }
    let affinity: HashMap<i64, f64> = by_item.iter().map(|(id, evs)| (*id, item_affinity(evs, &settings, now))).collect();

    let conn = db.lock();
    // How common each feature is across the whole catalog: rare features say more.
    let total_items: f64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get::<_, i64>(0))? as f64;
    let mut df: HashMap<String, f64> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT feature, count(*) FROM item_features GROUP BY feature")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (f, n) = row?;
            df.insert(f, n as f64);
        }
    }

    // "Not interested" with a reason only counts against the part named: the genre and mood,
    // or the people who made it.
    let reasons: HashMap<i64, &str> = events.iter().filter(|e| e.kind == EventKind::Dismiss).filter_map(|e| Some((e.item_id, e.meta["reason"].as_str()?))).collect();

    let mut raw: HashMap<String, f64> = HashMap::new();
    let mut support: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached("SELECT feature, weight FROM item_features WHERE item_id = ?1")?;
        for (item, a) in &affinity {
            if a.abs() < 0.01 {
                continue;
            }
            for row in stmt.query_map([item], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
                let (f, w) = row?;
                if *a < 0.0 {
                    let kind = f.as_bytes().first().copied().unwrap_or(b'?');
                    match reasons.get(item).copied() {
                        Some("genre") if !matches!(kind, b'g' | b't') => continue,
                        Some("creator") if kind != b'c' => continue,
                        _ => {}
                    }
                }
                *raw.entry(f.clone()).or_insert(0.0) += a * w;
                if *a > 0.0 {
                    *support.entry(f).or_insert(0) += 1;
                }
            }
        }
    }

    // Log-compress (ten sci-fi films aren't ten times one), weight by rarity, then scale each
    // kind of feature to -1..=1 so genres, tags and creators are comparable.
    let mut weights: HashMap<String, f64> = HashMap::new();
    for (f, w) in &raw {
        let idf = (1.0 + total_items / (1.0 + df.get(f).copied().unwrap_or(1.0))).ln().clamp(0.5, 3.0);
        weights.insert(f.clone(), w.signum() * (1.0 + w.abs()).ln() * idf);
    }
    let mut max_by_kind: HashMap<u8, f64> = HashMap::new();
    for (f, w) in &weights {
        let k = f.as_bytes().first().copied().unwrap_or(b'?');
        let m = max_by_kind.entry(k).or_insert(0.0);
        *m = m.max(w.abs());
    }
    for (f, w) in weights.iter_mut() {
        let k = f.as_bytes().first().copied().unwrap_or(b'?');
        let m = max_by_kind.get(&k).copied().unwrap_or(1.0);
        if m > 0.0 {
            *w /= m;
        }
    }
    // Pinned loves sit at the top of their kind no matter what the history says.
    {
        let mut stmt = conn.prepare("SELECT feature FROM taste_overrides WHERE mode = 'pin'")?;
        for f in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let f = f?;
            weights.insert(f.clone(), 1.0);
            support.entry(f).or_insert(1);
        }
    }

    conn.execute("DELETE FROM taste_profile", [])?;
    {
        let mut ins = conn.prepare("INSERT INTO taste_profile (feature, weight, support) VALUES (?1, ?2, ?3)")?;
        for (f, w) in &weights {
            if w.abs() >= 0.01 {
                ins.execute(params![f, w, support.get(f).copied().unwrap_or(0)])?;
            }
        }
    }
    drop(conn);

    // Calibrate: what does one of the user's own favourites score?
    let profile = Profile { weights: weights.clone(), support, blocked: vec![], reference: 1.0 };
    let mut favourites: Vec<(i64, f64)> = affinity.iter().filter(|(_, a)| **a > 1.0).map(|(i, a)| (*i, *a)).collect();
    favourites.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut scores = vec![];
    {
        let conn = db.lock();
        for (id, _) in favourites.iter().take(40) {
            let feats = crate::store::item_features(&conn, *id)?;
            if feats.len() >= 3 {
                scores.push(super::score::similarity(&profile, &feats, settings.dial));
            }
        }
    }
    let reference = if scores.is_empty() { 0.25 } else { (scores.iter().sum::<f64>() / scores.len() as f64).max(0.05) };
    crate::store::set_setting(db, "taste.reference", &reference)?;

    Ok(ProfileStats { events: events.len(), items: affinity.len(), features: weights.len(), reference })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{record_event, upsert_item};

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
    fn ago(days: i64) -> String {
        (now() - chrono::Duration::days(days)).to_rfc3339()
    }
    fn ev(kind: EventKind, value: f64, media: Media, at_days: i64) -> Event {
        Event { item_id: 1, media, source: "x".into(), kind, value, at: ago(at_days), first_at: None, meta: Value::Null }
    }

    #[test]
    fn signal_tiers_are_in_the_order_the_spec_gives() {
        let n = Value::Null;
        let rating = event_strength(EventKind::Rating, 5.0, Media::Movie, &n);
        let like = event_strength(EventKind::Like, 1.0, Media::Movie, &n);
        let play = event_strength(EventKind::Play, 100.0 * 60.0, Media::Game, &n);
        let watched = event_strength(EventKind::Watched, 1.0, Media::Movie, &n);
        let owned = event_strength(EventKind::Owned, 1.0, Media::Game, &n);
        let wish = event_strength(EventKind::Wishlist, 1.0, Media::Game, &n);
        assert!(rating > like && like > play && play > watched && watched > owned && owned >= wish, "{rating} {like} {play} {watched} {owned} {wish}");
    }

    #[test]
    fn a_bad_rating_is_a_negative_signal() {
        assert!(event_strength(EventKind::Rating, 1.0, Media::Movie, &Value::Null) < -2.0);
        assert!(event_strength(EventKind::Rating, 2.0, Media::Movie, &Value::Null) < 0.0);
        assert!(event_strength(EventKind::Rating, 3.0, Media::Movie, &Value::Null) > 0.0);
        // Watched it, hated it: the watch doesn't soften the rating.
        let s = TasteSettings::default();
        let watched = ev(EventKind::Watched, 1.0, Media::Movie, 1);
        let hated = ev(EventKind::Rating, 1.0, Media::Movie, 1);
        assert!(item_affinity(&[&watched, &hated], &s, now()) < -2.0);
    }

    #[test]
    fn playtime_is_logarithmic_and_saturates() {
        let n = Value::Null;
        let two = event_strength(EventKind::Play, 120.0, Media::Game, &n);
        let twenty = event_strength(EventKind::Play, 1200.0, Media::Game, &n);
        let two_hundred = event_strength(EventKind::Play, 12_000.0, Media::Game, &n);
        assert!(two < twenty && twenty < two_hundred);
        assert!(twenty - two > two_hundred - twenty, "early hours count for more");
        assert_eq!(two_hundred, 1.6);
        assert!(event_strength(EventKind::Play, 3.0, Media::Game, &n) < 0.1, "three minutes says almost nothing");
    }

    #[test]
    fn sustained_activity_decays_more_slowly_than_a_spike() {
        // Both last touched a year ago. One was a two-week binge, the other ran for two years.
        let spike = recency(EventKind::Play, &ago(365), Some(&ago(379)), now());
        let sustained = recency(EventKind::Play, &ago(365), Some(&ago(365 + 730)), now());
        assert!(sustained > spike + 0.15, "sustained {sustained} vs spike {spike}");
        // Nothing ever fades to nothing.
        assert!(recency(EventKind::Play, &ago(5000), None, now()) >= 0.35);
        // Ratings hold their weight longer than plays of the same age.
        assert!(recency(EventKind::Rating, &ago(600), None, now()) > recency(EventKind::Play, &ago(600), None, now()));
        assert!((recency(EventKind::Play, &ago(0), None, now()) - 1.0).abs() < 0.01);
    }

    #[test]
    fn already_seen_is_not_a_taste_signal_but_not_my_genre_is() {
        assert_eq!(event_strength(EventKind::Dismiss, 1.0, Media::Movie, &serde_json::json!({ "reason": "seen" })), 0.0);
        assert!(event_strength(EventKind::Dismiss, 1.0, Media::Movie, &serde_json::json!({ "reason": "genre" })) < 0.0);
    }

    #[test]
    fn source_weights_scale_a_source_and_zero_silences_it() {
        let mut s = TasteSettings::default();
        let mut e = ev(EventKind::Like, 1.0, Media::Album, 0);
        e.source = "lastfm".into();
        let full = item_affinity(&[&e], &s, now());
        s.source_weights.insert("lastfm".into(), 0.5);
        assert!((item_affinity(&[&e], &s, now()) - full / 2.0).abs() < 1e-9);
        s.source_weights.insert("lastfm".into(), 0.0);
        assert_eq!(item_affinity(&[&e], &s, now()), 0.0);
    }

    fn seed(db: &Db, media: Media, title: &str, genres: &[&str], tags: &[&str], creator: &str, rating: Option<f64>) -> i64 {
        let conn = db.lock();
        let mut d = ItemDraft::new(media, title).id("test", title);
        d.genres = genres.iter().map(|s| s.to_string()).collect();
        d.tags = tags.iter().map(|s| s.to_string()).collect();
        if !creator.is_empty() {
            d.creators = vec![Creator::new(creator, "director")];
        }
        let id = upsert_item(&conn, &d).unwrap();
        if let Some(r) = rating {
            record_event(&conn, id, "marquee", EventKind::Rating, r, None, None, &Value::Null).unwrap();
        }
        id
    }

    #[test]
    fn the_profile_likes_what_you_rate_highly_and_dislikes_what_you_pan() {
        let db = Db::memory().unwrap();
        seed(&db, Media::Movie, "Arrival", &["Science Fiction", "Drama"], &["alien contact", "slow burn", "melancholy"], "Denis Villeneuve", Some(5.0));
        seed(&db, Media::Movie, "Blade Runner 2049", &["Science Fiction"], &["cyberpunk", "neo noir", "slow burn"], "Denis Villeneuve", Some(4.5));
        seed(&db, Media::Movie, "Loud Sequel 7", &["Action"], &["superhero"], "Some Guy", Some(1.0));
        // Unrated catalog padding so rarity means something.
        for n in 0..20 {
            seed(&db, Media::Movie, &format!("Filler {n}"), &["Drama"], &[], "", None);
        }
        let stats = rebuild(&db).unwrap();
        assert_eq!(stats.items, 3);
        let p = Profile::load(&db).unwrap();
        assert!(p.weights["t:slow-burn"] > 0.5);
        assert!(p.weights["c:denis-villeneuve"] > 0.9);
        assert!(p.weights["t:superhero"] < 0.0);
        assert!(p.weights["g:action"] < 0.0);
        assert!(p.weights["g:sci-fi"] > p.weights.get("g:drama").copied().unwrap_or(0.0), "the rare genre you love outranks the common one");
        assert_eq!(p.support["c:denis-villeneuve"], 2);
        assert!(stats.reference > 0.05);
    }

    #[test]
    fn not_interested_with_a_reason_only_counts_against_that_part() {
        let db = Db::memory().unwrap();
        seed(&db, Media::Movie, "Loved", &["Drama"], &[], "Good Director", Some(5.0));
        let genre = seed(&db, Media::Movie, "Not My Genre", &["Horror"], &[], "Fine Director", None);
        let creator = seed(&db, Media::Movie, "Not My Director", &["Comedy"], &[], "That Director", None);
        {
            let conn = db.lock();
            record_event(&conn, genre, "marquee", EventKind::Dismiss, 1.0, None, None, &serde_json::json!({ "reason": "genre" })).unwrap();
            record_event(&conn, creator, "marquee", EventKind::Dismiss, 1.0, None, None, &serde_json::json!({ "reason": "creator" })).unwrap();
        }
        rebuild(&db).unwrap();
        let p = Profile::load(&db).unwrap();
        assert!(p.weights["g:horror"] < 0.0);
        assert!(!p.weights.contains_key("c:fine-director"), "disliking the genre says nothing about who made it");
        assert!(p.weights["c:that-director"] < 0.0);
        assert!(!p.weights.contains_key("g:comedy"), "disliking the creator says nothing about the genre");
    }

    #[test]
    fn pins_force_a_feature_to_the_top() {
        let db = Db::memory().unwrap();
        seed(&db, Media::Movie, "Arrival", &["Science Fiction"], &[], "", Some(5.0));
        db.lock().execute("INSERT INTO taste_overrides (feature, mode) VALUES ('t:cozy', 'pin')", []).unwrap();
        rebuild(&db).unwrap();
        assert_eq!(Profile::load(&db).unwrap().weights["t:cozy"], 1.0);
    }

    #[test]
    fn an_empty_history_gives_an_empty_profile() {
        let db = Db::memory().unwrap();
        let stats = rebuild(&db).unwrap();
        assert_eq!(stats.features, 0);
        assert!(Profile::load(&db).unwrap().is_empty());
    }
}
