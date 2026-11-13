//! Exports: the whole library as CSV or JSON, a list as Markdown, and the release calendar as
//! an `.ics` file other calendar apps can open or subscribe to.

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::feeds;
use crate::model::*;
use crate::store;
use serde_json::{json, Value};

/// One row per title the user has any history with or has saved.
pub fn library_rows(ctx: &Ctx) -> Result<Vec<Value>> {
    let conn = ctx.db.lock();
    let mut stmt = conn.prepare(
        "SELECT i.id, i.media, i.title, i.year, i.release_date, i.data,
            (SELECT value FROM user_events e WHERE e.item_id = i.id AND e.kind = 'rating' ORDER BY (e.source = 'marquee') DESC, e.at DESC LIMIT 1),
            (SELECT group_concat(DISTINCT e.kind) FROM user_events e WHERE e.item_id = i.id),
            (SELECT group_concat(DISTINCT e.source) FROM user_events e WHERE e.item_id = i.id),
            (SELECT max(e.at) FROM user_events e WHERE e.item_id = i.id),
            (SELECT group_concat(l.name, '; ') FROM list_items li JOIN lists l ON l.id = li.list_id WHERE li.item_id = i.id),
            (SELECT max(e.value) FROM user_events e WHERE e.item_id = i.id AND e.kind = 'play')
         FROM items i
         WHERE EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id) OR EXISTS (SELECT 1 FROM list_items li WHERE li.item_id = i.id)
         ORDER BY i.media, i.sort_title",
    )?;
    let rows: Vec<Value> = stmt
        .query_map([], |r| {
            let data: Value = serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or(Value::Null);
            let kinds: String = r.get::<_, Option<String>>(7)?.unwrap_or_default();
            let creator = data["artist"].as_str().or(data["director"].as_str()).or(data["developer"].as_str()).or(data["network"].as_str()).unwrap_or("").to_string();
            let status = if kinds.contains("rating") || kinds.contains("watched") || kinds.contains("play") { "done" } else if kinds.contains("wishlist") { "wishlist" } else if kinds.contains("owned") { "owned" } else { "saved" };
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "media": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "year": r.get::<_, Option<i64>>(3)?,
                "release_date": r.get::<_, Option<String>>(4)?,
                "creator": creator,
                "rating": r.get::<_, Option<f64>>(6)?,
                "status": status,
                "liked": kinds.contains("like"),
                "play_amount": r.get::<_, Option<f64>>(11)?,
                "last_activity": r.get::<_, Option<String>>(9)?,
                "lists": r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                "sources": r.get::<_, Option<String>>(8)?.unwrap_or_default(),
                "genres": data["genres"].as_array().map(|a| a.iter().filter_map(|g| g.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    // External ids, so the export can be matched up elsewhere.
    let mut ids = conn.prepare_cached("SELECT source, ext_id FROM item_ids WHERE item_id = ?1")?;
    let mut out = vec![];
    for mut row in rows {
        let id = row["id"].as_i64().unwrap_or(0);
        for pair in ids.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (source, ext) = pair?;
            if ["tmdb", "imdb", "steam", "igdb", "mbid", "spotify"].contains(&source.as_str()) {
                row[format!("{source}_id")] = json!(ext);
            }
        }
        if let Some(o) = row.as_object_mut() {
            o.remove("id");
        }
        out.push(row);
    }
    Ok(out)
}

const COLUMNS: &[&str] = &["media", "title", "year", "release_date", "creator", "rating", "status", "liked", "play_amount", "last_activity", "lists", "sources", "genres", "tmdb_id", "imdb_id", "steam_id", "igdb_id", "mbid_id", "spotify_id"];

pub fn library_csv(ctx: &Ctx) -> Result<String> {
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(COLUMNS)?;
    for row in library_rows(ctx)? {
        let rec: Vec<String> = COLUMNS
            .iter()
            .map(|c| match &row[*c] {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect();
        w.write_record(&rec)?;
    }
    String::from_utf8(w.into_inner().map_err(|e| Error::msg(e.to_string()))?).map_err(|e| Error::msg(e.to_string()))
}

pub fn library_json(ctx: &Ctx) -> Result<String> {
    Ok(serde_json::to_string_pretty(&json!({ "app": "Marquee", "version": ctx.version, "exported_at": now(), "library": library_rows(ctx)? }))?)
}

fn md_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('*', "\\*").replace('_', "\\_").replace('[', "\\[").replace(']', "\\]").replace('|', "\\|")
}

/// A list as Markdown, ready to paste anywhere.
pub fn list_markdown(ctx: &Ctx, list_id: i64) -> Result<String> {
    let conn = ctx.db.lock();
    let name: String = conn.query_row("SELECT name FROM lists WHERE id = ?1", [list_id], |r| r.get(0)).map_err(|_| Error::msg("That list doesn't exist."))?;
    let entries = store::list_entries(&conn, list_id)?;
    let mut out = format!("# {}\n\n", md_escape(&name));
    if entries.is_empty() {
        out.push_str("_Nothing here yet._\n");
        return Ok(out);
    }
    for (n, e) in entries.iter().enumerate() {
        let c = &e.card;
        let kind = match c.media.as_str() {
            "movie" => "Film",
            "tv" => "TV",
            "game" => "Game",
            _ => "Album",
        };
        let mut line = format!("{}. **{}**", n + 1, md_escape(&c.title));
        if let Some(a) = c.meta["artist"].as_str() {
            line.push_str(&format!(" by {}", md_escape(a)));
        }
        if let Some(y) = c.year {
            line.push_str(&format!(" ({y})"));
        }
        line.push_str(&format!(" · {kind}"));
        if let Some(r) = c.user_rating {
            line.push_str(&format!(" · {}", "★".repeat(r.floor() as usize) + if r.fract() > 0.0 { "½" } else { "" }));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&format!("\n_{} {} · made with Marquee_\n", entries.len(), if entries.len() == 1 { "title" } else { "titles" }));
    Ok(out)
}

fn ics_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

/// Folds a content line at 75 octets, as RFC 5545 requires.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for ch in line.chars() {
        let n = ch.len_utf8();
        if len + n > 75 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(ch);
        len += n;
    }
    out
}

/// The release calendar as iCalendar text: one all-day event per release.
pub fn ics(ctx: &Ctx, scope: &str) -> Result<String> {
    let entries = feeds::calendar(ctx, &days_from_today(-14), &days_from_today(365), scope)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//larosa023//Marquee//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        "X-WR-CALNAME:Marquee releases".into(),
        "X-WR-CALDESC:Movies\\, shows\\, games and albums from Marquee".into(),
        "REFRESH-INTERVAL;VALUE=DURATION:PT6H".into(),
        "X-PUBLISHED-TTL:PT6H".into(),
    ];
    for e in entries {
        let Ok(day) = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") else { continue };
        let next = day + chrono::Duration::days(1);
        let emoji = match e.card.media.as_str() {
            "movie" => "🎬",
            "tv" => "📺",
            "game" => "🎮",
            _ => "🎵",
        };
        let title = match (e.kind, e.card.meta["artist"].as_str()) {
            ("episode" | "premiere" | "finale", _) if e.card.media == "tv" && e.label != "Series premiere" => format!("{emoji} {} · {}", e.card.title, e.label),
            (_, Some(artist)) => format!("{emoji} {artist} · {}", e.card.title),
            _ => format!("{emoji} {}", e.card.title),
        };
        let mut desc = e.label.clone();
        if let Some(pct) = e.card.match_pct {
            desc.push_str(&format!("\n{pct}% match"));
        }
        if let Some(why) = e.card.why.first() {
            desc.push_str(&format!("\n{}", why.replace('*', "")));
        }
        lines.push("BEGIN:VEVENT".into());
        lines.push(format!("UID:marquee-{}-{}-{}@marquee.larosa023", e.card.id, e.date, e.kind));
        lines.push(format!("DTSTAMP:{stamp}"));
        lines.push(format!("DTSTART;VALUE=DATE:{}", day.format("%Y%m%d")));
        lines.push(format!("DTEND;VALUE=DATE:{}", next.format("%Y%m%d")));
        lines.push(format!("SUMMARY:{}", ics_escape(&title)));
        lines.push(format!("DESCRIPTION:{}", ics_escape(&desc)));
        lines.push("TRANSP:TRANSPARENT".into());
        lines.push("END:VEVENT".into());
    }
    lines.push("END:VCALENDAR".into());
    Ok(lines.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

/// Keeps `<data>/marquee.ics` current so a calendar app can subscribe to the file.
pub fn write_calendar_file(ctx: &Ctx) -> Result<std::path::PathBuf> {
    let scope: String = store::setting_or(&ctx.db, "calendar.export_scope", "lists".to_string());
    let path = ctx.data_dir.join("marquee.ics");
    std::fs::write(&path, ics(ctx, &scope)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::{ctx, TestCtx};

    fn world() -> (TestCtx, i64) {
        let t = ctx();
        let list = {
            let conn = t.ctx.db.lock();
            let mut film = ItemDraft::new(Media::Movie, "Paris, Texas").id("tmdb", 655).id("imdb", "tt0087884");
            film.year = Some(1984);
            film.genres = vec!["Drama".into()];
            film.set("director", "Wim Wenders");
            let film = store::upsert_item(&conn, &film).unwrap();
            store::record_event(&conn, film, "letterboxd", EventKind::Rating, 4.5, Some("2024-02-03T12:00:00Z"), None, &Value::Null).unwrap();
            let mut album = ItemDraft::new(Media::Album, "Infinite *Health*").id("mbid", "rg-1");
            album.release_date = Some(days_from_today(9));
            album.set("artist", "Tycho");
            album.set("date_precision", 10);
            let album = store::upsert_item(&conn, &album).unwrap();
            let list = store::create_list(&conn, "Rainy Sunday").unwrap();
            store::add_to_list(&conn, list, album, "local").unwrap();
            store::add_to_list(&conn, list, film, "local").unwrap();
            store::reorder_list(&conn, list, &[film, album]).unwrap();
            list
        };
        crate::sync::rescore(&t.ctx).unwrap();
        (t, list)
    }

    #[test]
    fn the_library_exports_as_csv_and_json_with_ids() {
        let (t, _) = world();
        let csv_text = library_csv(&t.ctx).unwrap();
        let mut rdr = csv::Reader::from_reader(csv_text.as_bytes());
        let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(rows.len(), 2);
        let film = rows.iter().find(|r| &r[0] == "movie").unwrap();
        assert_eq!(&film[1], "Paris, Texas", "commas in titles are quoted properly");
        assert_eq!((&film[4], &film[5], &film[6]), ("Wim Wenders", "4.5", "done"));
        assert_eq!(&film[10], "Rainy Sunday");
        assert_eq!((&film[13], &film[14]), ("655", "tt0087884"));
        let v: Value = serde_json::from_str(&library_json(&t.ctx).unwrap()).unwrap();
        assert_eq!(v["library"].as_array().unwrap().len(), 2);
        assert_eq!(v["library"][0]["status"], "saved");
    }

    #[test]
    fn a_list_becomes_markdown() {
        let (t, list) = world();
        let md = list_markdown(&t.ctx, list).unwrap();
        assert!(md.starts_with("# Rainy Sunday\n"));
        assert!(md.contains("1. **Paris, Texas** (1984) · Film · ★★★★½"));
        assert!(md.contains("2. **Infinite \\*Health\\*** by Tycho"), "markdown characters in titles are escaped: {md}");
        assert!(md.contains("_2 titles · made with Marquee_"));
    }

    #[test]
    fn the_calendar_exports_valid_all_day_events() {
        let (t, _) = world();
        let text = ics(&t.ctx, "lists").unwrap();
        assert!(text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        assert!(text.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(text.matches("BEGIN:VEVENT").count(), 1, "only the upcoming saved album");
        let day = (chrono::Local::now() + chrono::Duration::days(9)).format("%Y%m%d").to_string();
        assert!(text.contains(&format!("DTSTART;VALUE=DATE:{day}")));
        assert!(text.contains("SUMMARY:🎵 Tycho · Infinite *Health*"));
        assert!(text.lines().all(|l| l.len() <= 76), "lines are folded at 75 octets");
        let path = write_calendar_file(&t.ctx).unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("X-WR-CALNAME:Marquee releases"));
    }

    #[test]
    fn long_lines_fold_without_splitting_characters() {
        let folded = fold(&format!("SUMMARY:{}", "é".repeat(80)));
        assert!(folded.split("\r\n ").all(|part| part.len() <= 75));
        assert_eq!(folded.replace("\r\n ", ""), format!("SUMMARY:{}", "é".repeat(80)));
    }
}
