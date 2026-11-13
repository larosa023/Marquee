//! The on-disk image cache. Artwork is fetched once, kept under `<data>/images`, and evicted
//! least-recently-used first when the cache passes its size cap (1 GB by default).
//!
//! Only HTTPS URLs on known artwork hosts are fetched, so the cache can't be used to make the
//! app request arbitrary addresses.

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::now;
use crate::store;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const DEFAULT_CAP_MB: u64 = 1024;

const HOSTS: &[&str] =
    &["image.tmdb.org", "images.igdb.com", "coverartarchive.org", "archive.org", "steamstatic.com", "steampowered.com", "akamaihd.net", "scdn.co", "spotifycdn.com", "fastly.net", "last.fm", "ytimg.com", "ltrbxd.com", "cdn.myanimelist.net"];

pub fn allowed(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    if u.scheme() != "https" {
        return false;
    }
    let Some(host) = u.host_str() else { return false };
    HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

fn key(url: &str) -> String {
    Sha256::digest(url.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn path(ctx: &Ctx, key: &str) -> PathBuf {
    ctx.data_dir.join("images").join(&key[..2]).join(key)
}

fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', ..] => Some("image/avif"),
        _ => None,
    }
}

pub fn cap_bytes(ctx: &Ctx) -> u64 {
    store::setting_or(&ctx.db, "cache.image_cap_mb", DEFAULT_CAP_MB).clamp(50, 20_000) * 1024 * 1024
}

/// Returns the image at `url`, from disk if cached, fetching and caching it otherwise.
pub async fn get(ctx: &Ctx, url: &str) -> Result<(Vec<u8>, String)> {
    if !allowed(url) {
        return Err(Error::msg("That image address isn't one Marquee loads artwork from."));
    }
    let k = key(url);
    let file = path(ctx, &k);
    // A cached image is served straight from disk: no database lookup, so artwork never waits
    // behind a sync that is busy writing.
    if let Ok(bytes) = tokio::fs::read(&file).await {
        if let Some(mime) = sniff(&bytes) {
            // LRU bookkeeping is best effort: skipped if the database is busy, and at most hourly.
            if let Some(conn) = ctx.db.try_lock() {
                let stamp = now();
                let _ = conn.execute("UPDATE image_cache SET last_access = ?2 WHERE key = ?1 AND substr(last_access, 1, 13) != substr(?2, 1, 13)", params![k, stamp]);
            }
            return Ok((bytes, mime.to_string()));
        }
    }
    let resp = ctx.http.get("Artwork", url).send().await?;
    let mime = sniff(&resp.body).ok_or_else(|| Error::msg("That address didn't return an image."))?.to_string();
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(&file, &resp.body).await?;
    ctx.db.lock().execute("INSERT OR REPLACE INTO image_cache (key, bytes, mime, last_access) VALUES (?1, ?2, ?3, ?4)", params![k, resp.body.len() as i64, mime, now()])?;
    evict(ctx, cap_bytes(ctx))?;
    Ok((resp.body, mime))
}

pub fn size(ctx: &Ctx) -> (u64, u64) {
    ctx.db.lock().query_row("SELECT COALESCE(sum(bytes), 0), count(*) FROM image_cache", [], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))).unwrap_or((0, 0))
}

/// Deletes least-recently-used images until the cache is under 90% of `cap`.
pub fn evict(ctx: &Ctx, cap: u64) -> Result<usize> {
    let (total, _) = size(ctx);
    if total <= cap {
        return Ok(0);
    }
    let target = cap / 10 * 9;
    let victims: Vec<(String, u64)> = {
        let conn = ctx.db.lock();
        let mut stmt = conn.prepare("SELECT key, bytes FROM image_cache ORDER BY last_access ASC, key ASC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut remaining = total;
    let mut removed = 0;
    for (k, bytes) in victims {
        if remaining <= target {
            break;
        }
        let _ = std::fs::remove_file(path(ctx, &k));
        ctx.db.lock().execute("DELETE FROM image_cache WHERE key = ?1", [&k])?;
        remaining = remaining.saturating_sub(bytes);
        removed += 1;
    }
    Ok(removed)
}

pub fn clear(ctx: &Ctx) -> Result<()> {
    ctx.db.lock().execute("DELETE FROM image_cache", [])?;
    let dir = ctx.data_dir.join("images");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    const JPEG: &str = "\u{FF}\u{D8}\u{FF}";

    fn jpeg(extra: usize) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
        v.extend(std::iter::repeat_n(7u8, extra));
        v
    }

    #[test]
    fn only_https_artwork_hosts_are_fetched() {
        assert!(allowed("https://image.tmdb.org/t/p/w500/x.jpg"));
        assert!(allowed("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1/header.jpg"));
        assert!(allowed("https://i.scdn.co/image/abc"));
        assert!(allowed("https://cdn.myanimelist.net/images/anime/10/47347l.jpg"));
        assert!(!allowed("http://image.tmdb.org/t/p/w500/x.jpg"), "no plain http");
        assert!(!allowed("https://evil.example/x.jpg"));
        assert!(!allowed("https://image.tmdb.org.evil.example/x.jpg"));
        assert!(!allowed("file:///C:/Windows/win.ini"));
        assert!(!allowed("https://127.0.0.1/x.jpg"));
        let _ = JPEG;
    }

    #[tokio::test]
    async fn images_are_fetched_once_then_served_from_disk() {
        let t = ctx();
        let url = "https://image.tmdb.org/t/p/w500/a.jpg";
        t.net.routes_bytes(url, jpeg(100));
        let (a, mime) = get(&t.ctx, url).await.unwrap();
        assert_eq!(mime, "image/jpeg");
        t.net.set_offline(true);
        let (b, _) = get(&t.ctx, url).await.unwrap();
        assert_eq!(a, b);
        assert_eq!(t.net.calls("a.jpg"), 1);
        assert_eq!(size(&t.ctx), (104, 1));
    }

    #[tokio::test]
    async fn the_least_recently_used_images_are_evicted_first() {
        let t = ctx();
        for (n, name) in ["old", "mid", "new"].iter().enumerate() {
            let url = format!("https://image.tmdb.org/t/p/w500/{name}.jpg");
            t.net.routes_bytes(&url, jpeg(996));
            get(&t.ctx, &url).await.unwrap();
            t.ctx.db.lock().execute("UPDATE image_cache SET last_access = ?1 WHERE key = ?2", params![format!("2026-01-0{}T00:00:00Z", n + 1), key(&url)]).unwrap();
        }
        assert_eq!(size(&t.ctx).0, 3000);
        // A 2500-byte cap: evict down to 2250, which means dropping only the oldest.
        assert_eq!(evict(&t.ctx, 2500).unwrap(), 1);
        let left: Vec<String> = {
            let conn = t.ctx.db.lock();
            let mut s = conn.prepare("SELECT key FROM image_cache").unwrap();
            let rows = s.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            rows
        };
        assert!(!left.contains(&key("https://image.tmdb.org/t/p/w500/old.jpg")));
        assert_eq!(left.len(), 2);
        assert!(!path(&t.ctx, &key("https://image.tmdb.org/t/p/w500/old.jpg")).exists());
    }

    #[tokio::test]
    async fn a_page_that_is_not_an_image_is_rejected_and_not_cached() {
        let t = ctx();
        let url = "https://coverartarchive.org/release-group/x/front-500";
        t.net.on(url, 200, "<html>not found</html>");
        assert!(get(&t.ctx, url).await.is_err());
        assert_eq!(size(&t.ctx), (0, 0));
    }
}
