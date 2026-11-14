//! Marquee Wrapped: a year in review across movies, TV, games and music.
//!
//! Watching comes from watched/rated events (with runtimes), playing from the daily Steam
//! playtime log (Steam only reports lifetime totals, so hours count from the first sync of the
//! year), and listening from Last.fm charts for the year.

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::store;
use crate::taste::vocab;
use chrono::{Datelike, TimeZone};
use serde_json::{json, Value};

/// Years with anything to review, newest first.
pub fn years(ctx: &Ctx) -> Result<Vec<i32>> {
    let conn = ctx.db.lock();
    let mut stmt = conn.prepare("SELECT DISTINCT CAST(substr(at, 1, 4) AS INTEGER) FROM user_events WHERE kind IN ('watched', 'rating', 'play', 'like') AND at IS NOT NULL ORDER BY 1 DESC")?;
    let mut years: Vec<i32> = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).filter(|y: &i32| *y > 1990).collect();
    let this = chrono::Local::now().year();
    if !years.contains(&this) {
        years.insert(0, this);
    }
    Ok(years)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Top features (genres and tags) among things the user rated well or finished in a year.
fn top_features(conn: &rusqlite::Connection, from: &str, to: &str, n: usize) -> Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT f.feature, sum(f.weight * CASE WHEN e.kind = 'rating' THEN (e.value - 2.5) ELSE 1 END)
         FROM user_events e JOIN item_features f ON f.item_id = e.item_id
         WHERE e.at >= ?1 AND e.at < ?2 AND e.kind IN ('watched', 'rating', 'play', 'like') AND (f.feature LIKE 'g:%' OR f.feature LIKE 't:%')
         GROUP BY f.feature HAVING sum(f.weight) > 0 ORDER BY 2 DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![from, to, n as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?.filter_map(|r| r.ok()).filter(|(_, w)| *w > 0.0).collect();
    Ok(rows)
}

pub async fn year(ctx: &Ctx, year: i32) -> Result<Value> {
    let from = format!("{year}-01-01");
    let to = format!("{}-01-01", year + 1);
    let mut v = {
        let conn = ctx.db.lock();
        review(&conn, year, &from, &to)?
    };

    // Listening, from Last.fm's charts for the year.
    let user = store::source_config(&ctx.db.lock(), "lastfm")["username"].as_str().map(str::to_string);
    if let Some(user) = user {
        let ts = |y: i32| chrono::Local.with_ymd_and_hms(y, 1, 1, 0, 0, 0).single().map(|d| d.timestamp()).unwrap_or(0);
        let (start, end) = (ts(year), ts(year + 1).min(chrono::Utc::now().timestamp()));
        let scrobbles = crate::catalog::lastfm::scrobbles_between(ctx, &user, start, end).await.unwrap_or(0);
        let artists = crate::catalog::lastfm::chart_between(ctx, &user, "artist", start, end).await.unwrap_or_default();
        let albums = crate::catalog::lastfm::chart_between(ctx, &user, "album", start, end).await.unwrap_or_default();
        v["music"] = json!({
            "scrobbles": scrobbles,
            "hours": (scrobbles as f64 * 3.5 / 60.0).round(),
            "top_artists": artists.iter().take(5).map(|(n, _, p)| json!({ "name": n, "plays": p })).collect::<Vec<_>>(),
            "top_albums": albums.iter().take(5).map(|(n, a, p)| json!({ "title": n, "artist": a, "plays": p })).collect::<Vec<_>>(),
            "artists_count": artists.len(),
            "source": "lastfm",
        });
        v["totals"]["hours"] = json!(v["totals"]["hours"].as_f64().unwrap_or(0.0) + v["music"]["hours"].as_f64().unwrap_or(0.0));
    }
    Ok(v)
}

fn review(conn: &rusqlite::Connection, year: i32, from: &str, to: &str) -> Result<Value> {
    let args: &[&dyn rusqlite::ToSql] = &[&from, &to];

    // Movies: each watched once per year, at its runtime (110 minutes if unknown).
    let (movies, movie_minutes): (i64, f64) = conn.query_row(
        "SELECT count(DISTINCT e.item_id), COALESCE(sum(COALESCE(i.runtime, 110)), 0) FROM user_events e JOIN items i ON i.id = e.item_id
         WHERE e.kind = 'watched' AND i.media = 'movie' AND e.at >= ?1 AND e.at < ?2",
        args,
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // TV: episodes Trakt reports as plays, at the show's episode runtime (45 minutes if unknown).
    let (shows, episodes, tv_minutes): (i64, f64, f64) = conn.query_row(
        "SELECT count(DISTINCT e.item_id), COALESCE(sum(max(1, COALESCE(json_extract(e.meta, '$.plays'), 1))), 0),
                COALESCE(sum(max(1, COALESCE(json_extract(e.meta, '$.plays'), 1)) * COALESCE(i.runtime, 45)), 0)
         FROM user_events e JOIN items i ON i.id = e.item_id WHERE e.kind = 'watched' AND i.media = 'tv' AND e.at >= ?1 AND e.at < ?2",
        args,
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    // Games: hours added in the year, from the playtime log; for days before logging began,
    // games last played in the year are listed without hours.
    let mut game_hours: Vec<(i64, f64)> = {
        let mut stmt = conn.prepare(
            "SELECT p.item_id,
                    max(p.minutes) - COALESCE((SELECT max(q.minutes) FROM playtime_log q WHERE q.item_id = p.item_id AND q.day < ?1), min(p.minutes))
             FROM playtime_log p WHERE p.day >= ?1 AND p.day < ?2 GROUP BY p.item_id",
        )?;
        let rows = stmt.query_map(args, |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)? / 60.0)))?.filter_map(|r| r.ok()).filter(|(_, h)| *h >= 0.5).collect();
        rows
    };
    game_hours.sort_by(|a, b| b.1.total_cmp(&a.1));
    let logged_since: Option<String> = conn.query_row("SELECT min(day) FROM playtime_log WHERE day >= ?1 AND day < ?2", args, |r| r.get(0)).ok().flatten();
    let played_games: i64 =
        conn.query_row("SELECT count(DISTINCT e.item_id) FROM user_events e JOIN items i ON i.id = e.item_id WHERE i.media = 'game' AND e.kind IN ('play', 'rating', 'watched') AND e.at >= ?1 AND e.at < ?2", args, |r| r.get(0))?;
    let game_total: f64 = game_hours.iter().map(|g| g.1).sum();
    let top_games: Vec<Value> = if game_hours.is_empty() {
        store::cards(
            conn,
            "JOIN user_events e ON e.item_id = i.id WHERE i.media = 'game' AND e.kind IN ('play', 'rating') AND e.at >= ?1 AND e.at < ?2 GROUP BY i.id ORDER BY max(CASE WHEN e.kind = 'rating' THEN e.value * 1000 ELSE e.value END) DESC LIMIT 5",
            args,
        )?
        .into_iter()
        .map(|c| json!({ "card": c, "hours": null }))
        .collect()
    } else {
        game_hours.iter().take(5).filter_map(|(id, h)| store::card(conn, *id).ok().flatten().map(|c| json!({ "card": c, "hours": h.round() }))).collect()
    };

    // Favourites: best rated in the year, per medium.
    let best = |media: &str| -> Result<Vec<Card>> {
        store::cards(conn, "JOIN user_events e ON e.item_id = i.id WHERE i.media = ?3 AND e.kind = 'rating' AND e.at >= ?1 AND e.at < ?2 GROUP BY i.id ORDER BY max(e.value) DESC, max(e.at) DESC LIMIT 5", &[&from, &to, &media])
    };
    let rated: i64 = conn.query_row("SELECT count(DISTINCT item_id) FROM user_events WHERE kind = 'rating' AND at >= ?1 AND at < ?2", args, |r| r.get(0))?;
    let avg_rating: Option<f64> = conn.query_row("SELECT avg(value) FROM user_events WHERE kind = 'rating' AND at >= ?1 AND at < ?2", args, |r| r.get(0))?;

    // Months: how much happened each month; the busiest one gets a callout.
    let mut months = vec![0i64; 12];
    {
        let mut stmt = conn.prepare("SELECT CAST(substr(at, 6, 2) AS INTEGER), count(*) FROM user_events WHERE kind IN ('watched', 'rating', 'play', 'like') AND at >= ?1 AND at < ?2 GROUP BY 1")?;
        for row in stmt.query_map(args, |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (m, n) = row?;
            if (1..=12).contains(&m) {
                months[(m - 1) as usize] = n;
            }
        }
    }
    let busiest = months.iter().enumerate().max_by_key(|(_, n)| **n).filter(|(_, n)| **n > 0).map(|(i, n)| json!({ "month": i + 1, "count": n }));

    // Creators the user came back to most.
    let creators: Vec<Value> = {
        let mut stmt = conn.prepare(
            "SELECT json_extract(c.value, '$.name') name, json_extract(c.value, '$.role') role, count(DISTINCT e.item_id) n
             FROM user_events e JOIN items i ON i.id = e.item_id, json_each(i.data, '$.creators') c
             WHERE e.kind IN ('watched', 'rating', 'play') AND e.at >= ?1 AND e.at < ?2 AND json_extract(c.value, '$.role') IN ('director', 'developer', 'writer', 'studio')
             GROUP BY lower(name) HAVING n >= 2 ORDER BY n DESC, name LIMIT 6",
        )?;
        let rows = stmt.query_map(args, |r| Ok(json!({ "name": r.get::<_, String>(0)?, "role": r.get::<_, String>(1)?, "count": r.get::<_, i64>(2)? })))?.filter_map(|r| r.ok()).collect();
        rows
    };

    // Taste: this year's top genres and themes, and which are new compared with last year.
    let this_year = top_features(conn, from, to, 8)?;
    let last_year: Vec<String> = top_features(conn, &format!("{}-01-01", year - 1), from, 15)?.into_iter().map(|(f, _)| f).collect();
    let max_w = this_year.first().map(|f| f.1).unwrap_or(1.0).max(0.01);
    let taste: Vec<Value> = this_year.iter().map(|(f, w)| json!({ "feature": f, "label": vocab::label(f), "weight": round1(w / max_w), "new": !last_year.is_empty() && !last_year.contains(f) })).collect();

    // The most adventurous thing they loved: rated 4+ but a low match.
    let adventurous =
        store::cards(conn, "JOIN user_events e ON e.item_id = i.id WHERE e.kind = 'rating' AND e.value >= 4 AND e.at >= ?1 AND e.at < ?2 AND s.match_pct IS NOT NULL GROUP BY i.id ORDER BY s.match_pct ASC LIMIT 1", args)?.into_iter().next();
    let first = store::cards(conn, "JOIN user_events e ON e.item_id = i.id WHERE e.kind IN ('watched', 'rating') AND e.at >= ?1 AND e.at < ?2 AND i.media != 'artist' GROUP BY i.id ORDER BY min(e.at) ASC LIMIT 1", args)?.into_iter().next();

    let watch_hours = (movie_minutes + tv_minutes) / 60.0;
    Ok(json!({
        "year": year,
        "complete": year < chrono::Local::now().year(),
        "totals": {
            "hours": (watch_hours + game_total).round(),
            "titles": movies + shows + played_games,
            "rated": rated,
            "avg_rating": avg_rating.map(round1),
        },
        "movies": { "count": movies, "hours": (movie_minutes / 60.0).round(), "best": best("movie")? },
        "tv": { "shows": shows, "episodes": episodes as i64, "hours": (tv_minutes / 60.0).round(), "best": best("tv")? },
        "games": { "count": played_games, "hours": game_total.round(), "top": top_games, "best": best("game")?, "hours_since": logged_since },
        "music": { "best": best("album")?, "source": null },
        "months": months,
        "busiest": busiest,
        "creators": creators,
        "taste": taste,
        "adventurous": adventurous,
        "first": first,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;
    use serde_json::Value;

    #[tokio::test]
    async fn a_year_counts_watching_playing_and_favourites() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            let mut m = ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 1);
            m.runtime = Some(116);
            m.creators = vec![Creator::new("Denis Villeneuve", "director")];
            let arrival = store::upsert_item(&conn, &m).unwrap();
            store::record_event(&conn, arrival, "letterboxd", EventKind::Watched, 1.0, Some("2025-03-04T20:00:00Z"), None, &Value::Null).unwrap();
            store::record_event(&conn, arrival, "letterboxd", EventKind::Rating, 5.0, Some("2025-03-04T20:00:00Z"), None, &Value::Null).unwrap();
            let mut s = ItemDraft::new(Media::Tv, "Severance").id("tmdb", 2);
            s.runtime = Some(50);
            let sev = store::upsert_item(&conn, &s).unwrap();
            store::record_event(&conn, sev, "trakt", EventKind::Watched, 1.0, Some("2025-03-20T20:00:00Z"), None, &json!({ "plays": 9 })).unwrap();
            let g = store::upsert_item(&conn, &ItemDraft::new(Media::Game, "Hades II").id("steam", 3)).unwrap();
            store::record_event(&conn, g, "steam", EventKind::Play, 1800.0, Some("2025-06-01T20:00:00Z"), None, &Value::Null).unwrap();
            for (day, minutes) in [("2024-12-30", 600.0), ("2025-02-01", 900.0), ("2025-06-01", 1800.0)] {
                conn.execute("INSERT INTO playtime_log (item_id, day, minutes) VALUES (?1, ?2, ?3)", rusqlite::params![g, day, minutes]).unwrap();
            }
            // Last year doesn't count.
            let mut o = ItemDraft::new(Media::Movie, "Old").id("tmdb", 4);
            o.creators = vec![Creator::new("Denis Villeneuve", "director")];
            let old = store::upsert_item(&conn, &o).unwrap();
            store::record_event(&conn, old, "letterboxd", EventKind::Watched, 1.0, Some("2024-05-01T20:00:00Z"), None, &Value::Null).unwrap();
            let mut d = ItemDraft::new(Media::Movie, "Dune").id("tmdb", 5);
            d.creators = vec![Creator::new("Denis Villeneuve", "director")];
            let dune = store::upsert_item(&conn, &d).unwrap();
            store::record_event(&conn, dune, "letterboxd", EventKind::Watched, 1.0, Some("2025-09-04T20:00:00Z"), None, &Value::Null).unwrap();
        }
        let v = year(&t.ctx, 2025).await.unwrap();
        assert_eq!(v["movies"]["count"], 2);
        assert_eq!(v["movies"]["hours"], 4.0, "116 + 110 (unknown runtime) minutes");
        assert_eq!(v["creators"][0]["name"], "Denis Villeneuve");
        assert_eq!(v["creators"][0]["count"], 2, "only this year's films count");
        assert_eq!(v["tv"]["episodes"], 9);
        assert_eq!(v["tv"]["hours"], 8.0, "9 episodes x 50 minutes");
        assert_eq!(v["games"]["hours"], 20.0, "1800 - 600 minutes played in 2025");
        assert_eq!(v["games"]["top"][0]["card"]["title"], "Hades II");
        assert_eq!(v["movies"]["best"][0]["title"], "Arrival");
        assert_eq!(v["busiest"]["month"], 3);
        assert_eq!(v["first"]["title"], "Arrival");
        assert_eq!(v["totals"]["hours"], 31.0, "(116 + 110 + 9 x 50) minutes + 20 hours played");
        assert!(v["complete"].as_bool().unwrap());
        assert!(years(&t.ctx).unwrap().contains(&2024));
    }
}
