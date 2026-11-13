//! Manual import: any CSV or JSON file, for any media type, through a column mapper.
//! Known exports (IMDb ratings, RateYourMusic, Letterboxd, Trakt, Backloggd-style lists,
//! Serializd dumps) are recognised and mapped automatically; anything else can be mapped by hand.

use crate::error::{Error, Result};
use crate::model::*;
use crate::util::date_only;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub type Row = HashMap<String, String>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Mapping {
    /// "movie" | "tv" | "game" | "album", or "column:<name>" to read it per row.
    pub media: String,
    pub title: Option<String>,
    pub year: Option<String>,
    pub date: Option<String>,
    pub rating: Option<String>,
    /// "auto" | "5" | "10" | "100"
    pub rating_scale: String,
    pub artist: Option<String>,
    /// A second column joined to the artist (RateYourMusic splits names in two).
    pub artist_extra: Option<String>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub steam_id: Option<String>,
    /// A column saying watched / playing / wishlist...
    pub status: Option<String>,
    pub liked: Option<String>,
    /// Treat every row as a wishlist entry rather than something finished.
    pub as_wishlist: bool,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub columns: Vec<String>,
    pub sample: Vec<Vec<String>>,
    pub rows: usize,
    pub mapping: Mapping,
    /// The export this looks like, if any ("IMDb ratings", "RateYourMusic"...).
    pub recognised: Option<&'static str>,
}

fn flatten(prefix: &str, v: &Value, out: &mut Row) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&key, val, out);
            }
        }
        Value::Null => {}
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        Value::Array(a) => {
            let joined = a.iter().filter_map(|x| x.as_str().map(String::from).or_else(|| x.as_f64().map(|n| n.to_string()))).collect::<Vec<_>>().join(", ");
            if !joined.is_empty() {
                out.insert(prefix.to_string(), joined);
            }
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

/// Reads a CSV (comma, semicolon or tab separated) or a JSON array of objects.
pub fn read_table(bytes: &[u8], file_name: &str) -> Result<(Vec<String>, Vec<Row>)> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();
    if file_name.to_lowercase().ends_with(".json") || trimmed.starts_with('[') || trimmed.starts_with('{') {
        let v: Value = serde_json::from_str(trimmed).map_err(|e| Error::msg(format!("That JSON file couldn't be read: {e}")))?;
        // Accept a bare array, or an object with one array inside it ({"reviews": [...]}).
        let rows_v = match &v {
            Value::Array(a) => a.clone(),
            Value::Object(o) => o.values().filter_map(|x| x.as_array()).max_by_key(|a| a.len()).cloned().unwrap_or_default(),
            _ => vec![],
        };
        let mut columns: Vec<String> = vec![];
        let mut rows = vec![];
        for r in rows_v.iter().filter(|r| r.is_object()) {
            let mut row = Row::new();
            flatten("", r, &mut row);
            for k in row.keys() {
                if !columns.contains(k) {
                    columns.push(k.clone());
                }
            }
            rows.push(row);
        }
        if rows.is_empty() {
            return Err(Error::msg("That JSON file has no list of entries in it."));
        }
        columns.sort();
        return Ok((columns, rows));
    }
    let first_line = text.lines().next().unwrap_or("");
    let delim = [b',', b';', b'\t'].into_iter().max_by_key(|d| first_line.bytes().filter(|b| b == d).count()).unwrap_or(b',');
    let mut rdr = csv::ReaderBuilder::new().flexible(true).delimiter(delim).from_reader(text.as_bytes());
    let columns: Vec<String> = rdr.headers()?.iter().map(|h| h.trim().to_string()).collect();
    if columns.len() < 2 {
        return Err(Error::msg("That file doesn't look like a table. Marquee imports CSV and JSON files."));
    }
    let mut rows = vec![];
    for rec in rdr.records() {
        let rec = rec?;
        let row: Row = columns.iter().cloned().zip(rec.iter().map(|v| v.trim().to_string())).filter(|(_, v)| !v.is_empty()).collect();
        if !row.is_empty() {
            rows.push(row);
        }
    }
    Ok((columns, rows))
}

fn pick(columns: &[String], wanted: &[&str]) -> Option<String> {
    let norm = |s: &str| s.to_lowercase().replace([' ', '_', '-', '.'], "");
    for w in wanted {
        if let Some(c) = columns.iter().find(|c| norm(c) == norm(w)) {
            return Some(c.clone());
        }
    }
    None
}

/// Works out what each column is from its name.
pub fn guess(columns: &[String], file_name: &str) -> (Mapping, Option<&'static str>) {
    let has = |name: &str| columns.iter().any(|c| c == name);
    let mut m = Mapping { rating_scale: "auto".into(), ..Default::default() };
    let mut recognised = None;

    if has("Const") && has("Your Rating") {
        recognised = Some("IMDb ratings");
        m.media = "column:Title Type".into();
        m.rating_scale = "10".into();
        m.imdb_id = Some("Const".into());
    } else if has("RYM Album") {
        recognised = Some("RateYourMusic");
        m.media = "album".into();
        m.rating_scale = "10".into();
        m.artist = pick(columns, &["Last Name"]);
        m.artist_extra = pick(columns, &["First Name"]);
        m.year = pick(columns, &["Release_Date"]);
    } else if has("Letterboxd URI") {
        recognised = Some("Letterboxd");
        m.media = "movie".into();
        m.rating_scale = "5".into();
    } else if has("show.ids.trakt") || has("movie.ids.trakt") {
        recognised = Some("Trakt");
        m.media = if has("show.ids.trakt") { "tv".into() } else { "movie".into() };
        m.title = pick(columns, &["show.title", "movie.title"]);
        m.year = pick(columns, &["show.year", "movie.year"]);
        m.tmdb_id = pick(columns, &["show.ids.tmdb", "movie.ids.tmdb"]);
        m.imdb_id = pick(columns, &["show.ids.imdb", "movie.ids.imdb"]);
        m.rating_scale = "10".into();
        m.date = pick(columns, &["rated_at", "last_watched_at", "watched_at", "listed_at"]);
    } else if has("showName") || has("show_name") || file_name.to_lowercase().contains("serializd") {
        recognised = Some("Serializd");
        m.media = "tv".into();
        m.title = pick(columns, &["showName", "show_name", "show", "title", "name"]);
        m.tmdb_id = pick(columns, &["showId", "show_id", "tmdb_id", "tmdbId"]);
        m.rating_scale = "auto".into();
    }

    if m.title.is_none() {
        m.title = pick(columns, &["Title", "Name", "Game", "Album", "Show", "Film", "Movie", "Original Title", "Primary Title"]);
    }
    if m.year.is_none() {
        m.year = pick(columns, &["Year", "Release Year", "Released", "Release Date", "Release_Date", "first_air_date"]);
    }
    if m.date.is_none() {
        m.date = pick(columns, &["Date Rated", "Watched Date", "Date", "Finished", "Completed", "Date Finished", "Date Added", "Created", "Modified", "createdAt", "dateAdded"]);
    }
    if m.rating.is_none() {
        m.rating = pick(columns, &["Your Rating", "Rating", "My Rating", "Score", "Stars", "User Rating", "rating"]);
    }
    if m.artist.is_none() {
        m.artist = pick(columns, &["Artist", "Artists", "Artist Name", "Album Artist", "Band"]);
    }
    if m.imdb_id.is_none() {
        m.imdb_id = pick(columns, &["Const", "IMDb ID", "imdb_id", "imdbId", "tconst"]);
    }
    if m.tmdb_id.is_none() {
        m.tmdb_id = pick(columns, &["TMDB ID", "tmdb_id", "tmdbId", "TMDb"]);
    }
    m.steam_id = pick(columns, &["AppID", "App ID", "Steam ID", "steam_appid", "appid"]);
    m.status = pick(columns, &["Status", "Shelf", "State", "List", "Ownership"]);
    m.liked = pick(columns, &["Liked", "Like", "Loved", "Favorite", "Favourite"]);

    if m.media.is_empty() {
        let name = file_name.to_lowercase();
        m.media = if let Some(c) = pick(columns, &["Title Type", "Type", "Media Type", "Media", "Kind"]) {
            format!("column:{c}")
        } else if m.artist.is_some() || name.contains("album") || name.contains("music") || name.contains("rym") {
            "album".into()
        } else if m.steam_id.is_some() || name.contains("game") || name.contains("backloggd") || has("Platform") || has("Platforms") {
            "game".into()
        } else if name.contains("show") || name.contains("tv") || name.contains("series") {
            "tv".into()
        } else {
            "movie".into()
        };
    }
    if name_says_wishlist(file_name) {
        m.as_wishlist = true;
    }
    (m, recognised)
}

fn name_says_wishlist(file_name: &str) -> bool {
    let n = file_name.to_lowercase();
    ["watchlist", "wishlist", "backlog", "want", "to-watch", "to_play"].iter().any(|w| n.contains(w))
}

pub fn preview(bytes: &[u8], file_name: &str) -> Result<Preview> {
    let (columns, rows) = read_table(bytes, file_name)?;
    let (mapping, recognised) = guess(&columns, file_name);
    let sample = rows.iter().take(6).map(|r| columns.iter().map(|c| r.get(c).cloned().unwrap_or_default()).collect()).collect();
    Ok(Preview { columns, sample, rows: rows.len(), mapping, recognised })
}

fn media_from_cell(cell: &str) -> Option<Media> {
    let c = cell.to_lowercase().replace([' ', '_', '-'], "");
    Some(match c.as_str() {
        "movie" | "film" | "tvmovie" | "video" | "short" | "tvspecial" | "tvshort" => Media::Movie,
        "tvseries" | "tvminiseries" | "tv" | "show" | "series" | "tvepisode" | "anime" => Media::Tv,
        "videogame" | "game" => Media::Game,
        "album" | "ep" | "single" | "music" => Media::Album,
        other => return Media::parse(other),
    })
}

fn year_from(text: &str) -> Option<i32> {
    let digits: Vec<&str> = text.split(|c: char| !c.is_ascii_digit()).filter(|p| p.len() == 4).collect();
    digits.iter().filter_map(|d| d.parse::<i32>().ok()).find(|y| (1880..2100).contains(y))
}

fn parse_date(text: &str) -> Option<String> {
    if let Some(d) = date_only(text) {
        return Some(format!("{d}T12:00:00Z"));
    }
    for fmt in ["%m/%d/%Y", "%d/%m/%Y", "%Y/%m/%d", "%b %d, %Y", "%d %b %Y", "%B %d, %Y", "%d.%m.%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(text.trim(), fmt) {
            return Some(format!("{}T12:00:00Z", d.format("%Y-%m-%d")));
        }
    }
    // Unix seconds or milliseconds.
    text.trim().parse::<i64>().ok().filter(|n| *n > 100_000_000).and_then(|n| crate::util::unix_to_rfc3339(if n > 100_000_000_000 { n / 1000 } else { n }))
}

fn truthy(cell: &str) -> bool {
    matches!(cell.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "y" | "x" | "liked" | "loved" | "♥" | "❤")
}

/// Works out the star scale for a rating column: 5, 10 or 100.
fn detect_scale(rows: &[Row], col: &str) -> f64 {
    let max = rows.iter().filter_map(|r| r.get(col)).filter_map(|v| v.replace(',', ".").parse::<f64>().ok()).fold(0.0, f64::max);
    if max <= 5.0 {
        5.0
    } else if max <= 10.0 {
        10.0
    } else {
        100.0
    }
}

#[derive(Debug, Default)]
pub struct Imported {
    pub events: Vec<RawEvent>,
    pub wishlist: Vec<ItemDraft>,
    pub skipped: usize,
}

/// Turns the rows into events using the mapping.
pub fn run(bytes: &[u8], file_name: &str, m: &Mapping) -> Result<Imported> {
    let (_, rows) = read_table(bytes, file_name)?;
    let title_col = m.title.as_deref().ok_or_else(|| Error::msg("Choose which column holds the title."))?;
    let fixed_media = Media::parse(&m.media);
    let media_col = m.media.strip_prefix("column:");
    if fixed_media.is_none() && media_col.is_none() {
        return Err(Error::msg("Choose what kind of thing this file lists (movies, shows, games or albums)."));
    }
    let scale = match m.rating_scale.as_str() {
        "5" => 5.0,
        "10" => 10.0,
        "100" => 100.0,
        _ => m.rating.as_deref().map(|c| detect_scale(&rows, c)).unwrap_or(5.0),
    };
    let mut out = Imported::default();
    for row in &rows {
        let get = |col: &Option<String>| col.as_ref().and_then(|c| row.get(c)).filter(|v| !v.is_empty());
        let Some(title) = row.get(title_col).filter(|t| !t.is_empty()) else {
            out.skipped += 1;
            continue;
        };
        let media = match media_col {
            Some(c) => match row.get(c).and_then(|cell| media_from_cell(cell)) {
                Some(mm) => mm,
                None => {
                    out.skipped += 1;
                    continue;
                }
            },
            None => fixed_media.unwrap_or(Media::Movie),
        };
        if media == Media::Artist {
            out.skipped += 1;
            continue;
        }
        let mut d = ItemDraft::new(media, title.clone());
        d.tag_source = TagSource::Import;
        d.year = get(&m.year).and_then(|y| year_from(y));
        if let Some(id) = get(&m.tmdb_id).filter(|_| matches!(media, Media::Movie | Media::Tv)) {
            d.ids.push(("tmdb".into(), id.trim_end_matches(".0").to_string()));
        }
        if let Some(id) = get(&m.imdb_id).filter(|v| v.starts_with("tt")) {
            d.ids.push(("imdb".into(), id.clone()));
        }
        if let Some(id) = get(&m.steam_id).filter(|v| v.chars().all(|c| c.is_ascii_digit())).filter(|_| media == Media::Game) {
            d.ids.push(("steam".into(), id.clone()));
        }
        if media == Media::Album {
            let artist = [get(&m.artist_extra), get(&m.artist)].into_iter().flatten().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            if !artist.is_empty() {
                d.creators.push(Creator::new(artist.clone(), "artist"));
                d.set("artist", artist);
            }
        }
        let at = get(&m.date).and_then(|v| parse_date(v));
        let status = get(&m.status).map(|v| v.to_lowercase()).unwrap_or_default();
        let wish = m.as_wishlist || ["wish", "want", "plan", "backlog", "watchlist", "to watch", "to play"].iter().any(|w| status.contains(w));
        if wish {
            out.events.push(RawEvent::new(d.clone(), EventKind::Wishlist, 1.0).at(at.clone()));
            out.wishlist.push(d);
            continue;
        }
        out.events.push(RawEvent::new(d.clone(), EventKind::Watched, 1.0).at(at.clone()));
        if let Some(r) = get(&m.rating).and_then(|v| v.replace(',', ".").parse::<f64>().ok()).filter(|r| *r > 0.0) {
            // Round to the nearest half star.
            let stars = ((r / scale * 5.0) * 2.0).round() / 2.0;
            out.events.push(RawEvent::new(d.clone(), EventKind::Rating, stars.clamp(0.5, 5.0)).at(at.clone()));
        }
        if get(&m.liked).map(|v| truthy(v)).unwrap_or(false) {
            out.events.push(RawEvent::new(d, EventKind::Like, 1.0).at(at));
        }
    }
    if out.events.is_empty() {
        return Err(Error::msg("Nothing could be imported from that file with these columns. Check the title column."));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMDB: &str = "Const,Your Rating,Date Rated,Title,Original Title,URL,Title Type,IMDb Rating,Runtime (mins),Year,Genres,Num Votes,Release Date,Directors\n\
tt2543164,9,2024-03-01,Arrival,Arrival,https://www.imdb.com/title/tt2543164,Movie,7.9,116,2016,\"Drama, Sci-Fi\",800000,2016-09-01,Denis Villeneuve\n\
tt11280740,10,2025-02-01,Severance,Severance,https://www.imdb.com/title/tt11280740,TV Series,8.7,50,2022,\"Drama, Mystery\",300000,2022-02-18,\n\
tt0000001,6,2025-02-02,Some Podcast,Some Podcast,https://x,Podcast Series,6,10,2020,,10,2020-01-01,\n";

    #[test]
    fn imdb_exports_are_recognised_and_mixed_media_is_split() {
        let p = preview(IMDB.as_bytes(), "ratings.csv").unwrap();
        assert_eq!(p.recognised, Some("IMDb ratings"));
        assert_eq!(p.rows, 3);
        assert_eq!(p.mapping.media, "column:Title Type");
        let r = run(IMDB.as_bytes(), "ratings.csv", &p.mapping).unwrap();
        assert_eq!(r.skipped, 1, "a podcast isn't something Marquee tracks");
        let arrival = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Arrival").unwrap();
        assert_eq!(arrival.value, 4.5);
        assert_eq!(arrival.draft.media(), Media::Movie);
        assert_eq!(arrival.draft.ids, vec![("imdb".to_string(), "tt2543164".to_string())]);
        assert_eq!(arrival.at.as_deref(), Some("2024-03-01T12:00:00Z"));
        let sev = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Severance").unwrap();
        assert_eq!((sev.draft.media(), sev.value), (Media::Tv, 5.0));
    }

    #[test]
    fn rateyourmusic_joins_the_two_name_columns() {
        let csv = "RYM Album, First Name,Last Name,First Name localized, Last Name localized,Title,Release_Date,Rating,Ownership,Purchase Date,Media Type,Review\n\
123,,Tycho,,,Dive,2011,9,n,,,\n456,Nick,Drake,,,Pink Moon,1972,10,n,,,\n";
        let p = preview(csv.as_bytes(), "user_music_export.csv").unwrap();
        assert_eq!(p.recognised, Some("RateYourMusic"));
        let r = run(csv.as_bytes(), "user_music_export.csv", &p.mapping).unwrap();
        let pink = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Pink Moon").unwrap();
        assert_eq!(pink.draft.data["artist"], "Nick Drake");
        assert_eq!(pink.value, 5.0);
        assert_eq!(pink.draft.year, Some(1972));
        let dive = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Dive").unwrap();
        assert_eq!((dive.draft.data["artist"].as_str(), dive.value), (Some("Tycho"), 4.5));
    }

    #[test]
    fn an_unknown_game_list_is_guessed_from_its_headers() {
        let csv = "Game;Platform;Status;Score;Finished\nOuter Wilds;PC;Completed;95;03/14/2024\nHades II;PC;Wishlist;;\nCeleste;Switch;Played;80;\n";
        let p = preview(csv.as_bytes(), "my-games.csv").unwrap();
        assert_eq!(p.recognised, None);
        assert_eq!(p.mapping.media, "game");
        assert_eq!(p.mapping.title.as_deref(), Some("Game"));
        assert_eq!(p.mapping.status.as_deref(), Some("Status"));
        let r = run(csv.as_bytes(), "my-games.csv", &p.mapping).unwrap();
        assert_eq!(r.wishlist.len(), 1);
        assert_eq!(r.wishlist[0].title, "Hades II");
        let ow = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Outer Wilds").unwrap();
        assert_eq!(ow.value, 5.0, "95/100 rounds to five stars");
        assert_eq!(ow.at.as_deref(), Some("2024-03-14T12:00:00Z"));
        let celeste = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Celeste").unwrap();
        assert_eq!(celeste.value, 4.0);
    }

    #[test]
    fn json_with_nested_objects_is_flattened() {
        let json = r#"{"reviews":[{"showName":"Severance","showId":95396,"rating":9,"dateAdded":"2025-02-02T10:00:00Z","like":true},{"showName":"The Bear","showId":136315,"rating":8,"dateAdded":"2025-03-01T10:00:00Z"}]}"#;
        let p = preview(json.as_bytes(), "serializd-export.json").unwrap();
        assert_eq!(p.recognised, Some("Serializd"));
        assert_eq!(p.mapping.tmdb_id.as_deref(), Some("showId"));
        let r = run(json.as_bytes(), "serializd-export.json", &p.mapping).unwrap();
        let sev = r.events.iter().find(|e| e.kind == EventKind::Rating && e.draft.title == "Severance").unwrap();
        assert_eq!(sev.value, 4.5);
        assert_eq!(sev.draft.ids, vec![("tmdb".to_string(), "95396".to_string())]);
        assert_eq!(sev.draft.media(), Media::Tv);
    }

    #[test]
    fn a_watchlist_file_imports_as_wishlist_entries() {
        let csv = "Name,Year\nStalker,1979\nSolaris,1972\n";
        let p = preview(csv.as_bytes(), "watchlist.csv").unwrap();
        assert!(p.mapping.as_wishlist);
        let r = run(csv.as_bytes(), "watchlist.csv", &p.mapping).unwrap();
        assert_eq!(r.wishlist.len(), 2);
        assert!(r.events.iter().all(|e| e.kind == EventKind::Wishlist));
    }

    #[test]
    fn bad_files_and_mappings_explain_themselves() {
        assert!(read_table(b"just some words", "notes.txt").is_err());
        assert!(read_table(b"{\"a\":1}", "x.json").unwrap_err().to_string().contains("no list"));
        let csv = "A,B\n1,2\n";
        let mut m = Mapping { media: "movie".into(), ..Default::default() };
        assert!(run(csv.as_bytes(), "x.csv", &m).unwrap_err().to_string().contains("title"));
        m.title = Some("A".into());
        m.media = String::new();
        assert!(run(csv.as_bytes(), "x.csv", &m).unwrap_err().to_string().contains("kind of thing"));
    }

    #[test]
    fn ratings_scale_and_dates() {
        assert_eq!(parse_date("03/14/2024").as_deref(), Some("2024-03-14T12:00:00Z"));
        assert_eq!(parse_date("1700000000").as_deref(), Some("2023-11-14T22:13:20Z"));
        assert_eq!(year_from("Nov 5, 2026"), Some(2026));
        assert_eq!(year_from("n/a"), None);
    }
}
