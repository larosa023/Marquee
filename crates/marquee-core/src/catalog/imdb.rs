//! Episode ratings from IMDb: the same numbers SeriesGraph charts.
//!
//! SeriesGraph has no API; it draws its heatmaps from IMDb's ratings. IMDb publishes those
//! ratings as downloadable files for personal, non-commercial use
//! (https://developer.imdb.com/non-commercial-datasets/), so Marquee fetches the two files it
//! needs about once a week and keeps every rated episode in its own small database:
//!
//! * `title.ratings.tsv.gz` (~8 MB): rating and vote count for every rated title
//! * `title.episode.tsv.gz` (~50 MB): which series, season and episode each episode belongs to
//!
//! The result (`imdb.db` in the data folder, around 25 MB) is separate from Marquee's own
//! database, so backups stay small and a failed refresh never touches user data. IMDb requires
//! the attribution "Information courtesy of IMDb (https://www.imdb.com). Used with permission."

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::store;
use flate2::read::MultiGzDecoder;
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub const RATINGS_URL: &str = "https://datasets.imdbws.com/title.ratings.tsv.gz";
pub const EPISODES_URL: &str = "https://datasets.imdbws.com/title.episode.tsv.gz";
pub const ATTRIBUTION: &str = "Information courtesy of IMDb (https://www.imdb.com). Used with permission.";
const REFRESH_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImdbEpisode {
    pub season: i64,
    pub number: i64,
    pub rating: f64,
    pub votes: i64,
}

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("imdb.db")
}

/// On unless turned off in Settings → General.
pub fn enabled(ctx: &Ctx) -> bool {
    store::get_setting::<bool>(&ctx.db, "imdb.enabled").unwrap_or(true)
}

/// True when the local copy is missing or more than a week old.
pub fn is_due(ctx: &Ctx) -> bool {
    if !enabled(ctx) {
        return false;
    }
    if !db_path(&ctx.data_dir).exists() {
        return true;
    }
    let last = store::get_setting::<String>(&ctx.db, "imdb.updated_at");
    match last.and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok()) {
        Some(t) => chrono::Utc::now().signed_duration_since(t) > chrono::Duration::days(REFRESH_DAYS),
        None => true,
    }
}

/// "tt0903747" -> 903747. IMDb ids are "tt" plus digits; anything else is ignored.
pub fn tconst(s: &str) -> Option<u32> {
    s.strip_prefix("tt").and_then(|d| d.parse().ok())
}

fn lines(gz: &[u8]) -> impl Iterator<Item = String> + '_ {
    // The header row is skipped by the callers' parsers (it never parses as a tconst).
    BufReader::with_capacity(1 << 16, MultiGzDecoder::new(gz)).lines().map_while(|l| l.ok())
}

/// tconst -> (rating x10, votes). Ratings have one decimal, so x10 fits a u8 losslessly.
pub fn parse_ratings(gz: &[u8]) -> HashMap<u32, (u8, u32)> {
    let mut out = HashMap::with_capacity(1_700_000);
    for line in lines(gz) {
        let mut cols = line.split('\t');
        let (Some(id), Some(rating), Some(votes)) = (cols.next().and_then(tconst), cols.next(), cols.next()) else { continue };
        let (Ok(rating), Ok(votes)) = (rating.parse::<f64>(), votes.parse::<u32>()) else { continue };
        out.insert(id, ((rating * 10.0).round().clamp(0.0, 100.0) as u8, votes));
    }
    out
}

/// Builds `imdb.db` from the two files and swaps it in. Returns the number of rated episodes.
pub fn build(path: &Path, ratings: &HashMap<u32, (u8, u32)>, episodes_gz: &[u8]) -> Result<usize> {
    let tmp = path.with_extension("db.tmp");
    let _ = std::fs::remove_file(&tmp);
    let count = {
        let mut conn = Connection::open(&tmp)?;
        conn.execute_batch(
            "PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;
             CREATE TABLE episodes (show INTEGER NOT NULL, season INTEGER NOT NULL, episode INTEGER NOT NULL, rating REAL NOT NULL, votes INTEGER NOT NULL,
                                    PRIMARY KEY (show, season, episode)) WITHOUT ROWID;
             CREATE TABLE shows (show INTEGER PRIMARY KEY, rating REAL, votes INTEGER);",
        )?;
        let tx = conn.transaction()?;
        let mut count = 0usize;
        let mut parents: Vec<u32> = vec![];
        {
            let mut insert = tx.prepare("INSERT OR REPLACE INTO episodes (show, season, episode, rating, votes) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for line in lines(episodes_gz) {
                let mut cols = line.split('\t');
                let (Some(ep), Some(show), Some(season), Some(number)) = (cols.next().and_then(tconst), cols.next().and_then(tconst), cols.next(), cols.next()) else { continue };
                let Some(&(rating, votes)) = ratings.get(&ep) else { continue };
                // "\N" (unknown) seasons and episode numbers can't be placed on a grid.
                let (Ok(season), Ok(number)) = (season.parse::<i64>(), number.parse::<i64>()) else { continue };
                insert.execute(params![show, season, number, rating as f64 / 10.0, votes])?;
                parents.push(show);
                count += 1;
            }
        }
        parents.sort_unstable();
        parents.dedup();
        {
            let mut insert = tx.prepare("INSERT INTO shows (show, rating, votes) VALUES (?1, ?2, ?3)")?;
            for show in parents {
                if let Some(&(rating, votes)) = ratings.get(&show) {
                    insert.execute(params![show, rating as f64 / 10.0, votes])?;
                }
            }
        }
        tx.commit()?;
        if count == 0 {
            return Err(Error::msg("IMDb's episode file came back empty."));
        }
        count
    };
    std::fs::rename(&tmp, path)?;
    Ok(count)
}

/// Downloads both files and rebuilds the local copy. Runs in the background during a sync.
pub async fn refresh(ctx: &Ctx) -> Result<usize> {
    let ua = ctx.user_agent();
    let ratings_gz = ctx.http.get("IMDb", RATINGS_URL).header("User-Agent", ua.clone()).send().await?.body;
    let ratings = tokio::task::spawn_blocking(move || parse_ratings(&ratings_gz)).await.map_err(|e| Error::msg(format!("Reading IMDb's ratings failed: {e}")))?;
    if ratings.is_empty() {
        return Err(Error::msg("IMDb's ratings file came back empty."));
    }
    let episodes_gz = ctx.http.get("IMDb", EPISODES_URL).header("User-Agent", ua).send().await?.body;
    let path = db_path(&ctx.data_dir);
    let count = tokio::task::spawn_blocking(move || build(&path, &ratings, &episodes_gz)).await.map_err(|e| Error::msg(format!("Storing IMDb's ratings failed: {e}")))??;
    store::set_setting(&ctx.db, "imdb.updated_at", &crate::model::now())?;
    tracing::info!(episodes = count, "IMDb episode ratings updated");
    Ok(count)
}

fn open(data_dir: &Path) -> Option<Connection> {
    let path = db_path(data_dir);
    if !path.exists() {
        return None;
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()
}

/// Every rated episode of a show, in order, plus the show's own rating and votes.
/// A show's rated episodes, and its own (rating, votes) when IMDb has one.
pub type ShowRatings = (Vec<ImdbEpisode>, Option<(f64, i64)>);

pub fn show(data_dir: &Path, imdb_id: &str) -> Option<ShowRatings> {
    let id = tconst(imdb_id)?;
    let conn = open(data_dir)?;
    let mut stmt = conn.prepare("SELECT season, episode, rating, votes FROM episodes WHERE show = ?1 ORDER BY season, episode").ok()?;
    let episodes: Vec<ImdbEpisode> = stmt.query_map([id], |r| Ok(ImdbEpisode { season: r.get(0)?, number: r.get(1)?, rating: r.get(2)?, votes: r.get(3)? })).ok()?.filter_map(|r| r.ok()).collect();
    let rating = conn.query_row("SELECT rating, votes FROM shows WHERE show = ?1", [id], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?))).ok();
    if episodes.is_empty() && rating.is_none() {
        return None;
    }
    Some((episodes, rating))
}

pub fn size_bytes(data_dir: &Path) -> u64 {
    std::fs::metadata(db_path(data_dir)).map(|m| m.len()).unwrap_or(0)
}

pub fn remove(data_dir: &Path) {
    let _ = std::fs::remove_file(db_path(data_dir));
}

#[cfg(test)]
pub mod testing {
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    pub fn gz(text: &str) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::fast());
        e.write_all(text.as_bytes()).unwrap();
        e.finish().unwrap()
    }

    pub const RATINGS: &str = "tconst\taverageRating\tnumVotes\n\
        tt0903747\t9.5\t2300000\n\
        tt0959621\t9.0\t41000\n\
        tt1054724\t8.6\t29000\n\
        tt1054725\t8.2\t26000\n\
        tt2301451\t10.0\t240000\n\
        tt9999999\t7.1\t12\n";

    // Breaking Bad: S1E1, S1E2, S2E1 (unrated, dropped), S5E14; plus a row with unknown numbers.
    pub const EPISODES: &str = "tconst\tparentTconst\tseasonNumber\tepisodeNumber\n\
        tt0959621\ttt0903747\t1\t1\n\
        tt1054724\ttt0903747\t1\t2\n\
        tt1232244\ttt0903747\t2\t1\n\
        tt2301451\ttt0903747\t5\t14\n\
        tt1054725\ttt0903747\t\\N\t\\N\n";
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn ids_parse() {
        assert_eq!(tconst("tt0903747"), Some(903747));
        assert_eq!(tconst("tconst"), None);
        assert_eq!(tconst("nm0000123x"), None);
    }

    #[test]
    fn builds_a_grid_of_rated_episodes() {
        let dir = tempfile::tempdir().unwrap();
        let ratings = parse_ratings(&gz(RATINGS));
        assert_eq!(ratings.len(), 6, "the header row is skipped");
        assert_eq!(ratings[&2301451], (100, 240000));
        let n = build(&db_path(dir.path()), &ratings, &gz(EPISODES)).unwrap();
        assert_eq!(n, 3, "unrated episodes and unknown numbers are left out");
        let (eps, show_rating) = show(dir.path(), "tt0903747").unwrap();
        assert_eq!(eps.iter().map(|e| (e.season, e.number)).collect::<Vec<_>>(), vec![(1, 1), (1, 2), (5, 14)]);
        assert_eq!(eps[2].rating, 10.0);
        assert_eq!(eps[0].votes, 41000);
        assert_eq!(show_rating, Some((9.5, 2300000)));
        assert!(show(dir.path(), "tt0000001").is_none());
        assert!(!dir.path().join("imdb.db.tmp").exists());
    }

    #[test]
    fn a_failed_rebuild_keeps_the_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let ratings = parse_ratings(&gz(RATINGS));
        build(&db_path(dir.path()), &ratings, &gz(EPISODES)).unwrap();
        assert!(build(&db_path(dir.path()), &ratings, &gz("tconst\tparentTconst\tseasonNumber\tepisodeNumber\n")).is_err());
        assert_eq!(show(dir.path(), "tt0903747").unwrap().0.len(), 3);
    }

    #[tokio::test]
    async fn refresh_downloads_both_files_and_records_when() {
        let t = ctx();
        t.net.routes_bytes("title.ratings.tsv.gz", gz(RATINGS));
        t.net.routes_bytes("title.episode.tsv.gz", gz(EPISODES));
        assert!(is_due(&t.ctx));
        assert_eq!(refresh(&t.ctx).await.unwrap(), 3);
        assert!(!is_due(&t.ctx));
        assert!(show(&t.ctx.data_dir, "tt0903747").is_some());
        store::set_setting(&t.ctx.db, "imdb.enabled", &false).unwrap();
        store::set_setting(&t.ctx.db, "imdb.updated_at", &"2000-01-01T00:00:00Z").unwrap();
        assert!(!is_due(&t.ctx), "turned off means never due");
    }
}
