//! "Build your taste" and "Your ratings": letting people shape their profile directly rather
//! than only through history. Genres, themes and creators can be loved, liked, disliked or
//! blocked (`taste_overrides`), and titles rated in bulk.

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::store;
use crate::taste::vocab;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The builder's sections, and which titles each covers.
pub const SECTIONS: [(&str, &str); 5] = [
    ("movie", "i.media = 'movie' AND NOT COALESCE(json_extract(i.data, '$.anime'), 0)"),
    ("tv", "i.media = 'tv' AND NOT COALESCE(json_extract(i.data, '$.anime'), 0)"),
    ("anime", "i.media IN ('tv', 'movie') AND json_extract(i.data, '$.anime') = 1"),
    ("game", "i.media = 'game'"),
    ("album", "i.media IN ('album', 'artist')"),
];

fn section_sql(section: &str) -> &'static str {
    SECTIONS.iter().find(|(s, _)| *s == section).map(|(_, w)| *w).unwrap_or("1")
}

/// The user's choice for each feature: "love" | "like" | "dislike" | "block".
fn choices(conn: &rusqlite::Connection) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT feature, mode FROM taste_overrides")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.filter_map(|r| r.ok()).map(|(f, m)| (f, if m == "pin" { "love".to_string() } else { m })).collect();
    Ok(rows)
}

fn weights(conn: &rusqlite::Connection) -> Result<HashMap<String, f64>> {
    let mut stmt = conn.prepare("SELECT feature, weight FROM taste_profile")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?.filter_map(|r| r.ok()).collect();
    Ok(rows)
}

fn chip(f: &str, label: String, count: i64, choice: &HashMap<String, String>, weight: &HashMap<String, f64>) -> Value {
    json!({
        "feature": f,
        "label": label,
        "count": count,
        "choice": choice.get(f),
        // How much the history already leans this way (-1..1), for a hint beside the chip.
        "learned": weight.get(f).map(|w| (w * 100.0).round() / 100.0),
    })
}

/// Everything the builder shows: genres per section, cross-media themes and creators, each with
/// the user's choice and what the history already says.
pub fn overview(ctx: &Ctx) -> Result<Value> {
    // Read before taking the connection: settings lock it too.
    let dial = crate::taste::TasteSettings::load(&ctx.db).dial;
    let conn = ctx.db.lock();
    let choice = choices(&conn)?;
    let weight = weights(&conn)?;
    let mut sections = serde_json::Map::new();
    for (section, sql) in SECTIONS {
        let mut stmt = conn.prepare(&format!(
            "SELECT f.feature, count(*) n FROM item_features f JOIN items i ON i.id = f.item_id WHERE {sql} AND f.feature LIKE 'g:%' AND f.feature != 'g:animation'
             GROUP BY f.feature HAVING n >= 2 ORDER BY n DESC LIMIT 24"
        ))?;
        let genres: Vec<Value> = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.filter_map(|r| r.ok()).map(|(f, n)| chip(&f, vocab::label(&f), n, &choice, &weight)).collect();
        let rated: i64 = conn.query_row(&format!("SELECT count(DISTINCT e.item_id) FROM user_events e JOIN items i ON i.id = e.item_id WHERE e.kind = 'rating' AND {sql}"), [], |r| r.get(0))?;
        sections.insert(section.into(), json!({ "genres": genres, "rated": rated }));
    }
    let themes: Vec<Value> = {
        let mut stmt = conn.prepare("SELECT feature, count(*) n FROM item_features WHERE feature LIKE 't:%' GROUP BY feature HAVING n >= 3 ORDER BY n DESC LIMIT 60")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.filter_map(|r| r.ok()).map(|(f, n)| chip(&f, vocab::label(&f), n, &choice, &weight)).collect();
        rows
    };
    // Creators the user has a history with first, then the busiest in the catalog.
    let creators = creators(&conn, None, 36, &choice, &weight)?;
    // Choices on features that no longer show above (a theme picked before it became rare).
    let other: Vec<Value> = choice
        .keys()
        .filter(|f| !f.starts_with("c:"))
        .filter(|f| !themes.iter().any(|t| t["feature"] == **f) && !sections.values().any(|s| s["genres"].as_array().is_some_and(|g| g.iter().any(|x| x["feature"] == **f))))
        .map(|f| chip(f, vocab::label(f), 0, &choice, &weight))
        .collect();
    let counts = json!({
        "loved": choice.values().filter(|m| *m == "love").count(),
        "liked": choice.values().filter(|m| *m == "like").count(),
        "disliked": choice.values().filter(|m| *m == "dislike").count(),
        "blocked": choice.values().filter(|m| *m == "block").count(),
    });
    Ok(json!({ "sections": sections, "themes": themes, "creators": creators, "other": other, "counts": counts, "dial": dial }))
}

/// Creators (directors, developers, artists, studios…) with their display names. `q` searches.
fn creators(conn: &rusqlite::Connection, q: Option<&str>, limit: usize, choice: &HashMap<String, String>, weight: &HashMap<String, f64>) -> Result<Vec<Value>> {
    let filter = if q.is_some() { "AND json_extract(c.value, '$.name') LIKE ?1" } else { "AND ?1 IS NULL" };
    let like = q.map(|q| format!("%{}%", q.trim()));
    let mut stmt = conn.prepare(&format!(
        "SELECT json_extract(c.value, '$.name') name, json_extract(c.value, '$.role') role, count(DISTINCT i.id) n,
                sum(EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('rating', 'watched', 'play', 'like'))) mine
         FROM items i, json_each(i.data, '$.creators') c
         WHERE json_extract(c.value, '$.role') IN ('director', 'writer', 'creator', 'developer', 'publisher', 'studio', 'artist', 'network') {filter}
         GROUP BY lower(name) ORDER BY mine DESC, n DESC LIMIT {limit}"
    ))?;
    let rows: Vec<(String, String, i64, i64)> = stmt.query_map([like.as_deref()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.filter_map(|r| r.ok()).collect();
    Ok(rows
        .into_iter()
        .map(|(name, role, n, mine)| {
            let f = format!("c:{}", slug(&name));
            let mut v = chip(&f, name, n, choice, weight);
            v["role"] = json!(role);
            v["yours"] = json!(mine);
            v
        })
        .collect())
}

pub fn search_creators(ctx: &Ctx, q: &str) -> Result<Vec<Value>> {
    let conn = ctx.db.lock();
    let choice = choices(&conn)?;
    let weight = weights(&conn)?;
    creators(&conn, Some(q), 24, &choice, &weight)
}

/// Titles to rate in a section: what you've watched or played but not rated first, then the most
/// popular released titles you haven't told Marquee about at all.
pub fn to_rate(ctx: &Ctx, section: &str, skip: &[i64], limit: usize) -> Result<Vec<Card>> {
    let conn = ctx.db.lock();
    let sql = section_sql(section);
    let not_skipped = if skip.is_empty() { String::new() } else { format!("AND i.id NOT IN ({})", skip.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")) };
    let unrated = "NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('rating', 'dismiss'))";
    let mut out = store::cards(
        &conn,
        &format!(
            "WHERE {sql} AND i.media != 'artist' AND i.poster IS NOT NULL AND {unrated} {not_skipped}
               AND EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'play', 'owned', 'like'))
             ORDER BY (SELECT max(e.at) FROM user_events e WHERE e.item_id = i.id) DESC LIMIT {limit}"
        ),
        &[],
    )?;
    if out.len() < limit {
        let more = store::cards(
            &conn,
            &format!(
                "WHERE {sql} AND i.media != 'artist' AND i.poster IS NOT NULL AND {unrated} {not_skipped} AND i.release_date <= date('now')
                   AND NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id)
                 ORDER BY i.popularity DESC LIMIT {}",
                limit - out.len()
            ),
            &[],
        )?;
        out.extend(more);
    }
    Ok(out)
}

/// Every title the user has rated, with a histogram of the stars given.
pub fn ratings(ctx: &Ctx, section: &str, sort: &str, offset: usize, limit: usize) -> Result<Value> {
    let conn = ctx.db.lock();
    let sql = if section == "all" || section.is_empty() { "1" } else { section_sql(section) };
    let order = match sort {
        "high" => "r.value DESC, r.at DESC",
        "low" => "r.value ASC, r.at DESC",
        "title" => "i.sort_title ASC",
        _ => "r.at DESC",
    };
    // One rating per title: Marquee's own wins over a synced one.
    let rated = "(SELECT e.item_id, e.value, e.at, e.source FROM user_events e WHERE e.kind = 'rating'
                    AND e.rowid = (SELECT x.rowid FROM user_events x WHERE x.item_id = e.item_id AND x.kind = 'rating' ORDER BY x.source = 'marquee' DESC, x.at DESC LIMIT 1))";
    let ids: Vec<(i64, f64, String, String)> = {
        let mut stmt = conn.prepare(&format!("SELECT r.item_id, r.value, r.at, r.source FROM {rated} r JOIN items i ON i.id = r.item_id WHERE {sql} ORDER BY {order} LIMIT {} OFFSET {offset}", limit + 1))?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.filter_map(|r| r.ok()).collect();
        rows
    };
    let more = ids.len() > limit;
    let items: Vec<Value> = ids.into_iter().take(limit).filter_map(|(id, stars, at, source)| store::card(&conn, id).ok().flatten().map(|c| json!({ "card": c, "stars": stars, "at": at, "source": source }))).collect();
    let mut histogram = vec![0i64; 10];
    let mut stmt = conn.prepare(&format!("SELECT r.value, count(*) FROM {rated} r JOIN items i ON i.id = r.item_id WHERE {sql} GROUP BY r.value"))?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?)))? {
        let (v, n) = row?;
        let slot = ((v * 2.0).round() as i64 - 1).clamp(0, 9) as usize;
        histogram[slot] += n;
    }
    let total: i64 = histogram.iter().sum();
    let average = if total > 0 { histogram.iter().enumerate().map(|(i, n)| (i as f64 + 1.0) / 2.0 * *n as f64).sum::<f64>() / total as f64 } else { 0.0 };
    let per_section: serde_json::Map<String, Value> = SECTIONS
        .iter()
        .map(|(s, w)| {
            let n: i64 = conn.query_row(&format!("SELECT count(*) FROM {rated} r JOIN items i ON i.id = r.item_id WHERE {w}"), [], |r| r.get(0)).unwrap_or(0);
            (s.to_string(), json!(n))
        })
        .collect();
    Ok(json!({
        "items": items,
        "next": if more { Some(offset + limit) } else { None },
        "histogram": histogram,
        "total": total,
        "average": (average * 100.0).round() / 100.0,
        "sections": per_section,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    fn seed(t: &crate::ctx::testing::TestCtx) -> (i64, i64) {
        let conn = t.ctx.db.lock();
        let mut a = ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 1);
        a.genres = vec!["Science Fiction".into(), "Drama".into()];
        a.creators = vec![Creator::new("Denis Villeneuve", "director")];
        a.poster = Some("https://image.tmdb.org/t/p/w500/a.jpg".into());
        a.release_date = Some("2016-11-11".into());
        let arrival = store::upsert_item(&conn, &a).unwrap();
        let mut d = ItemDraft::new(Media::Movie, "Dune").id("tmdb", 2);
        d.genres = vec!["Science Fiction".into()];
        d.creators = vec![Creator::new("Denis Villeneuve", "director")];
        d.poster = Some("https://image.tmdb.org/t/p/w500/d.jpg".into());
        d.release_date = Some("2021-10-22".into());
        d.popularity = 90.0;
        let dune = store::upsert_item(&conn, &d).unwrap();
        (arrival, dune)
    }

    #[test]
    fn the_overview_lists_genres_and_creators_with_choices() {
        let t = ctx();
        let (arrival, _) = seed(&t);
        {
            let conn = t.ctx.db.lock();
            store::record_event(&conn, arrival, "letterboxd", EventKind::Rating, 5.0, None, None, &Value::Null).unwrap();
            conn.execute("INSERT INTO taste_overrides (feature, mode) VALUES ('g:sci-fi', 'pin'), ('c:denis-villeneuve', 'like')", []).unwrap();
        }
        let v = overview(&t.ctx).unwrap();
        let genres = v["sections"]["movie"]["genres"].as_array().unwrap();
        let scifi = genres.iter().find(|g| g["feature"] == "g:sci-fi").unwrap();
        assert_eq!(scifi["choice"], "love");
        assert_eq!(scifi["count"], 2);
        let dv = v["creators"].as_array().unwrap().iter().find(|c| c["label"] == "Denis Villeneuve").unwrap();
        assert_eq!(dv["choice"], "like");
        assert_eq!(dv["yours"], 1);
        assert_eq!(v["counts"]["loved"], 1);
        assert_eq!(v["sections"]["movie"]["rated"], 1);
        assert_eq!(search_creators(&t.ctx, "villen").unwrap().len(), 1);
    }

    #[test]
    fn likes_and_dislikes_move_the_profile_without_banning() {
        let t = ctx();
        let (arrival, _) = seed(&t);
        {
            let conn = t.ctx.db.lock();
            store::record_event(&conn, arrival, "letterboxd", EventKind::Rating, 5.0, None, None, &Value::Null).unwrap();
            conn.execute("INSERT INTO taste_overrides (feature, mode) VALUES ('g:drama', 'dislike'), ('t:cozy', 'like')", []).unwrap();
        }
        crate::taste::profile::rebuild(&t.ctx.db).unwrap();
        let w = weights(&t.ctx.db.lock()).unwrap();
        assert!(w["g:drama"] < -0.5, "disliked: {:?}", w.get("g:drama"));
        assert!(w["t:cozy"] >= 0.55, "liked without any history");
        assert!(w["g:sci-fi"] > 0.0, "the rest of the history stands");
    }

    #[test]
    fn ratings_list_one_rating_per_title_with_a_histogram() {
        let t = ctx();
        let (arrival, dune) = seed(&t);
        {
            let conn = t.ctx.db.lock();
            store::record_event(&conn, arrival, "letterboxd", EventKind::Rating, 4.0, Some("2025-01-01T00:00:00Z"), None, &Value::Null).unwrap();
            store::record_event(&conn, arrival, "marquee", EventKind::Rating, 5.0, Some("2025-02-01T00:00:00Z"), None, &Value::Null).unwrap();
            store::record_event(&conn, dune, "trakt", EventKind::Rating, 3.5, Some("2025-03-01T00:00:00Z"), None, &Value::Null).unwrap();
        }
        let v = ratings(&t.ctx, "all", "high", 0, 50).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["items"][0]["card"]["title"], "Arrival");
        assert_eq!(v["items"][0]["stars"], 5.0, "Marquee's own rating wins");
        assert_eq!(v["histogram"][9], 1);
        assert_eq!(v["histogram"][6], 1);
        assert_eq!(v["average"], 4.25);
        assert_eq!(v["sections"]["movie"], 2);
        let unrated = to_rate(&t.ctx, "movie", &[], 10).unwrap();
        assert!(unrated.is_empty(), "both are rated");
    }
}
