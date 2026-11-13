//! Backup and restore. A `.marqueebackup` file is a zip holding a consistent copy of the
//! database (without caches), the custom themes and a small manifest. Secrets are never in it:
//! tokens and API keys live in Windows Credential Manager and stay there.

use crate::ctx::Ctx;
use crate::db;
use crate::error::{Error, Result};
use crate::model::now;
use crate::store;
use crate::themes;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "marqueebackup";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSettings {
    pub auto: bool,
    pub every_days: i64,
    /// How many automatic backups to keep.
    pub keep: usize,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings { auto: true, every_days: 7, keep: 5 }
    }
}

pub fn dir(ctx: &Ctx) -> PathBuf {
    ctx.data_dir.join("backups")
}

fn counts(conn: &rusqlite::Connection) -> Value {
    let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    json!({
        "items": n("SELECT count(*) FROM items WHERE media != 'artist'"),
        "events": n("SELECT count(*) FROM user_events"),
        "ratings": n("SELECT count(*) FROM user_events WHERE kind = 'rating'"),
        "lists": n("SELECT count(*) FROM lists"),
        "list_items": n("SELECT count(*) FROM list_items"),
        "follows": n("SELECT count(*) FROM follows"),
        "connections": n("SELECT count(*) FROM sources WHERE status != 'disconnected'"),
    })
}

/// Writes a backup. `dest` defaults to `<data>/backups/marquee-<timestamp>.marqueebackup`.
pub fn create(ctx: &Ctx, dest: Option<&Path>, automatic: bool) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = match dest {
        Some(d) => d.to_path_buf(),
        None => dir(ctx).join(format!("{}-{stamp}.{EXTENSION}", if automatic { "auto" } else { "marquee" })),
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A consistent snapshot, even while the app keeps writing.
    let tmp = ctx.data_dir.join(format!("backup-{}.tmp.db", rand::random::<u32>()));
    let _ = std::fs::remove_file(&tmp);
    let manifest = {
        let conn = ctx.db.lock();
        conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])?;
        json!({ "app": "marquee", "format": 1, "version": ctx.version, "schema": db::schema_version(&conn)?, "created_at": now(), "automatic": automatic, "counts": counts(&conn) })
    };
    let result = (|| -> Result<()> {
        {
            // Caches are rebuilt on demand and would only make the file large.
            let snap = rusqlite::Connection::open(&tmp)?;
            snap.execute_batch("DELETE FROM cache; DELETE FROM image_cache; DELETE FROM embeddings; DELETE FROM pending_writes; VACUUM;")?;
        }
        let file = std::fs::File::create(&dest)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts)?;
        zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        zip.start_file("marquee.db", opts)?;
        zip.write_all(&std::fs::read(&tmp)?)?;
        if let Ok(entries) = std::fs::read_dir(themes::dir(&ctx.data_dir)) {
            for e in entries.flatten() {
                if let (Some(name), Ok(bytes)) = (e.file_name().to_str(), std::fs::read(e.path())) {
                    zip.start_file(format!("themes/{name}"), opts)?;
                    zip.write_all(&bytes)?;
                }
            }
        }
        zip.finish()?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result?;
    if automatic {
        prune(ctx)?;
        store::set_setting(&ctx.db, "backup.last_auto", &now())?;
    }
    Ok(dest)
}

#[derive(Debug, Serialize)]
pub struct BackupFile {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub created_at: String,
    pub automatic: bool,
}

pub fn list(ctx: &Ctx) -> Vec<BackupFile> {
    let mut out = vec![];
    let Ok(entries) = std::fs::read_dir(dir(ctx)) else { return out };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let created: chrono::DateTime<chrono::Utc> = meta.modified().map(Into::into).unwrap_or_else(|_| chrono::Utc::now());
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        out.push(BackupFile { path: path.to_string_lossy().to_string(), automatic: name.starts_with("auto-"), name, bytes: meta.len(), created_at: created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true) });
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// Keeps the newest N automatic backups. Manual ones are never deleted.
fn prune(ctx: &Ctx) -> Result<()> {
    let keep = store::setting_or(&ctx.db, "backup", BackupSettings::default()).keep.max(1);
    for old in list(ctx).into_iter().filter(|b| b.automatic).skip(keep) {
        let _ = std::fs::remove_file(old.path);
    }
    Ok(())
}

/// Makes an automatic backup if one is due. Called after each sync.
pub fn auto_if_due(ctx: &Ctx) -> Result<Option<PathBuf>> {
    let s: BackupSettings = store::setting_or(&ctx.db, "backup", BackupSettings::default());
    if !s.auto {
        return Ok(None);
    }
    let has_data: bool = ctx.db.lock().query_row("SELECT EXISTS (SELECT 1 FROM user_events) OR EXISTS (SELECT 1 FROM list_items)", [], |r| r.get(0))?;
    if !has_data {
        return Ok(None);
    }
    let last: Option<String> = store::get_setting(&ctx.db, "backup.last_auto");
    let due = match last.and_then(|l| chrono::DateTime::parse_from_rfc3339(&l).ok()) {
        Some(l) => (chrono::Utc::now() - l.with_timezone(&chrono::Utc)).num_days() >= s.every_days.max(1),
        None => true,
    };
    if due {
        create(ctx, None, true).map(Some)
    } else {
        Ok(None)
    }
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut f = zip.by_name(name).map_err(|_| Error::msg("That file isn't a Marquee backup."))?;
    let mut bytes = vec![];
    f.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn open(path: &Path) -> Result<(zip::ZipArchive<std::fs::File>, Value)> {
    let file = std::fs::File::open(path).map_err(|_| Error::msg("That backup file couldn't be opened."))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| Error::msg("That file isn't a Marquee backup."))?;
    let manifest: Value = serde_json::from_slice(&read_entry(&mut zip, "manifest.json")?).map_err(|_| Error::msg("That backup's manifest is damaged."))?;
    if manifest["app"] != "marquee" {
        return Err(Error::msg("That file isn't a Marquee backup."));
    }
    if manifest["schema"].as_u64().unwrap_or(0) > db::latest_version() as u64 {
        return Err(Error::msg(format!("That backup was made by a newer Marquee ({}). Update Marquee, then restore it.", manifest["version"].as_str().unwrap_or("unknown"))));
    }
    Ok((zip, manifest))
}

/// What's inside a backup, and what restoring it would replace.
pub fn preview(ctx: &Ctx, path: &Path) -> Result<Value> {
    let (zip, manifest) = open(path)?;
    let themes: Vec<String> = zip.file_names().filter_map(|n| n.strip_prefix("themes/")).filter(|n| !n.is_empty()).map(String::from).collect();
    let current = counts(&ctx.db.lock());
    Ok(json!({ "manifest": manifest, "themes": themes, "current": current, "path": path.to_string_lossy() }))
}

/// Replaces everything with the backup's contents. The current data is backed up first, so a
/// restore can always be undone.
pub fn restore(ctx: &Ctx, path: &Path) -> Result<Value> {
    let (mut zip, manifest) = open(path)?;
    let safety = create(ctx, Some(&dir(ctx).join(format!("before-restore-{}.{EXTENSION}", chrono::Local::now().format("%Y%m%d-%H%M%S")))), false)?;

    let tmp = ctx.data_dir.join(format!("restore-{}.tmp.db", rand::random::<u32>()));
    std::fs::write(&tmp, read_entry(&mut zip, "marquee.db")?)?;
    let result = (|| -> Result<()> {
        {
            // Bring an older backup up to the current schema before copying rows across.
            let mut snap = rusqlite::Connection::open(&tmp)?;
            db::migrate(&mut snap)?;
        }
        let conn = ctx.db.lock();
        conn.execute("ATTACH DATABASE ?1 AS bak", [tmp.to_string_lossy().as_ref()])?;
        let copy = (|| -> Result<()> {
            let tables: Vec<String> = {
                let mut stmt = conn.prepare("SELECT name FROM bak.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT IN ('cache', 'image_cache')")?;
                let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            conn.execute_batch("PRAGMA foreign_keys = OFF; BEGIN;")?;
            for t in &tables {
                let exists: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM main.sqlite_master WHERE type = 'table' AND name = ?1)", [t], |r| r.get(0))?;
                if exists {
                    conn.execute(&format!("DELETE FROM main.\"{t}\""), [])?;
                    conn.execute(&format!("INSERT INTO main.\"{t}\" SELECT * FROM bak.\"{t}\""), [])?;
                }
            }
            conn.execute_batch("COMMIT;")?;
            Ok(())
        })();
        if copy.is_err() {
            let _ = conn.execute_batch("ROLLBACK;");
        }
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");
        let _ = conn.execute("DETACH DATABASE bak", []);
        copy?;
        // Tokens aren't in a backup, so restored connections need signing in again where a token is required.
        for (source, secret) in [("trakt", "trakt.access"), ("spotify", "spotify.access")] {
            if ctx.secrets.get(secret).is_none() {
                conn.execute("UPDATE sources SET status = 'error', error = 'Sign in again to reconnect after restoring.' WHERE id = ?1 AND status != 'disconnected'", [source])?;
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result?;

    let theme_dir = themes::dir(&ctx.data_dir);
    let names: Vec<String> = zip.file_names().filter(|n| n.starts_with("themes/") && n.len() > 7).map(String::from).collect();
    for name in names {
        // Only plain file names: never write outside the themes folder.
        let Some(base) = Path::new(&name).file_name().and_then(|b| b.to_str()).filter(|b| !b.contains("..")) else { continue };
        if let Ok(bytes) = read_entry(&mut zip, &name) {
            std::fs::create_dir_all(&theme_dir)?;
            std::fs::write(theme_dir.join(base), bytes)?;
        }
    }
    ctx.emit("data:restored", json!({}));
    Ok(json!({ "restored": manifest, "safety_backup": safety.to_string_lossy() }))
}

/// "Delete all my data": the database contents, the caches, the themes, the backups and every
/// stored token and key.
pub fn delete_everything(ctx: &Ctx) -> Result<()> {
    for key in crate::secrets::SECRET_KEYS {
        let _ = ctx.secrets.delete(key);
    }
    {
        let conn = ctx.db.lock();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             BEGIN;
             DELETE FROM user_events; DELETE FROM list_items; DELETE FROM lists WHERE kind = 'custom'; UPDATE lists SET mirror = NULL;
             DELETE FROM follows; DELETE FROM notifications; DELETE FROM items; DELETE FROM item_ids; DELETE FROM item_features; DELETE FROM item_similar;
             DELETE FROM cache; DELETE FROM image_cache; DELETE FROM settings; DELETE FROM pending_writes; DELETE FROM taste_profile; DELETE FROM taste_overrides;
             DELETE FROM scores; DELETE FROM price_history; DELETE FROM embeddings;
             UPDATE sources SET status = 'disconnected', account = NULL, last_sync = NULL, error = NULL, config = '{}';
             COMMIT;
             PRAGMA foreign_keys = ON;",
        )?;
        let _ = conn.execute_batch("VACUUM;");
    }
    for sub in ["images", "themes", "backups"] {
        let p = ctx.data_dir.join(sub);
        if p.exists() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
    let _ = std::fs::remove_file(ctx.data_dir.join("marquee.ics"));
    crate::catalog::imdb::remove(&ctx.data_dir);
    ctx.emit("data:restored", json!({ "wiped": true }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::{ctx, TestCtx};
    use crate::model::*;

    fn with_data() -> (TestCtx, i64) {
        let t = ctx();
        let id = {
            let conn = t.ctx.db.lock();
            let id = store::upsert_item(&conn, &ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865)).unwrap();
            store::record_event(&conn, id, "marquee", EventKind::Rating, 5.0, None, None, &Value::Null).unwrap();
            store::add_to_list(&conn, store::list_id(&conn, "favorites").unwrap(), id, "local").unwrap();
            let custom = store::create_list(&conn, "Rainy Sunday").unwrap();
            store::add_to_list(&conn, custom, id, "local").unwrap();
            conn.execute("INSERT INTO cache (key, body, fetched_at, expires_at) VALUES ('k', x'00', 'now', 'later')", []).unwrap();
            id
        };
        store::set_setting(&t.ctx.db, "region", &"US").unwrap();
        themes::duplicate(&t.ctx.data_dir, "synthwave").unwrap();
        t.ctx.secrets.set("trakt.access", "super-secret-token").unwrap();
        (t, id)
    }

    #[test]
    fn a_backup_restores_everything_but_holds_no_secrets_or_caches() {
        let (t, id) = with_data();
        let file = create(&t.ctx, None, false).unwrap();
        assert!(file.extension().unwrap() == EXTENSION);
        let raw = std::fs::read(&file).unwrap();
        assert!(!raw.windows(18).any(|w| w == b"super-secret-token"));

        let p = preview(&t.ctx, &file).unwrap();
        assert_eq!(p["manifest"]["counts"]["ratings"], 1);
        assert_eq!(p["manifest"]["counts"]["lists"], 5);
        assert_eq!(p["themes"].as_array().unwrap().len(), 1);

        // Lose everything, then restore.
        delete_everything(&t.ctx).unwrap();
        assert!(store::get_item(&t.ctx.db.lock(), id).is_err());
        assert_eq!(t.ctx.secrets.get("trakt.access"), None);
        // delete_everything removed the backups folder too, so the file was copied out first in real use;
        // here it was written before the wipe, so re-create from the bytes we kept.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &raw).unwrap();

        let out = restore(&t.ctx, &file).unwrap();
        assert!(out["safety_backup"].as_str().unwrap().contains("before-restore"));
        let conn = t.ctx.db.lock();
        assert_eq!(store::get_item(&conn, id).unwrap().title, "Arrival");
        assert_eq!(store::user_rating(&conn, id), Some(5.0));
        assert_eq!(store::lists(&conn).unwrap().len(), 5);
        assert_eq!(conn.query_row("SELECT count(*) FROM cache", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        drop(conn);
        assert_eq!(store::get_setting::<String>(&t.ctx.db, "region").as_deref(), Some("US"));
        assert_eq!(themes::custom(&t.ctx.data_dir).len(), 1);
        assert!(t.host.events.lock().unwrap().iter().any(|(e, _)| e == "data:restored"));
    }

    #[test]
    fn automatic_backups_run_when_due_and_keep_only_the_newest() {
        let (t, _) = with_data();
        store::set_setting(&t.ctx.db, "backup", &json!({ "auto": true, "every_days": 7, "keep": 2 })).unwrap();
        let first = auto_if_due(&t.ctx).unwrap();
        assert!(first.is_some());
        assert!(auto_if_due(&t.ctx).unwrap().is_none(), "not due again yet");
        // Pretend three older automatic backups exist.
        for n in 1..=3 {
            std::fs::write(dir(&t.ctx).join(format!("auto-2020010{n}-000000.{EXTENSION}")), b"old").unwrap();
        }
        let manual = create(&t.ctx, None, false).unwrap();
        store::set_setting(&t.ctx.db, "backup.last_auto", &"2020-01-01T00:00:00Z").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100)); // file names are per-second
        auto_if_due(&t.ctx).unwrap().unwrap();
        let files = list(&t.ctx);
        assert_eq!(files.iter().filter(|f| f.automatic).count(), 2);
        assert!(manual.exists(), "manual backups are never pruned");
    }

    #[test]
    fn an_empty_library_is_not_backed_up_automatically() {
        let t = ctx();
        assert!(auto_if_due(&t.ctx).unwrap().is_none());
    }

    #[test]
    fn wrong_and_too_new_files_are_refused_with_a_reason() {
        let (t, _) = with_data();
        let junk = t.dir.join("junk.marqueebackup");
        std::fs::write(&junk, b"nope").unwrap();
        assert!(preview(&t.ctx, &junk).unwrap_err().to_string().contains("isn't a Marquee backup"));
        let newer = t.dir.join("newer.marqueebackup");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&newer).unwrap());
            z.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(br#"{"app":"marquee","schema":999,"version":"9.0.0"}"#).unwrap();
            z.finish().unwrap();
        }
        assert!(restore(&t.ctx, &newer).unwrap_err().to_string().contains("newer Marquee (9.0.0)"));
    }
}
