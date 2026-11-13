//! Reads and writes for items, events, lists, follows, settings and sources.
//! Everything that touches SQL lives here so the rest of the crate deals in plain types.

use crate::db::Db;
use crate::error::{Error, Result};
use crate::model::*;
use crate::taste::vocab;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};

// ------------------------------------------------------------------ settings

pub fn get_setting<T: DeserializeOwned>(db: &Db, key: &str) -> Option<T> {
    let raw: Option<String> = db.lock().query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten();
    raw.and_then(|s| serde_json::from_str(&s).ok())
}

pub fn setting_or<T: DeserializeOwned>(db: &Db, key: &str, default: T) -> T {
    get_setting(db, key).unwrap_or(default)
}

pub fn set_setting<T: Serialize>(db: &Db, key: &str, value: &T) -> Result<()> {
    db.lock().execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", params![key, serde_json::to_string(value)?])?;
    Ok(())
}

pub fn all_settings(db: &Db) -> Result<Map<String, Value>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Map::new();
    for row in rows {
        let (k, v) = row?;
        out.insert(k, serde_json::from_str(&v).unwrap_or(Value::Null));
    }
    Ok(out)
}

// --------------------------------------------------------------------- items

pub fn find_item(conn: &Connection, source: &str, media: Media, ext_id: &str) -> Result<Option<i64>> {
    Ok(conn.query_row("SELECT item_id FROM item_ids WHERE source = ?1 AND media = ?2 AND ext_id = ?3", params![source, media.as_str(), ext_id], |r| r.get(0)).optional()?)
}

/// Looser match for imports that only have a title and year.
pub fn find_item_by_title(conn: &Connection, media: Media, title: &str, year: Option<i32>) -> Result<Option<i64>> {
    let st = sort_title(title);
    let mut stmt = conn.prepare("SELECT id, year FROM items WHERE media = ?1 AND sort_title = ?2")?;
    let rows: Vec<(i64, Option<i32>)> = stmt.query_map(params![media.as_str(), st], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(match year {
        Some(y) => rows.iter().find(|(_, iy)| iy.map(|iy| (iy - y).abs() <= 1).unwrap_or(false)).or(if rows.len() == 1 { rows.first() } else { None }).map(|r| r.0),
        None => rows.first().map(|r| r.0),
    })
}

/// Inserts or updates an item and returns its id.
///
/// A draft matches an existing item when any of its external ids is already known. A stub
/// (detail level 0) never overwrites fields that a full fetch filled in.
pub fn upsert_item(conn: &Connection, d: &ItemDraft) -> Result<i64> {
    upsert_item_at(conn, d, None)
}

/// Finds an album by artist and title, so the same record arriving from Spotify, Last.fm and
/// MusicBrainz (each with its own id) becomes one item.
pub fn find_album(conn: &Connection, artist: &str, title: &str) -> Result<Option<i64>> {
    Ok(conn.query_row("SELECT id FROM items WHERE media = 'album' AND sort_title = ?1 AND lower(json_extract(data, '$.artist')) = lower(?2) LIMIT 1", params![sort_title(title), artist], |r| r.get(0)).optional()?)
}

/// Like `upsert_item`, but `target` (when given) names the item to update regardless of ids.
pub fn upsert_item_at(conn: &Connection, d: &ItemDraft, target: Option<i64>) -> Result<i64> {
    let media = d.media();
    let mut existing = target;
    if existing.is_none() {
        for (source, ext) in &d.ids {
            if let Some(id) = find_item(conn, source, media, ext)? {
                existing = Some(id);
                break;
            }
        }
    }
    if existing.is_none() && media == Media::Album {
        if let Some(artist) = d.data.get("artist").and_then(|a| a.as_str()) {
            existing = find_album(conn, artist, &d.title)?;
        }
    }
    if existing.is_none() && d.ids.is_empty() {
        existing = find_item_by_title(conn, media, &d.title, d.year)?;
    }
    let now = now();
    let expires = (chrono::Utc::now() + chrono::Duration::seconds(d.ttl_secs.max(3600))).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let year = d.year.or_else(|| d.release_date.as_deref().and_then(year_of));
    let mut data = d.data.clone();
    if !d.genres.is_empty() {
        data.insert("genres".into(), json!(d.genres));
    }
    if !d.creators.is_empty() {
        data.insert("creators".into(), json!(d.creators));
    }
    if !d.tags.is_empty() {
        data.insert("tags".into(), json!(d.tags.iter().take(40).collect::<Vec<_>>()));
    }
    if let Some(l) = &d.language {
        data.insert("language".into(), json!(l));
    }

    let id = match existing {
        Some(id) => {
            let (old_level, old_data): (i64, String) = conn.query_row("SELECT detail_level, data FROM items WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            // Merge JSON: new keys win, but a stub keeps what a detail fetch stored.
            let mut merged: Map<String, Value> = serde_json::from_str(&old_data).unwrap_or_default();
            for (k, v) in data {
                if d.detail_level >= old_level || !merged.contains_key(&k) {
                    merged.insert(k, v);
                }
            }
            let level = old_level.max(d.detail_level);
            conn.execute(
                "UPDATE items SET
                    title = CASE WHEN ?2 = '' THEN title ELSE ?2 END,
                    sort_title = CASE WHEN ?2 = '' THEN sort_title ELSE ?3 END,
                    year = COALESCE(?4, year), release_date = COALESCE(?5, release_date),
                    poster = COALESCE(?6, poster), backdrop = COALESCE(?7, backdrop),
                    overview = COALESCE(?8, overview), runtime = COALESCE(?9, runtime),
                    popularity = CASE WHEN ?10 > 0 THEN ?10 ELSE popularity END,
                    rating = COALESCE(?11, rating), data = ?12, detail_level = ?13,
                    updated_at = ?14, expires_at = ?15
                 WHERE id = ?1",
                params![
                    id,
                    d.title,
                    sort_title(&d.title),
                    year,
                    d.release_date,
                    d.poster,
                    d.backdrop,
                    d.overview.as_deref().filter(|s| !s.is_empty()),
                    d.runtime,
                    d.popularity,
                    d.rating,
                    Value::Object(merged).to_string(),
                    level,
                    now,
                    expires
                ],
            )?;
            id
        }
        None => {
            if d.title.trim().is_empty() {
                return Err(Error::msg("Can't store an item with no title"));
            }
            conn.execute(
                "INSERT INTO items (media, title, sort_title, year, release_date, poster, backdrop, overview, runtime,
                                    popularity, rating, data, detail_level, updated_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![media.as_str(), d.title, sort_title(&d.title), year, d.release_date, d.poster, d.backdrop, d.overview, d.runtime, d.popularity, d.rating, Value::Object(data).to_string(), d.detail_level, now, expires],
            )?;
            conn.last_insert_rowid()
        }
    };

    for (source, ext) in &d.ids {
        conn.execute("INSERT OR IGNORE INTO item_ids (source, ext_id, media, item_id) VALUES (?1, ?2, ?3, ?4)", params![source, ext, media.as_str(), id])?;
    }

    // Features: a stub adds to what's there; a detail fetch replaces the set.
    let features = vocab::features_for(d);
    if !features.is_empty() {
        let has_any: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM item_features WHERE item_id = ?1)", [id], |r| r.get(0))?;
        if d.detail_level >= 1 && has_any {
            conn.execute("DELETE FROM item_features WHERE item_id = ?1", [id])?;
        }
        let mut stmt = conn.prepare_cached(
            "INSERT INTO item_features (item_id, feature, weight) VALUES (?1, ?2, ?3)
             ON CONFLICT (item_id, feature) DO UPDATE SET weight = max(weight, excluded.weight)",
        )?;
        for (f, w) in &features {
            stmt.execute(params![id, f, w])?;
        }
    }

    for s in &d.similar {
        if s.media() == Media::Artist && media != Media::Artist {
            continue;
        }
        if let Ok(sid) = upsert_item(conn, s) {
            if sid != id {
                conn.execute("INSERT OR IGNORE INTO item_similar (item_id, similar_id) VALUES (?1, ?2)", params![id, sid])?;
            }
        }
    }
    Ok(id)
}

fn item_ids(conn: &Connection, id: i64) -> Result<Map<String, Value>> {
    let mut stmt = conn.prepare_cached("SELECT source, ext_id FROM item_ids WHERE item_id = ?1")?;
    let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Map::new();
    for row in rows {
        let (s, e) = row?;
        out.entry(s).or_insert(Value::String(e));
    }
    Ok(out)
}

pub fn get_item(conn: &Connection, id: i64) -> Result<Item> {
    let mut item = conn
        .query_row(
            "SELECT id, media, title, year, release_date, poster, backdrop, overview, runtime, popularity, rating, data, detail_level
             FROM items WHERE id = ?1",
            [id],
            |r| {
                Ok(Item {
                    id: r.get(0)?,
                    media: Media::parse(&r.get::<_, String>(1)?).unwrap_or(Media::Movie),
                    title: r.get(2)?,
                    year: r.get(3)?,
                    release_date: r.get(4)?,
                    poster: r.get(5)?,
                    backdrop: r.get(6)?,
                    overview: r.get(7)?,
                    runtime: r.get(8)?,
                    popularity: r.get(9)?,
                    rating: r.get(10)?,
                    data: serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or(Value::Null),
                    detail_level: r.get(12)?,
                    ids: Map::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(|| Error::msg("That title isn't in your library anymore."))?;
    item.ids = item_ids(conn, id)?;
    Ok(item)
}

pub fn item_is_stale(conn: &Connection, id: i64) -> bool {
    conn.query_row("SELECT detail_level < 1 OR expires_at IS NULL OR expires_at < ?2 FROM items WHERE id = ?1", params![id, now()], |r| r.get(0)).unwrap_or(true)
}

pub fn item_features(conn: &Connection, id: i64) -> Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare_cached("SELECT feature, weight FROM item_features WHERE item_id = ?1")?;
    let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The column list every card query selects. Keep `card_from_row` in step with it.
pub const CARD_COLS: &str = "i.id, i.media, i.title, i.year, i.release_date, i.poster, i.backdrop, i.overview, i.runtime, i.rating,
    i.popularity, s.match_pct, s.why, i.data,
    (SELECT value FROM user_events e WHERE e.item_id = i.id AND e.kind = 'rating' ORDER BY (e.source = 'marquee') DESC, e.at DESC LIMIT 1),
    EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'play', 'rating')),
    EXISTS (SELECT 1 FROM list_items li WHERE li.item_id = i.id)";

pub const CARD_FROM: &str = "items i LEFT JOIN scores s ON s.item_id = i.id";

pub fn card_from_row(r: &Row) -> rusqlite::Result<Card> {
    let data: Value = serde_json::from_str(&r.get::<_, String>(13)?).unwrap_or(Value::Null);
    let why: Vec<String> = r.get::<_, Option<String>>(12)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut meta = Map::new();
    for key in ["artist", "price", "discount", "platforms", "providers", "next_episode", "album_type", "status", "deck", "low", "seasons_count", "network", "developer", "director", "trailer", "spotify_url", "store_url", "header"] {
        if let Some(v) = data.get(key).filter(|v| !v.is_null()) {
            meta.insert(key.into(), v.clone());
        }
    }
    Ok(Card {
        id: r.get(0)?,
        media: r.get(1)?,
        title: r.get(2)?,
        year: r.get(3)?,
        release_date: r.get(4)?,
        poster: r.get(5)?,
        backdrop: r.get(6)?,
        overview: r.get(7)?,
        runtime: r.get(8)?,
        rating: r.get(9)?,
        popularity: r.get(10)?,
        match_pct: r.get::<_, Option<i64>>(11)?.filter(|p| *p >= 0),
        why,
        genres: data["genres"].as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect()).unwrap_or_default(),
        meta: Value::Object(meta),
        user_rating: r.get(14)?,
        done: r.get(15)?,
        saved: r.get(16)?,
    })
}

/// Runs `SELECT <card columns> FROM items i LEFT JOIN scores s ... <tail>`.
pub fn cards(conn: &Connection, tail: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Card>> {
    let sql = format!("SELECT {CARD_COLS} FROM {CARD_FROM} {tail}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(args, card_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn card(conn: &Connection, id: i64) -> Result<Option<Card>> {
    Ok(cards(conn, "WHERE i.id = ?1", &[&id])?.into_iter().next())
}

pub fn cards_by_ids(conn: &Connection, ids: &[i64]) -> Result<Vec<Card>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT {CARD_COLS} FROM {CARD_FROM} WHERE i.id IN ({marks})");
    let mut stmt = conn.prepare(&sql)?;
    let mut found: Vec<Card> = stmt.query_map(params_from_iter(ids.iter()), card_from_row)?.collect::<rusqlite::Result<_>>()?;
    found.sort_by_key(|c| ids.iter().position(|i| *i == c.id).unwrap_or(usize::MAX));
    Ok(found)
}

// -------------------------------------------------------------------- events

/// Records (or updates) one thing the user did. Returns true when something changed.
#[allow(clippy::too_many_arguments)]
pub fn record_event(conn: &Connection, item_id: i64, source: &str, kind: EventKind, value: f64, at: Option<&str>, first_at: Option<&str>, meta: &Value) -> Result<bool> {
    let at = at.map(str::to_string).unwrap_or_else(now);
    let n = conn.execute(
        "INSERT INTO user_events (item_id, source, kind, value, at, first_at, meta) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (item_id, source, kind) DO UPDATE SET
            value = excluded.value,
            at = max(at, excluded.at),
            first_at = min(COALESCE(first_at, at), COALESCE(excluded.first_at, excluded.at)),
            meta = excluded.meta
         WHERE value != excluded.value OR at < excluded.at OR meta != excluded.meta",
        params![item_id, source, kind.as_str(), value, at, first_at.unwrap_or(&at), if meta.is_null() { "{}".to_string() } else { meta.to_string() }],
    )?;
    Ok(n > 0)
}

pub fn remove_event(conn: &Connection, item_id: i64, source: &str, kind: EventKind) -> Result<()> {
    conn.execute("DELETE FROM user_events WHERE item_id = ?1 AND source = ?2 AND kind = ?3", params![item_id, source, kind.as_str()])?;
    Ok(())
}

pub fn clear_source_events(conn: &Connection, source: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM user_events WHERE source = ?1", [source])?)
}

pub fn user_rating(conn: &Connection, item_id: i64) -> Option<f64> {
    conn.query_row("SELECT value FROM user_events WHERE item_id = ?1 AND kind = 'rating' ORDER BY (source = 'marquee') DESC, at DESC LIMIT 1", [item_id], |r| r.get(0)).optional().ok().flatten()
}

pub fn has_event(conn: &Connection, item_id: i64, kinds: &[&str]) -> bool {
    let list = kinds.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(",");
    conn.query_row(&format!("SELECT EXISTS (SELECT 1 FROM user_events WHERE item_id = ?1 AND kind IN ({list}))"), [item_id], |r| r.get(0)).unwrap_or(false)
}

// --------------------------------------------------------------------- lists

pub fn list_id(conn: &Connection, slug: &str) -> Result<i64> {
    conn.query_row("SELECT id FROM lists WHERE slug = ?1", [slug], |r| r.get(0)).optional()?.ok_or_else(|| Error::msg(format!("There's no list called \"{slug}\".")))
}

pub fn lists(conn: &Connection) -> Result<Vec<ListInfo>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.slug, l.name, l.kind, l.mirror, (SELECT count(*) FROM list_items li WHERE li.list_id = l.id)
         FROM lists l ORDER BY (l.kind = 'system') DESC, l.position, l.id",
    )?;
    let mut out: Vec<ListInfo> = stmt.query_map([], |r| Ok(ListInfo { id: r.get(0)?, slug: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, mirror: r.get(4)?, count: r.get(5)?, covers: vec![] }))?.collect::<rusqlite::Result<_>>()?;
    let mut covers = conn.prepare(
        "SELECT i.poster FROM list_items li JOIN items i ON i.id = li.item_id
         WHERE li.list_id = ?1 AND i.poster IS NOT NULL ORDER BY li.position, li.added_at DESC LIMIT 4",
    )?;
    for l in &mut out {
        l.covers = covers.query_map([l.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    }
    Ok(out)
}

pub fn create_list(conn: &Connection, name: &str) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::msg("Give the list a name."));
    }
    let base = slug(name);
    let base = if base.is_empty() { "list".to_string() } else { base };
    let mut candidate = base.clone();
    let mut n = 2;
    while conn.query_row("SELECT EXISTS (SELECT 1 FROM lists WHERE slug = ?1)", [&candidate], |r| r.get::<_, bool>(0))? {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    let pos: i64 = conn.query_row("SELECT COALESCE(max(position), 0) + 1 FROM lists", [], |r| r.get(0))?;
    conn.execute("INSERT INTO lists (slug, name, kind, position, created_at) VALUES (?1, ?2, 'custom', ?3, ?4)", params![candidate, name, pos, now()])?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_list(conn: &Connection, id: i64, name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(Error::msg("Give the list a name."));
    }
    conn.execute("UPDATE lists SET name = ?2 WHERE id = ?1 AND kind = 'custom'", params![id, name.trim()])?;
    Ok(())
}

pub fn delete_list(conn: &Connection, id: i64) -> Result<()> {
    let n = conn.execute("DELETE FROM lists WHERE id = ?1 AND kind = 'custom'", [id])?;
    if n == 0 {
        return Err(Error::msg("The built-in lists can't be deleted."));
    }
    Ok(())
}

/// Adds an item to a list. Returns false if it was already there.
pub fn add_to_list(conn: &Connection, list_id: i64, item_id: i64, sync_state: &str) -> Result<bool> {
    let pos: i64 = conn.query_row("SELECT COALESCE(min(position), 0) - 1 FROM list_items WHERE list_id = ?1", [list_id], |r| r.get(0))?;
    let n = conn.execute(
        "INSERT INTO list_items (list_id, item_id, position, added_at, sync_state) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (list_id, item_id) DO UPDATE SET sync_state = excluded.sync_state
         WHERE list_items.sync_state != excluded.sync_state AND excluded.sync_state != 'local'",
        params![list_id, item_id, pos, now(), sync_state],
    )?;
    Ok(n > 0)
}

pub fn remove_from_list(conn: &Connection, list_id: i64, item_id: i64) -> Result<()> {
    conn.execute("DELETE FROM list_items WHERE list_id = ?1 AND item_id = ?2", params![list_id, item_id])?;
    Ok(())
}

pub fn set_sync_state(conn: &Connection, list_id: i64, item_id: i64, state: &str) -> Result<()> {
    conn.execute("UPDATE list_items SET sync_state = ?3 WHERE list_id = ?1 AND item_id = ?2", params![list_id, item_id, state])?;
    Ok(())
}

pub fn reorder_list(conn: &Connection, list_id: i64, item_ids: &[i64]) -> Result<()> {
    let mut stmt = conn.prepare("UPDATE list_items SET position = ?3 WHERE list_id = ?1 AND item_id = ?2")?;
    for (pos, item) in item_ids.iter().enumerate() {
        stmt.execute(params![list_id, item, pos as i64])?;
    }
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct ListEntry {
    #[serde(flatten)]
    pub card: Card,
    pub sync_state: String,
    pub added_at: String,
    pub note: Option<String>,
}

pub fn list_entries(conn: &Connection, list_id: i64) -> Result<Vec<ListEntry>> {
    let sql = format!(
        "SELECT {CARD_COLS}, li.sync_state, li.added_at, li.note FROM {CARD_FROM}
         JOIN list_items li ON li.item_id = i.id WHERE li.list_id = ?1 ORDER BY li.position, li.added_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([list_id], |r| Ok(ListEntry { card: card_from_row(r)?, sync_state: r.get(17)?, added_at: r.get(18)?, note: r.get(19)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn lists_containing(conn: &Connection, item_id: i64) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT list_id, sync_state FROM list_items WHERE item_id = ?1")?;
    let rows = stmt.query_map([item_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ------------------------------------------------------------------- follows

pub fn follow(conn: &Connection, kind: &str, reference: &str, name: &str, item_id: Option<i64>) -> Result<()> {
    conn.execute(
        "INSERT INTO follows (kind, ref, name, item_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (kind, ref) DO UPDATE SET name = excluded.name",
        params![kind, reference, name, item_id, now()],
    )?;
    Ok(())
}

pub fn unfollow(conn: &Connection, kind: &str, reference: &str) -> Result<()> {
    conn.execute("DELETE FROM follows WHERE kind = ?1 AND ref = ?2", params![kind, reference])?;
    Ok(())
}

pub fn is_following(conn: &Connection, kind: &str, reference: &str) -> bool {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM follows WHERE kind = ?1 AND ref = ?2)", params![kind, reference], |r| r.get(0)).unwrap_or(false)
}

#[derive(Serialize, Clone, Debug)]
pub struct Follow {
    pub id: i64,
    pub kind: String,
    pub reference: String,
    pub name: String,
    pub item_id: Option<i64>,
}

pub fn follows(conn: &Connection) -> Result<Vec<Follow>> {
    let mut stmt = conn.prepare("SELECT id, kind, ref, name, item_id FROM follows ORDER BY created_at DESC")?;
    let rows = stmt.query_map([], |r| Ok(Follow { id: r.get(0)?, kind: r.get(1)?, reference: r.get(2)?, name: r.get(3)?, item_id: r.get(4)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ------------------------------------------------------------- notifications

/// Stores a notification once (`dedupe` identifies it). Returns its id when it's new.
pub fn add_notification(conn: &Connection, kind: &str, dedupe: &str, title: &str, body: &str, item_id: Option<i64>) -> Result<Option<i64>> {
    let n = conn.execute("INSERT OR IGNORE INTO notifications (kind, dedupe, title, body, item_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![kind, dedupe, title, body, item_id, now()])?;
    Ok(if n > 0 { Some(conn.last_insert_rowid()) } else { None })
}

pub fn notifications(conn: &Connection, limit: i64) -> Result<Vec<Notification>> {
    let mut stmt = conn.prepare("SELECT id, kind, title, body, item_id, created_at, read_at IS NOT NULL FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit], |r| Ok(Notification { id: r.get(0)?, kind: r.get(1)?, title: r.get(2)?, body: r.get(3)?, item_id: r.get(4)?, created_at: r.get(5)?, read: r.get(6)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn untoasted(conn: &Connection) -> Result<Vec<Notification>> {
    let mut stmt = conn.prepare("SELECT id, kind, title, body, item_id, created_at, read_at IS NOT NULL FROM notifications WHERE toasted = 0 ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok(Notification { id: r.get(0)?, kind: r.get(1)?, title: r.get(2)?, body: r.get(3)?, item_id: r.get(4)?, created_at: r.get(5)?, read: r.get(6)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn mark_toasted(conn: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        conn.execute("UPDATE notifications SET toasted = 1 WHERE id = ?1", [id])?;
    }
    Ok(())
}

// ------------------------------------------------------------------- sources

pub fn set_source(conn: &Connection, id: &str, status: &str, account: Option<&str>, error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE sources SET status = ?2, account = COALESCE(?3, account), error = ?4,
                last_sync = CASE WHEN ?2 = 'connected' AND ?5 THEN ?6 ELSE last_sync END
         WHERE id = ?1",
        params![id, status, account, error, error.is_none(), now()],
    )?;
    Ok(())
}

pub fn set_source_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute("UPDATE sources SET status = ?2 WHERE id = ?1", params![id, status])?;
    Ok(())
}

pub fn source_config(conn: &Connection, id: &str) -> Value {
    conn.query_row("SELECT config FROM sources WHERE id = ?1", [id], |r| r.get::<_, String>(0)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
}

pub fn set_source_config(conn: &Connection, id: &str, patch: Value) -> Result<()> {
    let mut cfg = source_config(conn, id);
    if let (Some(obj), Some(p)) = (cfg.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            if v.is_null() {
                obj.remove(k);
            } else {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    conn.execute("UPDATE sources SET config = ?2 WHERE id = ?1", params![id, cfg.to_string()])?;
    Ok(())
}

/// A connection's status, account name, last sync time and last error.
pub type SourceRow = (String, Option<String>, Option<String>, Option<String>);

pub fn source_row(conn: &Connection, id: &str) -> Result<SourceRow> {
    Ok(conn.query_row("SELECT status, account, last_sync, error FROM sources WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?)
}

pub fn disconnect_source(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE sources SET status = 'disconnected', account = NULL, error = NULL, last_sync = NULL, config = '{}' WHERE id = ?1", [id])?;
    Ok(())
}

pub fn is_connected(conn: &Connection, id: &str) -> bool {
    conn.query_row("SELECT status != 'disconnected' FROM sources WHERE id = ?1", [id], |r| r.get(0)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival() -> ItemDraft {
        let mut d = ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865).id("imdb", "tt2543164");
        d.year = Some(2016);
        d.genres = vec!["Science Fiction".into(), "Drama".into()];
        d.tags = vec!["alien contact".into(), "slow burn".into()];
        d.creators = vec![Creator::new("Denis Villeneuve", "director")];
        d
    }

    #[test]
    fn upsert_matches_on_any_known_id() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        let a = upsert_item(&conn, &arrival()).unwrap();
        // Same film arriving from Letterboxd with only the IMDb id.
        let b = upsert_item(&conn, &ItemDraft::new(Media::Movie, "Arrival").id("imdb", "tt2543164").id("letterboxd", "arrival-2016")).unwrap();
        assert_eq!(a, b);
        let item = get_item(&conn, a).unwrap();
        assert_eq!(item.ext("tmdb"), Some("329865"));
        assert_eq!(item.ext("letterboxd"), Some("arrival-2016"));
        // A tv show with the same tmdb number is a different thing.
        let c = upsert_item(&conn, &ItemDraft::new(Media::Tv, "Other").id("tmdb", 329865)).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn a_stub_never_wipes_out_full_details() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        let mut full = arrival();
        full.detail_level = 1;
        full.overview = Some("A linguist...".into());
        full.set("trailer", "abc");
        let id = upsert_item(&conn, &full).unwrap();
        let mut stub = ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865);
        stub.set("trailer", "zzz");
        upsert_item(&conn, &stub).unwrap();
        let item = get_item(&conn, id).unwrap();
        assert_eq!(item.detail_level, 1);
        assert_eq!(item.overview.as_deref(), Some("A linguist..."));
        assert_eq!(item.data["trailer"], "abc");
        assert!(item_features(&conn, id).unwrap().iter().any(|(f, _)| f == "c:denis-villeneuve"));
    }

    #[test]
    fn events_update_in_place_and_keep_the_first_date() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        let id = upsert_item(&conn, &arrival()).unwrap();
        assert!(record_event(&conn, id, "steam", EventKind::Play, 60.0, Some("2025-01-01T00:00:00Z"), None, &Value::Null).unwrap());
        assert!(record_event(&conn, id, "steam", EventKind::Play, 600.0, Some("2025-06-01T00:00:00Z"), None, &Value::Null).unwrap());
        assert!(!record_event(&conn, id, "steam", EventKind::Play, 600.0, Some("2025-06-01T00:00:00Z"), None, &Value::Null).unwrap());
        let (value, at, first): (f64, String, String) = conn.query_row("SELECT value, at, first_at FROM user_events WHERE item_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(value, 600.0);
        assert!(at.starts_with("2025-06") && first.starts_with("2025-01"));
    }

    #[test]
    fn lists_add_reorder_and_protect_built_ins() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        let a = upsert_item(&conn, &arrival()).unwrap();
        let b = upsert_item(&conn, &ItemDraft::new(Media::Game, "Outer Wilds").id("steam", 753640)).unwrap();
        let wl = list_id(&conn, "watchlist").unwrap();
        assert!(add_to_list(&conn, wl, a, "local").unwrap());
        assert!(!add_to_list(&conn, wl, a, "local").unwrap());
        assert!(add_to_list(&conn, wl, b, "pending").unwrap());
        reorder_list(&conn, wl, &[a, b]).unwrap();
        let entries = list_entries(&conn, wl).unwrap();
        assert_eq!(entries.iter().map(|e| e.card.id).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(entries[1].sync_state, "pending");
        assert!(entries[0].card.saved);
        assert!(delete_list(&conn, wl).is_err());
        let custom = create_list(&conn, "Rainy Sunday").unwrap();
        let again = create_list(&conn, "Rainy Sunday").unwrap();
        assert_ne!(custom, again);
        delete_list(&conn, custom).unwrap();
    }

    #[test]
    fn notifications_are_stored_once() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        assert!(add_notification(&conn, "release", "release:1:2026-01-01", "Out today", "Arrival", None).unwrap().is_some());
        assert!(add_notification(&conn, "release", "release:1:2026-01-01", "Out today", "Arrival", None).unwrap().is_none());
        assert_eq!(notifications(&conn, 10).unwrap().len(), 1);
    }

    #[test]
    fn settings_round_trip() {
        let db = Db::memory().unwrap();
        set_setting(&db, "region", &"US").unwrap();
        assert_eq!(get_setting::<String>(&db, "region").as_deref(), Some("US"));
        assert_eq!(setting_or(&db, "missing", 6), 6);
    }
}
