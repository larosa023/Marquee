//! Works out what's worth telling the user about. Notifications are stored first (that's the
//! in-app notification center); the app then shows the new ones as Windows toasts, unless it's
//! quiet hours, in which case they wait.

use crate::ctx::Ctx;
use crate::db::Db;
use crate::error::Result;
use crate::model::*;
use crate::store;
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifySettings {
    pub enabled: bool,
    pub releases: bool,
    pub seasons: bool,
    pub albums: bool,
    pub sales: bool,
    /// Minimum discount (percent) worth a sale alert.
    pub sale_threshold: i64,
    pub digest: bool,
    /// 0 = Monday ... 6 = Sunday.
    pub digest_day: u32,
    pub quiet_enabled: bool,
    /// "HH:MM", local time.
    pub quiet_from: String,
    pub quiet_to: String,
}

impl Default for NotifySettings {
    fn default() -> Self {
        NotifySettings { enabled: true, releases: true, seasons: true, albums: true, sales: true, sale_threshold: 25, digest: true, digest_day: 4, quiet_enabled: true, quiet_from: "22:00".into(), quiet_to: "08:00".into() }
    }
}

impl NotifySettings {
    pub fn load(db: &Db) -> Self {
        store::get_setting(db, "notifications").unwrap_or_default()
    }
}

fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Is `now` (minutes since local midnight) inside the quiet window? The window may cross midnight.
pub fn is_quiet(s: &NotifySettings, now_minutes: u32) -> bool {
    if !s.quiet_enabled {
        return false;
    }
    let (Some(from), Some(to)) = (minutes(&s.quiet_from), minutes(&s.quiet_to)) else { return false };
    if from == to {
        false
    } else if from < to {
        (from..to).contains(&now_minutes)
    } else {
        now_minutes >= from || now_minutes < to
    }
}

/// Notifications that should be shown as toasts right now. Empty during quiet hours; they
/// stay queued and are returned once the window ends.
pub fn toastable(db: &Db) -> Result<Vec<Notification>> {
    let s = NotifySettings::load(db);
    let now = chrono::Local::now();
    if !s.enabled || is_quiet(&s, now.hour() * 60 + now.minute()) {
        return Ok(vec![]);
    }
    let conn = db.lock();
    let list = store::untoasted(&conn)?;
    store::mark_toasted(&conn, &list.iter().map(|n| n.id).collect::<Vec<_>>())?;
    // After a long time away, don't flood the screen: toast the newest few.
    Ok(list.into_iter().rev().take(4).rev().collect())
}

fn price_text(data: &Value) -> String {
    data["price"]["text"].as_str().map(|t| format!(" ({t})")).unwrap_or_default()
}

/// Looks at the catalog and the user's lists and follows, and stores anything new worth an alert.
/// Safe to call often: every notification has a dedupe key.
pub fn generate(ctx: &Ctx) -> Result<usize> {
    let s = NotifySettings::load(&ctx.db);
    if !s.enabled {
        return Ok(0);
    }
    let today = today();
    let recent = days_from_today(-2);
    let conn = ctx.db.lock();
    let mut made = 0;
    // "Cares about" = followed, saved to a list, or wishlisted.
    const CARES: &str = "(EXISTS (SELECT 1 FROM follows f WHERE f.item_id = i.id AND f.remind = 1)
        OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id)
        OR EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'wishlist'))";

    if s.releases {
        let sql = format!(
            "SELECT i.id, i.media, i.title, i.release_date, i.data FROM items i
             WHERE i.media IN ('movie', 'game', 'album') AND i.release_date BETWEEN ?1 AND ?2 AND COALESCE(json_extract(i.data, '$.date_precision'), 10) >= 10 AND {CARES}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(i64, String, String, String, String)> = stmt.query_map([&recent, &today], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<rusqlite::Result<_>>()?;
        for (id, media, title, date, data) in rows {
            let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            let when = if date == today { "Out today" } else { "Just released" };
            let body = match media.as_str() {
                "game" => format!("{title} is out now{}.", price_text(&data)),
                "album" => format!("{title} by {} is out now.", data["artist"].as_str().unwrap_or("an artist you saved")),
                _ => format!("{title} is out now."),
            };
            if store::add_notification(&conn, "release", &format!("release:{id}:{date}"), when, &body, Some(id))?.is_some() {
                made += 1;
            }
        }
    }

    if s.seasons {
        // Shows the user watches, saved or follows, whose next episode airs today.
        let mut stmt = conn.prepare(
            "SELECT i.id, i.title, i.data FROM items i
             WHERE i.media = 'tv' AND json_extract(i.data, '$.next_episode.air_date') BETWEEN ?1 AND ?2
               AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'rating', 'like', 'wishlist', 'play'))
                 OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id)
                 OR EXISTS (SELECT 1 FROM follows f WHERE f.item_id = i.id))",
        )?;
        let rows: Vec<(i64, String, String)> = stmt.query_map([&recent, &today], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        for (id, title, data) in rows {
            let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            let ep = &data["next_episode"];
            let (season, episode) = (ep["season"].as_i64().unwrap_or(0), ep["episode"].as_i64().unwrap_or(0));
            let (heading, body) = if episode == 1 && season == 1 {
                ("Premiere", format!("{title} premieres today."))
            } else if episode == 1 {
                ("New season", format!("Season {season} of {title} starts today."))
            } else if ep["finale"] == true {
                ("Season finale", format!("The season {season} finale of {title} airs today."))
            } else {
                continue; // ordinary episodes are on the calendar, not in your notifications
            };
            if store::add_notification(&conn, "season", &format!("season:{id}:{season}:{episode}"), heading, &body, Some(id))?.is_some() {
                made += 1;
            }
        }
    }

    if s.albums {
        // New music from artists the user follows or plays, even if the album itself isn't saved.
        let mut stmt = conn.prepare(
            "SELECT i.id, i.title, i.release_date, i.data FROM items i
             WHERE i.media = 'album' AND i.release_date BETWEEN ?1 AND ?2 AND COALESCE(json_extract(i.data, '$.date_precision'), 10) >= 10
               AND (EXISTS (SELECT 1 FROM follows f WHERE f.kind = 'artist' AND lower(f.name) = lower(json_extract(i.data, '$.artist')))
                 OR EXISTS (SELECT 1 FROM items a JOIN user_events e ON e.item_id = a.id
                            WHERE a.media = 'artist' AND lower(a.title) = lower(json_extract(i.data, '$.artist')) AND e.kind IN ('like', 'pin') ))",
        )?;
        let rows: Vec<(i64, String, String, String)> = stmt.query_map([&days_from_today(-4), &today], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
        for (id, title, date, data) in rows {
            let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            let artist = data["artist"].as_str().unwrap_or("An artist you follow");
            let kind = match data["album_type"].as_str() {
                Some("ep") => "EP",
                Some("single") => "single",
                _ => "album",
            };
            // Same dedupe key as the release alert, so one album never produces two toasts.
            if store::add_notification(&conn, "album", &format!("release:{id}:{date}"), &format!("New {kind} from {artist}"), &format!("{title} is out now."), Some(id))?.is_some() {
                made += 1;
            }
        }
    }

    if s.sales {
        let mut stmt = conn.prepare(
            "SELECT i.id, i.title, i.data FROM items i
             WHERE i.media = 'game' AND COALESCE(json_extract(i.data, '$.discount'), 0) >= ?1
               AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'wishlist') OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id))
               AND NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'owned')",
        )?;
        let rows: Vec<(i64, String, String)> = stmt.query_map([s.sale_threshold], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let week = chrono::Local::now().iso_week();
        for (id, title, data) in rows {
            let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            let discount = data["discount"].as_i64().unwrap_or(0);
            let low = data["low"]["cents"].as_i64().zip(data["price"]["cents"].as_i64()).map(|(low, now)| now <= low).unwrap_or(false);
            let body = format!("{title} is {discount}% off{}{}", price_text(&data), if low { ", the lowest price Marquee has seen." } else { "." });
            // One alert per game per sale week, unless the discount gets deeper.
            if store::add_notification(&conn, "sale", &format!("sale:{id}:{}-{}:{discount}", week.year(), week.week()), "On sale", &body, Some(id))?.is_some() {
                made += 1;
            }
        }
    }

    if s.digest && chrono::Local::now().weekday().num_days_from_monday() == s.digest_day {
        let week = chrono::Local::now().iso_week();
        let mut stmt = conn.prepare(
            "SELECT i.title FROM items i JOIN scores sc ON sc.item_id = i.id
             WHERE i.media != 'artist' AND i.release_date BETWEEN ?1 AND ?2 AND sc.match_pct >= 70
             ORDER BY sc.score DESC LIMIT 40",
        )?;
        let titles: Vec<String> = stmt.query_map([&days_from_today(-3), &days_from_today(4)], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        if !titles.is_empty() {
            let lead = titles.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let more = titles.len().saturating_sub(3);
            let body = if more > 0 { format!("{lead} and {more} more this week match your taste.") } else { format!("{lead}: this week's picks for you.") };
            if store::add_notification(&conn, "digest", &format!("digest:{}-{}", week.year(), week.week()), "Your Marquee", &body, None)?.is_some() {
                made += 1;
            }
        }
    }
    drop(conn);
    if made > 0 {
        ctx.emit("notifications:new", serde_json::json!({ "count": made }));
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;
    use serde_json::json;

    #[test]
    fn quiet_hours_handle_windows_that_cross_midnight() {
        let s = NotifySettings::default(); // 22:00 -> 08:00
        assert!(is_quiet(&s, 23 * 60));
        assert!(is_quiet(&s, 2 * 60));
        assert!(is_quiet(&s, 7 * 60 + 59));
        assert!(!is_quiet(&s, 8 * 60));
        assert!(!is_quiet(&s, 14 * 60));
        let day = NotifySettings { quiet_from: "09:00".into(), quiet_to: "17:00".into(), ..Default::default() };
        assert!(is_quiet(&day, 12 * 60) && !is_quiet(&day, 18 * 60));
        assert!(!is_quiet(&NotifySettings { quiet_enabled: false, ..Default::default() }, 23 * 60));
        assert!(!is_quiet(&NotifySettings { quiet_from: "nonsense".into(), ..Default::default() }, 23 * 60));
    }

    #[test]
    fn a_saved_game_releasing_today_alerts_once() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            let mut d = ItemDraft::new(Media::Game, "Hades II").id("steam", 1);
            d.release_date = Some(today());
            d.set("price", json!({ "text": "$29.99", "cents": 2999 }));
            let id = store::upsert_item(&conn, &d).unwrap();
            store::add_to_list(&conn, store::list_id(&conn, "play-later").unwrap(), id, "local").unwrap();
            // An unsaved game out today is not the user's business.
            let mut other = ItemDraft::new(Media::Game, "Shovelware").id("steam", 2);
            other.release_date = Some(today());
            store::upsert_item(&conn, &other).unwrap();
        }
        assert_eq!(generate(&t.ctx).unwrap(), 1);
        assert_eq!(generate(&t.ctx).unwrap(), 0, "running again doesn't repeat it");
        let n = store::notifications(&t.ctx.db.lock(), 10).unwrap();
        assert_eq!(n[0].title, "Out today");
        assert_eq!(n[0].body, "Hades II is out now ($29.99).");
    }

    #[test]
    fn premieres_new_seasons_and_finales_alert_but_ordinary_episodes_do_not() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            for (n, (title, season, episode, finale)) in [("Severance", 3, 1, false), ("The Bear", 4, 10, true), ("Slow Horses", 5, 3, false)].into_iter().enumerate() {
                let mut d = ItemDraft::new(Media::Tv, title).id("tmdb", n as i64 + 1);
                d.set("next_episode", json!({ "air_date": today(), "season": season, "episode": episode, "finale": finale }));
                let id = store::upsert_item(&conn, &d).unwrap();
                store::record_event(&conn, id, "trakt", EventKind::Watched, 1.0, None, None, &Value::Null).unwrap();
            }
        }
        assert_eq!(generate(&t.ctx).unwrap(), 2);
        let bodies: Vec<String> = store::notifications(&t.ctx.db.lock(), 10).unwrap().into_iter().map(|n| n.body).collect();
        assert!(bodies.contains(&"Season 3 of Severance starts today.".to_string()));
        assert!(bodies.contains(&"The season 4 finale of The Bear airs today.".to_string()));
    }

    #[test]
    fn wishlist_sales_respect_the_threshold_and_skip_owned_games() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            for (n, (title, discount, owned)) in [("Deep Cut", 50, false), ("Small Cut", 10, false), ("Already Mine", 80, true)].into_iter().enumerate() {
                let mut d = ItemDraft::new(Media::Game, title).id("steam", n as i64 + 1);
                d.set("discount", discount);
                d.set("price", json!({ "text": "$9.99", "cents": 999 }));
                d.set("low", json!({ "cents": 999 }));
                let id = store::upsert_item(&conn, &d).unwrap();
                store::record_event(&conn, id, "steam", EventKind::Wishlist, 1.0, None, None, &Value::Null).unwrap();
                if owned {
                    store::record_event(&conn, id, "steam", EventKind::Owned, 1.0, None, None, &Value::Null).unwrap();
                }
            }
        }
        assert_eq!(generate(&t.ctx).unwrap(), 1);
        let n = store::notifications(&t.ctx.db.lock(), 10).unwrap();
        assert_eq!(n[0].body, "Deep Cut is 50% off ($9.99), the lowest price Marquee has seen.");
    }

    #[test]
    fn a_new_album_from_a_followed_artist_alerts_even_if_unsaved() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            store::follow(&conn, "artist", "tycho", "Tycho", None).unwrap();
            let mut d = ItemDraft::new(Media::Album, "Infinite Health").id("mbid", "rg-1");
            d.release_date = Some(today());
            d.set("artist", "Tycho");
            d.set("album_type", "album");
            d.set("date_precision", 10);
            store::upsert_item(&conn, &d).unwrap();
            // Year-only dates are placeholders, not release days.
            let mut vague = ItemDraft::new(Media::Album, "Someday").id("mbid", "rg-2");
            vague.release_date = Some(today());
            vague.set("artist", "Tycho");
            vague.set("date_precision", 4);
            store::upsert_item(&conn, &vague).unwrap();
        }
        assert_eq!(generate(&t.ctx).unwrap(), 1);
        assert_eq!(store::notifications(&t.ctx.db.lock(), 10).unwrap()[0].title, "New album from Tycho");
    }

    #[test]
    fn turning_notifications_off_stops_everything() {
        let t = ctx();
        store::set_setting(&t.ctx.db, "notifications", &json!({ "enabled": false })).unwrap();
        assert_eq!(generate(&t.ctx).unwrap(), 0);
        assert!(toastable(&t.ctx.db).unwrap().is_empty());
    }

    #[test]
    fn toasts_are_handed_out_once_and_capped() {
        let t = ctx();
        store::set_setting(&t.ctx.db, "notifications", &json!({ "quiet_enabled": false })).unwrap();
        {
            let conn = t.ctx.db.lock();
            for n in 0..7 {
                store::add_notification(&conn, "release", &format!("k{n}"), "Out today", &format!("Title {n}"), None).unwrap();
            }
        }
        let first = toastable(&t.ctx.db).unwrap();
        assert_eq!(first.len(), 4);
        assert_eq!(first.last().unwrap().body, "Title 6");
        assert!(toastable(&t.ctx.db).unwrap().is_empty());
        assert_eq!(store::notifications(&t.ctx.db.lock(), 20).unwrap().len(), 7, "all seven are still in the notification center");
    }
}
