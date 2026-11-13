//! Letterboxd: diary, ratings, likes and watchlist.
//!
//! Letterboxd has no open public API, so Marquee reads two things instead:
//! * the public RSS feed (`letterboxd.com/<user>/rss/`), which carries the ~50 most recent
//!   diary entries with ratings, likes and TMDB ids
//! * the account's data export (Settings → Data → Export your data), a zip of CSV files with
//!   the full history
//!
//! Writing isn't possible, so "Add to Letterboxd watchlist" opens the film's Letterboxd page.

use super::{cfg, item_for_write, Source, Synced, WriteOp, WriteOutcome};
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use crate::util::*;
use async_trait::async_trait;
use serde_json::json;
use std::io::Read;

pub struct Letterboxd;

pub const EXPORT_STEPS: &[&str] = &["Open letterboxd.com and sign in.", "Go to Settings → Data.", "Click \"Export your data\" and confirm. A zip file downloads.", "Drop that zip here (no need to unpack it)."];

fn valid_username(u: &str) -> bool {
    !u.is_empty() && u.len() <= 40 && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub async fn connect(ctx: &Ctx, username: &str) -> Result<String> {
    let user = username.trim().trim_start_matches('@').trim_matches('/').rsplit('/').next().unwrap_or("").to_lowercase();
    if !valid_username(&user) {
        return Err(Error::msg("Type your Letterboxd username (the part after letterboxd.com/)."));
    }
    let resp = ctx.http.get("Letterboxd", format!("https://letterboxd.com/{user}/rss/")).send_raw().await?;
    match resp.status {
        200 => {}
        404 => return Err(Error::msg(format!("There's no Letterboxd member called \"{user}\"."))),
        403 | 429 | 503 => return Err(Error::msg("Letterboxd is blocking automated requests right now. Import your data export instead, or try again later.")),
        status => return Err(Error::Api { service: "Letterboxd".into(), status, detail: String::new() }),
    }
    parse_rss(&resp.text())?;
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "letterboxd", json!({ "username": user }))?;
    store::set_source(&conn, "letterboxd", "connected", Some(&user), None)?;
    Ok(user)
}

/// Diary entries from the RSS feed. List and review-only items are skipped.
pub fn parse_rss(xml: &str) -> Result<Vec<RawEvent>> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| Error::msg(format!("Letterboxd's feed couldn't be read: {e}")))?;
    let mut out = vec![];
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let field = |name: &str| item.children().find(|c| c.tag_name().name() == name).and_then(|c| c.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let Some(title) = field("filmTitle") else { continue };
        let mut d = ItemDraft::new(Media::Movie, title);
        d.year = field("filmYear").and_then(|y| y.parse().ok());
        if let Some(tmdb) = field("movieId") {
            d.ids.push(("tmdb".into(), tmdb));
        } else if let Some(tv) = field("tvId") {
            // Letterboxd lists some miniseries; they are TV on TMDB.
            d.media = Some(Media::Tv);
            d.ids.push(("tmdb".into(), tv));
        }
        if let Some(link) = field("link") {
            if let Some(slug) = link.trim_end_matches('/').rsplit("/film/").next().filter(|s| !s.contains("letterboxd.com")) {
                d.ids.push(("letterboxd".into(), slug.split('/').next().unwrap_or(slug).to_string()));
            }
        }
        let watched = field("watchedDate").map(|w| format!("{w}T12:00:00Z"));
        let mut ev = RawEvent::new(d.clone(), EventKind::Watched, 1.0).at(watched.clone());
        ev.meta = json!({ "rewatch": field("rewatch").as_deref() == Some("Yes") });
        out.push(ev);
        if let Some(r) = field("memberRating").and_then(|r| r.parse::<f64>().ok()) {
            out.push(RawEvent::new(d.clone(), EventKind::Rating, r).at(watched.clone()));
        }
        if field("memberLike").as_deref() == Some("Yes") {
            out.push(RawEvent::new(d, EventKind::Like, 1.0).at(watched));
        }
    }
    Ok(out)
}

fn csv_rows(bytes: &[u8]) -> Result<Vec<std::collections::HashMap<String, String>>> {
    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let headers: Vec<String> = rdr.headers()?.iter().map(|h| h.trim().to_string()).collect();
    let mut rows = vec![];
    for rec in rdr.records() {
        let rec = rec?;
        rows.push(headers.iter().cloned().zip(rec.iter().map(|v| v.trim().to_string())).collect());
    }
    Ok(rows)
}

fn film(row: &std::collections::HashMap<String, String>) -> Option<ItemDraft> {
    let name = row.get("Name").filter(|n| !n.is_empty())?;
    let mut d = ItemDraft::new(Media::Movie, name.clone());
    d.year = row.get("Year").and_then(|y| y.parse().ok());
    if let Some(uri) = row.get("Letterboxd URI").filter(|u| !u.is_empty()) {
        d.set("letterboxd_url", uri.clone());
    }
    Some(d)
}

#[derive(Default, Debug)]
pub struct ExportResult {
    pub events: Vec<RawEvent>,
    pub watchlist: Vec<ItemDraft>,
    pub counts: Vec<(String, usize)>,
}

/// Reads a Letterboxd data export zip: watched, ratings, likes, diary and watchlist.
pub fn parse_export(zip_bytes: &[u8]) -> Result<ExportResult> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|_| Error::msg("That isn't a Letterboxd export. Pick the zip file Letterboxd gave you."))?;
    let mut out = ExportResult::default();
    let mut found_any = false;
    for idx in 0..zip.len() {
        let mut file = zip.by_index(idx)?;
        let name = file.name().to_lowercase();
        let kind = match name.as_str() {
            n if n.ends_with("watched.csv") && !n.contains("deleted") => "watched",
            n if n.ends_with("ratings.csv") && !n.contains("deleted") => "ratings",
            n if n.ends_with("watchlist.csv") && !n.contains("deleted") => "watchlist",
            n if n.ends_with("diary.csv") && !n.contains("deleted") => "diary",
            n if n.ends_with("likes/films.csv") => "likes",
            _ => continue,
        };
        found_any = true;
        let mut bytes = vec![];
        file.read_to_end(&mut bytes)?;
        let rows = csv_rows(&bytes)?;
        out.counts.push((kind.to_string(), rows.len()));
        for row in &rows {
            let Some(d) = film(row) else { continue };
            let date = row.get("Watched Date").filter(|d| !d.is_empty()).or(row.get("Date")).and_then(|d| date_only(d)).map(|d| format!("{d}T12:00:00Z"));
            match kind {
                "watched" => out.events.push(RawEvent::new(d, EventKind::Watched, 1.0).at(date)),
                "diary" => {
                    // The diary has the real watch date; the rating there duplicates ratings.csv.
                    out.events.push(RawEvent::new(d, EventKind::Watched, 1.0).at(date));
                }
                "ratings" => {
                    if let Some(r) = row.get("Rating").and_then(|r| r.parse::<f64>().ok()) {
                        out.events.push(RawEvent::new(d, EventKind::Rating, r).at(date));
                    }
                }
                "likes" => out.events.push(RawEvent::new(d, EventKind::Like, 1.0).at(date)),
                "watchlist" => {
                    out.events.push(RawEvent::new(d.clone(), EventKind::Wishlist, 1.0).at(date));
                    out.watchlist.push(d);
                }
                _ => {}
            }
        }
    }
    if !found_any {
        return Err(Error::msg("That zip doesn't have Letterboxd's files in it (watched.csv, ratings.csv...). Pick the export zip from Letterboxd → Settings → Data."));
    }
    Ok(out)
}

pub fn film_url(item: &Item) -> Option<String> {
    if let Some(u) = item.data["letterboxd_url"].as_str() {
        return Some(u.to_string());
    }
    if let Some(slug) = item.ext("letterboxd") {
        return Some(format!("https://letterboxd.com/film/{slug}/"));
    }
    // Letterboxd redirects these to the film page.
    if let Some(tmdb) = item.ext("tmdb").filter(|_| item.media == Media::Movie) {
        return Some(format!("https://letterboxd.com/tmdb/{tmdb}/"));
    }
    item.ext("imdb").map(|imdb| format!("https://letterboxd.com/imdb/{imdb}/"))
}

#[async_trait]
impl Source for Letterboxd {
    fn id(&self) -> &'static str {
        "letterboxd"
    }
    fn name(&self) -> &'static str {
        "Letterboxd"
    }
    fn reads(&self) -> Vec<&'static str> {
        vec!["Recent diary (RSS)", "Full history (export zip)", "Ratings", "Likes", "Watchlist (export zip)"]
    }
    fn writes(&self) -> Vec<&'static str> {
        vec!["Watchlist (opens the film page)"]
    }
    fn note(&self) -> &'static str {
        "Letterboxd has no public API. Marquee reads your public RSS feed for recent entries and your data export for everything else."
    }
    fn configured(&self, _ctx: &Ctx) -> bool {
        true
    }

    async fn sync(&self, ctx: &Ctx) -> Result<Synced> {
        let user = s(&cfg(ctx, "letterboxd"), "username").ok_or_else(|| Error::Reauth("Letterboxd".into()))?;
        let xml = ctx.http.get("Letterboxd", format!("https://letterboxd.com/{user}/rss/")).ttl(1800).text().await?;
        Ok(Synced { events: parse_rss(&xml)?, account: Some(user), ..Default::default() })
    }

    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome> {
        let item = item_for_write(ctx, op.item_id())?;
        if item.media != Media::Movie {
            return Err(Error::msg("Letterboxd is for films."));
        }
        let url = film_url(&item).ok_or_else(|| Error::msg(format!("Marquee doesn't know the Letterboxd page for {}.", item.title)))?;
        ctx.host.open_url(&url);
        let hint = match op {
            WriteOp::AddToList { .. } => format!("Letterboxd opened {}. Click Watchlist there. It's already on your Marquee Watchlist.", item.title),
            WriteOp::Rate { .. } | WriteOp::MarkDone { .. } => format!("Letterboxd opened {}. Log or rate it there; Marquee picks it up from your feed.", item.title),
            _ => format!("Letterboxd opened {}.", item.title),
        };
        Ok(WriteOutcome::Opened { url, hint })
    }

    fn forget(&self, _ctx: &Ctx) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;
    use std::io::Write;

    const RSS: &str = include_str!("../../tests/fixtures/letterboxd_rss.xml");

    #[test]
    fn rss_yields_watches_ratings_and_likes() {
        let events = parse_rss(RSS).unwrap();
        // Two diary entries; the list item is skipped.
        assert_eq!(events.iter().filter(|e| e.kind == EventKind::Watched).count(), 2);
        let rating = events.iter().find(|e| e.kind == EventKind::Rating).unwrap();
        assert_eq!(rating.value, 4.5);
        assert_eq!(rating.draft.title, "Arrival");
        assert_eq!(rating.draft.year, Some(2016));
        assert!(rating.draft.ids.contains(&("tmdb".into(), "329865".into())));
        assert!(rating.draft.ids.contains(&("letterboxd".into(), "arrival-2016".into())));
        assert_eq!(rating.at.as_deref(), Some("2025-01-10T12:00:00Z"));
        assert_eq!(events.iter().filter(|e| e.kind == EventKind::Like).count(), 1);
        assert_eq!(events.iter().filter(|e| e.kind == EventKind::Rating).count(), 1, "an unrated entry has no rating event");
    }

    fn export_zip() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("watched.csv", "Date,Name,Year,Letterboxd URI\n2024-01-02,Arrival,2016,https://boxd.it/aaaa\n2024-02-03,\"Paris, Texas\",1984,https://boxd.it/bbbb\n"),
                ("ratings.csv", "Date,Name,Year,Letterboxd URI,Rating\n2024-01-02,Arrival,2016,https://boxd.it/aaaa,5\n2024-02-03,\"Paris, Texas\",1984,https://boxd.it/bbbb,4.5\n"),
                ("watchlist.csv", "Date,Name,Year,Letterboxd URI\n2025-05-05,Stalker,1979,https://boxd.it/cccc\n"),
                ("likes/films.csv", "Date,Name,Year,Letterboxd URI\n2024-01-02,Arrival,2016,https://boxd.it/aaaa\n"),
                ("deleted/watched.csv", "Date,Name,Year,Letterboxd URI\n2020-01-01,Oops,2000,https://boxd.it/dddd\n"),
                ("profile.csv", "Date Joined,Username\n2019-01-01,anthony\n"),
            ] {
                z.start_file(name, opts).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn the_export_zip_gives_full_history() {
        let r = parse_export(&export_zip()).unwrap();
        assert_eq!(r.events.iter().filter(|e| e.kind == EventKind::Watched).count(), 2);
        assert_eq!(r.events.iter().filter(|e| e.kind == EventKind::Rating).count(), 2);
        assert_eq!(r.events.iter().filter(|e| e.kind == EventKind::Like).count(), 1);
        assert_eq!(r.watchlist.len(), 1);
        assert!(r.events.iter().any(|e| e.draft.title == "Paris, Texas" && e.kind == EventKind::Rating && e.value == 4.5), "quoted commas survive");
        assert!(!r.events.iter().any(|e| e.draft.title == "Oops"), "deleted entries are ignored");
        assert_eq!(r.events[0].draft.data["letterboxd_url"], "https://boxd.it/aaaa");
    }

    #[test]
    fn a_wrong_file_is_explained() {
        assert!(parse_export(b"not a zip").unwrap_err().to_string().contains("isn't a Letterboxd export"));
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("holiday.txt", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"hi").unwrap();
            z.finish().unwrap();
        }
        assert!(parse_export(&buf.into_inner()).unwrap_err().to_string().contains("watched.csv"));
    }

    #[tokio::test]
    async fn connecting_checks_the_feed_and_cleans_the_username() {
        let t = ctx();
        t.net.on("letterboxd.com/larosa/rss/", 200, RSS);
        assert_eq!(connect(&t.ctx, "https://letterboxd.com/Larosa/").await.unwrap(), "larosa");
        t.net.on("letterboxd.com/ghost/rss/", 404, "");
        assert!(connect(&t.ctx, "ghost").await.unwrap_err().to_string().contains("no Letterboxd member"));
        assert!(connect(&t.ctx, "bad name!").await.is_err());
    }

    #[tokio::test]
    async fn the_write_fallback_opens_the_film_page() {
        let t = ctx();
        let id = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865)).unwrap();
        let out = Letterboxd.write(&t.ctx, &WriteOp::AddToList { item_id: id, target: "watchlist".into() }).await.unwrap();
        assert!(matches!(out, WriteOutcome::Opened { .. }));
        assert_eq!(t.host.opened.lock().unwrap()[0], "https://letterboxd.com/tmdb/329865/");
    }
}
