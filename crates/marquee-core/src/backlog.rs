//! The game backlog: games saved to Play Later or owned and barely played, with how long each
//! takes to beat (IGDB's player-reported times), and a "start next" pick.

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::store;
use serde::Deserialize;
use serde_json::{json, Value};

const NOT_FINISHED: &str = "NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'rating'))";
const IN_PLAY_LATER: &str = "EXISTS (SELECT 1 FROM list_items l JOIN lists ls ON ls.id = l.list_id WHERE l.item_id = i.id AND ls.slug = 'play-later')";
/// Owned games with under an hour of playtime count as unplayed.
const UNPLAYED: &str = "(EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'owned')
    AND COALESCE((SELECT max(e.value) FROM user_events e WHERE e.item_id = i.id AND e.kind = 'play'), 0) < 60)";
const MAIN: &str = "json_extract(i.data, '$.ttb.main')";

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Query {
    /// "all" | "play_later" | "unplayed"
    pub filter: String,
    /// "next" | "shortest" | "longest" | "match" | "sale"
    pub sort: String,
}

fn where_sql(filter: &str) -> String {
    let which = match filter {
        "play_later" => IN_PLAY_LATER.to_string(),
        "unplayed" => UNPLAYED.to_string(),
        _ => format!("({IN_PLAY_LATER} OR {UNPLAYED})"),
    };
    format!("WHERE i.media = 'game' AND {NOT_FINISHED} AND {which}")
}

/// "Start next" ranking: a good match, finishable soon, cheaper today is better. Unreleased last.
const NEXT_ORDER: &str = "(i.release_date IS NULL OR i.release_date > date('now')) ASC,
    COALESCE(s.match_pct, 50) / 100.0
      + CASE WHEN json_extract(i.data, '$.ttb.main') IS NULL THEN 0.03 WHEN json_extract(i.data, '$.ttb.main') <= 12 THEN 0.18 WHEN json_extract(i.data, '$.ttb.main') <= 30 THEN 0.1 WHEN json_extract(i.data, '$.ttb.main') <= 60 THEN 0.03 ELSE -0.05 END
      + CASE WHEN COALESCE(json_extract(i.data, '$.discount'), 0) > 0 THEN 0.04 ELSE 0 END DESC";

fn order(sort: &str) -> String {
    match sort {
        "shortest" => format!("{MAIN} IS NULL, {MAIN} ASC, s.score DESC"),
        "longest" => format!("{MAIN} IS NULL, {MAIN} DESC, s.score DESC"),
        "match" => "s.match_pct IS NULL, s.match_pct DESC".into(),
        "sale" => "COALESCE(json_extract(i.data, '$.discount'), 0) DESC, s.score DESC".into(),
        _ => NEXT_ORDER.into(),
    }
}

/// Why the "start next" game was picked, in one sentence.
fn next_reason(c: &Card) -> String {
    let main = c.meta["ttb"]["main"].as_f64();
    let mut parts = vec![];
    if let Some(m) = c.match_pct {
        parts.push(format!("a {m}% match"));
    }
    match main {
        Some(h) if h <= 12.0 => parts.push(format!("you can finish it in about {}", hours(h))),
        Some(h) => parts.push(format!("about {} to beat", hours(h))),
        None => {}
    }
    if let Some(d) = c.meta["discount"].as_i64().filter(|d| *d > 0) {
        parts.push(format!("{d}% off right now"));
    }
    let mut s = parts.join(", ");
    if s.is_empty() {
        s = "next up on your list".into();
    }
    let mut chars = s.chars();
    chars.next().map(|f| f.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or(s) + "."
}

pub fn hours(h: f64) -> String {
    if h < 1.0 {
        "under an hour".into()
    } else if (h - h.round()).abs() < 0.01 {
        format!("{} hour{}", h as i64, if h as i64 == 1 { "" } else { "s" })
    } else {
        format!("{h:.1} hours")
    }
}

pub fn feed(ctx: &Ctx, q: &Query) -> Result<Value> {
    let conn = ctx.db.lock();
    let tail = format!("{} ORDER BY {} LIMIT 500", where_sql(&q.filter), order(&q.sort));
    let cards = store::cards(&conn, &tail, &[])?;
    let next = store::cards(&conn, &format!("{} AND i.release_date <= date('now') ORDER BY {NEXT_ORDER} LIMIT 1", where_sql(&q.filter)), &[])?.into_iter().next();
    let count = |filter: &str| conn.query_row(&format!("SELECT count(*) FROM items i {}", where_sql(filter)), [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    let known: Vec<f64> = cards.iter().filter_map(|c| c.meta["ttb"]["main"].as_f64()).collect();
    Ok(json!({
        "cards": cards,
        "next": next.as_ref().map(|c| json!({ "card": c, "reason": next_reason(c) })),
        "counts": { "all": count("all"), "play_later": count("play_later"), "unplayed": count("unplayed") },
        "hours": known.iter().sum::<f64>().round(),
        "with_length": known.len(),
        "lengths_available": crate::catalog::igdb::configured(ctx),
    }))
}

/// Looks up how long backlog games take to beat, for games not checked in the last 30 days.
pub async fn refresh_lengths(ctx: &Ctx) -> Result<usize> {
    if !crate::catalog::igdb::configured(ctx) {
        return Ok(0);
    }
    // (item, igdb id, steam app id)
    let rows: Vec<(i64, Option<String>, Option<String>)> = {
        let conn = ctx.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id,
                    (SELECT ext_id FROM item_ids x WHERE x.item_id = i.id AND x.source = 'igdb' LIMIT 1),
                    (SELECT ext_id FROM item_ids x WHERE x.item_id = i.id AND x.source = 'steam' LIMIT 1)
             FROM items i {} AND (json_extract(i.data, '$.ttb_checked') IS NULL OR json_extract(i.data, '$.ttb_checked') < ?1)
             LIMIT 300",
            where_sql("all")
        ))?;
        let rows = stmt.query_map([days_from_today(-30)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    if rows.is_empty() {
        return Ok(0);
    }
    let need_map: Vec<String> = rows.iter().filter(|r| r.1.is_none()).filter_map(|r| r.2.clone()).collect();
    let mapped = if need_map.is_empty() { Default::default() } else { crate::catalog::igdb::steam_to_igdb(ctx, &need_map).await? };
    let pairs: Vec<(i64, String)> = rows.iter().filter_map(|(id, igdb, steam)| igdb.clone().or_else(|| steam.as_ref().and_then(|s| mapped.get(s).cloned())).map(|g| (*id, g))).collect();
    let ids: Vec<String> = pairs.iter().map(|p| p.1.clone()).collect();
    let times = if ids.is_empty() { Default::default() } else { crate::catalog::igdb::time_to_beat(ctx, &ids).await? };
    let conn = ctx.db.lock();
    let stamp = today();
    let mut found = 0;
    for (id, _, _) in &rows {
        let t = pairs.iter().find(|p| p.0 == *id).and_then(|p| times.get(&p.1));
        if let Some(t) = t {
            conn.execute("UPDATE items SET data = json_set(data, '$.ttb', json(?1), '$.ttb_checked', ?2) WHERE id = ?3", rusqlite::params![t.to_string(), stamp, id])?;
            found += 1;
        } else {
            conn.execute("UPDATE items SET data = json_set(data, '$.ttb_checked', ?1) WHERE id = ?2", rusqlite::params![stamp, id])?;
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn the_backlog_holds_play_later_and_barely_played_games_and_picks_a_short_great_match() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            let later = store::list_id(&conn, "play-later").unwrap();
            let add = |title: &str, n: i64, ttb: Option<f64>, released: bool| {
                let mut d = ItemDraft::new(Media::Game, title).id("steam", n);
                d.release_date = Some(if released { days_from_today(-400) } else { days_from_today(90) });
                if let Some(h) = ttb {
                    d.set("ttb", json!({ "main": h }));
                }
                store::upsert_item(&conn, &d).unwrap()
            };
            let short = add("Short Gem", 1, Some(8.0), true);
            let long = add("Huge RPG", 2, Some(120.0), true);
            let future = add("Not Out", 3, Some(5.0), false);
            let barely = add("Bought Once", 4, None, true);
            let played = add("Played A Lot", 5, Some(10.0), true);
            let beaten = add("Beaten", 6, Some(6.0), true);
            for id in [short, long, future, beaten] {
                store::add_to_list(&conn, later, id, "local").unwrap();
            }
            store::record_event(&conn, barely, "steam", EventKind::Owned, 1.0, None, None, &Value::Null).unwrap();
            store::record_event(&conn, barely, "steam", EventKind::Play, 20.0, None, None, &Value::Null).unwrap();
            store::record_event(&conn, played, "steam", EventKind::Owned, 1.0, None, None, &Value::Null).unwrap();
            store::record_event(&conn, played, "steam", EventKind::Play, 900.0, None, None, &Value::Null).unwrap();
            store::record_event(&conn, beaten, "marquee", EventKind::Watched, 1.0, None, None, &Value::Null).unwrap();
            for (id, pct) in [(short, 90), (long, 95), (future, 99), (barely, 60)] {
                conn.execute("INSERT INTO scores (item_id, score, match_pct, why, computed_at) VALUES (?1, ?2, ?3, '[]', '')", rusqlite::params![id, pct as f64 / 100.0, pct]).unwrap();
            }
        }
        let v = feed(&t.ctx, &Query { filter: "all".into(), sort: "shortest".into() }).unwrap();
        let titles: Vec<&str> = v["cards"].as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["Not Out", "Short Gem", "Huge RPG", "Bought Once"], "shortest first, unknown lengths last; played and beaten games are out");
        assert_eq!(v["counts"]["play_later"], 3);
        assert_eq!(v["counts"]["unplayed"], 1);
        assert_eq!(v["hours"], 133.0);
        assert_eq!(v["next"]["card"]["title"], "Short Gem", "a 90% match you can finish in 8 hours beats a 95% match that takes 120, and unreleased games aren't picked");
        assert_eq!(v["next"]["reason"], "A 90% match, you can finish it in about 8 hours.");
    }

    #[test]
    fn hours_read_naturally() {
        assert_eq!(hours(0.5), "under an hour");
        assert_eq!(hours(1.0), "1 hour");
        assert_eq!(hours(12.5), "12.5 hours");
    }
}
