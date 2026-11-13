//! The one entry point the UI calls: `dispatch(ctx, method, params)`.
//!
//! Keeping every engine call behind a single function means the app shell stays thin, the whole
//! surface can be exercised in tests without a window, and the UI tests can swap in a fake.

use crate::ctx::{Ctx, API_KEYS};
use crate::error::{Error, Result};
use crate::integrations::{self, import, letterboxd, spotify, steam, trakt};
use crate::model::*;
use crate::{ai, backup, catalog, export, feeds, hangar, images, notifications, store, sync, taste, themes};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RESCORE_GEN: AtomicU64 = AtomicU64::new(0);

fn int(p: &Value, key: &str) -> Result<i64> {
    p[key].as_i64().ok_or_else(|| Error::msg(format!("Missing \"{key}\".")))
}

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| Error::msg(format!("Missing \"{key}\".")))
}

fn opt<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p[key].as_str().filter(|s| !s.is_empty())
}

fn filters(p: &Value) -> feeds::Filters {
    serde_json::from_value(p["filters"].clone()).unwrap_or_default()
}

/// Rebuilds the profile and scores shortly after the last change, so a burst of ratings
/// (Quick Rate) costs one rescore instead of twenty.
fn rescore_soon(ctx: &Arc<Ctx>) {
    let gen = RESCORE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let ctx = ctx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        if RESCORE_GEN.load(Ordering::SeqCst) == gen {
            let _ = tokio::task::spawn_blocking(move || sync::rescore(&ctx)).await;
        }
    });
}

fn key_status(ctx: &Ctx) -> Vec<Value> {
    API_KEYS
        .iter()
        .map(|(name, what, url)| {
            json!({ "name": name, "what": what, "url": url, "set": ctx.has_key(name), "custom": ctx.secrets.get(&format!("key.{name}")).is_some(), "built_in": ctx.default_keys.get(*name).map(|v| !v.is_empty()).unwrap_or(false) })
        })
        .collect()
}

fn bootstrap(ctx: &Ctx) -> Result<Value> {
    let settings = store::all_settings(&ctx.db)?;
    let conn = ctx.db.lock();
    let lists = store::lists(&conn)?;
    let unread: i64 = conn.query_row("SELECT count(*) FROM notifications WHERE read_at IS NULL", [], |r| r.get(0))?;
    let pending: i64 = conn.query_row("SELECT count(*) FROM pending_writes", [], |r| r.get(0))?;
    let has_taste: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM taste_profile)", [], |r| r.get(0))?;
    drop(conn);
    Ok(json!({
        "version": ctx.version,
        "settings": settings,
        "sources": integrations::statuses(ctx)?,
        "keys": key_status(ctx),
        "lists": lists,
        "themes": themes::all(&ctx.data_dir),
        "system_light": themes::system_light(),
        "unread": unread,
        "pending_writes": pending,
        "has_taste": has_taste,
        "onboarded": settings.get("onboarded").and_then(|v| v.as_bool()).unwrap_or(false),
        "data_dir": ctx.data_dir.to_string_lossy(),
    }))
}

/// Runs a sync unless one is already running.
pub async fn run_sync(ctx: &Arc<Ctx>, only: Option<&str>) -> Result<Value> {
    let Ok(_guard) = SYNC_LOCK.try_lock() else { return Ok(json!({ "already_running": true })) };
    ctx.set_busy(Some("syncing"));
    let result = sync::run(ctx, only).await;
    if result.is_ok() {
        let _ = backup::auto_if_due(ctx);
        if store::setting_or(&ctx.db, "calendar.auto_export", false) {
            let _ = export::write_calendar_file(ctx);
        }
    }
    ctx.set_busy(None);
    Ok(serde_json::to_value(result?)?)
}

/// After an import: give every imported title its details (in passes), then rescore.
fn enrich_in_background(ctx: &Arc<Ctx>) {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        for pass in 0..12 {
            ctx.emit("sync:progress", json!({ "label": "Matching your imported titles", "pct": 0.1 + 0.07 * pass as f64 }));
            match sync::enrich(&ctx, 250).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let c = ctx.clone();
                    let _ = tokio::task::spawn_blocking(move || sync::rescore(&c)).await;
                }
            }
        }
        let c = ctx.clone();
        let _ = tokio::task::spawn_blocking(move || sync::rescore(&c)).await;
        ctx.emit("sync:progress", json!({ "label": "Up to date", "pct": 1.0 }));
        ctx.emit("sync:done", json!({ "offline": false, "events": 0, "warnings": [] }));
    });
}

fn apply_import(ctx: &Ctx, source: &str, events: Vec<RawEvent>, wishlist: Vec<ItemDraft>) -> Result<usize> {
    let synced = integrations::Synced { events, ..Default::default() };
    let changed = sync::apply(ctx, source, &synced)?;
    let conn = ctx.db.lock();
    for d in &wishlist {
        if let Ok(id) = store::upsert_item(&conn, d) {
            let list = store::list_id(&conn, d.media().default_list())?;
            store::add_to_list(&conn, list, id, "local")?;
        }
    }
    Ok(changed)
}

/// Popular, recognisable titles across every medium for the "pick 10 things you love" step.
async fn seeds(ctx: &Ctx) -> Result<Value> {
    let mut drafts: Vec<ItemDraft> = vec![];
    let mut missing = vec![];
    if ctx.has_key("tmdb") {
        let (m, t) = futures::join!(catalog::tmdb::all_time(ctx, Media::Movie, 2), catalog::tmdb::all_time(ctx, Media::Tv, 2));
        drafts.extend(m.unwrap_or_default());
        drafts.extend(t.unwrap_or_default());
    } else {
        missing.push("tmdb");
    }
    if catalog::igdb::configured(ctx) {
        drafts.extend(catalog::igdb::all_time(ctx, 40).await.unwrap_or_default());
    } else {
        missing.push("igdb");
    }
    let mut ids = catalog::save_all(ctx, &drafts)?;
    if ctx.has_key("lastfm_key") {
        for tag in ["rock", "pop", "hip-hop", "electronic", "indie", "jazz", "metal", "ambient"] {
            ids.extend(catalog::albums_for_tag(ctx, tag, 6).await.unwrap_or_default());
        }
    } else {
        missing.push("lastfm");
    }
    let conn = ctx.db.lock();
    let cards = store::cards_by_ids(&conn, &ids)?;
    Ok(json!({ "cards": cards.into_iter().filter(|c| c.poster.is_some()).collect::<Vec<_>>(), "missing": missing }))
}

fn storage(ctx: &Ctx) -> Value {
    let (image_bytes, image_count) = images::size(ctx);
    let db_bytes = ctx.db.path().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
    let cache_rows: i64 = ctx.db.lock().query_row("SELECT count(*) FROM cache", [], |r| r.get(0)).unwrap_or(0);
    let backups: u64 = backup::list(ctx).iter().map(|b| b.bytes).sum();
    json!({ "images_bytes": image_bytes, "images": image_count, "image_cap_bytes": images::cap_bytes(ctx), "database_bytes": db_bytes, "cached_responses": cache_rows, "backups_bytes": backups, "imdb_bytes": catalog::imdb::size_bytes(&ctx.data_dir), "imdb_updated_at": store::get_setting::<String>(&ctx.db, "imdb.updated_at"), "data_dir": ctx.data_dir.to_string_lossy() })
}

fn diagnostics(ctx: &Ctx) -> Result<String> {
    let conn = ctx.db.lock();
    let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(-1);
    let mut out = format!(
        "Marquee {}\nSchema {}\nItems {} · Events {} · Lists {} · Pending writes {}\nData folder: {}\n\nConnections:\n",
        ctx.version,
        crate::db::schema_version(&conn)?,
        n("SELECT count(*) FROM items"),
        n("SELECT count(*) FROM user_events"),
        n("SELECT count(*) FROM lists"),
        n("SELECT count(*) FROM pending_writes"),
        ctx.data_dir.display()
    );
    let mut stmt = conn.prepare("SELECT id, status, last_sync, error FROM sources ORDER BY id")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)))? {
        let (id, status, last, err) = row?;
        out.push_str(&format!("  {id}: {status}{}{}\n", last.map(|l| format!(", last sync {l}")).unwrap_or_default(), err.map(|e| format!(", {e}")).unwrap_or_default()));
    }
    out.push_str("\nAPI keys (set or not; values are never included):\n");
    for (name, ..) in API_KEYS {
        out.push_str(&format!("  {name}: {}\n", if ctx.has_key(name) { "set" } else { "missing" }));
    }
    Ok(out)
}

fn read_file(path: &str) -> Result<(Vec<u8>, String)> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|_| Error::msg("That file couldn't be opened."))?;
    if meta.len() > 200 * 1024 * 1024 {
        return Err(Error::msg("That file is over 200 MB, which is more than Marquee imports."));
    }
    Ok((std::fs::read(p)?, p.file_name().and_then(|n| n.to_str()).unwrap_or("import").to_string()))
}

pub async fn dispatch(ctx: &Arc<Ctx>, method: &str, p: Value) -> Result<Value> {
    let c: &Ctx = ctx;
    Ok(match method {
        // ------------------------------------------------------------ app
        "bootstrap" => bootstrap(c)?,
        "settings.set" => {
            let key = text(&p, "key")?;
            store::set_setting(&c.db, key, &p["value"])?;
            if key == "taste" || key == "ai" {
                rescore_soon(ctx);
            }
            if key == "cache.image_cap_mb" {
                images::evict(c, images::cap_bytes(c))?;
            }
            c.emit("settings:changed", json!({ "key": key }));
            Value::Null
        }
        "keys.set" => {
            let name = text(&p, "name")?;
            if !API_KEYS.iter().any(|k| k.0 == name) {
                return Err(Error::msg("Marquee doesn't use a key by that name."));
            }
            match opt(&p, "value") {
                Some(v) => {
                    c.secrets.set(&format!("key.{name}"), v.trim())?;
                    // Titles skipped while this key was missing get looked up on the next sync.
                    c.db.lock().execute("UPDATE items SET data = json_remove(data, '$.enrich_tried') WHERE json_extract(data, '$.enrich_tried') IS NOT NULL", [])?;
                }
                None => c.secrets.delete(&format!("key.{name}"))?,
            }
            json!(key_status(c))
        }
        "keys" => json!(key_status(c)),
        "storage" => storage(c),
        "cache.clear" => {
            images::clear(c)?;
            c.http.clear_cache()?;
            storage(c)
        }
        "imdb.refresh" => {
            let episodes = catalog::imdb::refresh(c).await?;
            json!({ "episodes": episodes, "storage": storage(c) })
        }
        "imdb.remove" => {
            catalog::imdb::remove(&c.data_dir);
            store::set_setting(&c.db, "imdb.enabled", &false)?;
            storage(c)
        }
        "diagnostics" => json!(diagnostics(c)?),
        "providers" => json!(catalog::tmdb::provider_list(c).await?),
        "ai.status" => serde_json::to_value(ai::status(c).await)?,
        "hangar.manifest" => hangar::manifest_entry(),

        // ---------------------------------------------------------- feeds
        "home" => feeds::home(c)?,
        "for_you" => feeds::for_you(c, &filters(&p), p["offset"].as_u64().unwrap_or(0) as usize, p["limit"].as_u64().unwrap_or(40).min(100) as usize)?,
        "hub" => {
            let media = Media::parse(text(&p, "media")?).ok_or_else(|| Error::msg("Unknown media type."))?;
            feeds::hub(c, media, &filters(&p), p["offset"].as_u64().unwrap_or(0) as usize, p["limit"].as_u64().unwrap_or(60).min(120) as usize)?
        }
        "tracked_shows" => json!(feeds::tracked_shows(c)?),
        "calendar" => json!(feeds::calendar(c, text(&p, "from")?, text(&p, "to")?, opt(&p, "scope").unwrap_or("taste"))?),
        "quick_rate" => json!(feeds::quick_rate(c, p["limit"].as_u64().unwrap_or(24) as usize)?),
        "today" => json!(feeds::today_releases(c, p["limit"].as_i64().unwrap_or(8))?),
        "item" => feeds::detail(c, int(&p, "id")?).await?,
        "item.episodes" => feeds::episodes(c, int(&p, "id")?).await?,
        "item.blurb" => {
            let id = int(&p, "id")?;
            let (item, why, liked) = {
                let conn = c.db.lock();
                let item = store::get_item(&conn, id)?;
                let why: Vec<String> = conn.query_row("SELECT why FROM scores WHERE item_id = ?1", [id], |r| r.get::<_, String>(0)).ok().and_then(|w| serde_json::from_str(&w).ok()).unwrap_or_default();
                let mut stmt = conn.prepare("SELECT i.title FROM items i JOIN user_events e ON e.item_id = i.id WHERE e.kind = 'rating' AND e.value >= 4.5 ORDER BY e.at DESC LIMIT 8")?;
                let liked: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                (item, why, liked)
            };
            json!({ "blurb": ai::blurb(c, &item.title, item.media.as_str(), item.overview.as_deref().unwrap_or(""), &why, &liked).await })
        }
        "search" => {
            let media = opt(&p, "media").and_then(Media::parse);
            match catalog::search_everything(c, text(&p, "query")?, media, 12).await {
                Ok(ids) => json!({ "cards": store::cards_by_ids(&c.db.lock(), &ids)?, "offline": false }),
                Err(e) if e.is_offline() => json!({ "cards": [], "offline": true }),
                Err(e) => return Err(e),
            }
        }
        // Instant, local-only: what's already in the library and lists, as you type.
        "search.local" => {
            let q = text(&p, "query")?.to_lowercase().replace('%', "");
            let conn = c.db.lock();
            let cards = store::cards(
                &conn,
                "WHERE i.media != 'artist' AND (i.sort_title LIKE ?1 OR lower(i.title) LIKE ?2 OR lower(json_extract(i.data, '$.artist')) LIKE ?2)
                 ORDER BY (EXISTS (SELECT 1 FROM list_items li WHERE li.item_id = i.id)) DESC, (i.sort_title LIKE ?1) DESC, i.popularity DESC LIMIT 12",
                &[&format!("{}%", sort_title(&q)), &format!("%{q}%")],
            )?;
            let mut lists = conn.prepare("SELECT id, name FROM lists WHERE lower(name) LIKE ?1 LIMIT 5")?;
            let lists: Vec<Value> = lists.query_map([format!("%{q}%")], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)? })))?.collect::<rusqlite::Result<_>>()?;
            json!({ "cards": cards, "lists": lists })
        }
        "vibe" => serde_json::to_value(taste::vibe::search(c, text(&p, "query")?, 30).await?)?,

        // -------------------------------------------------------- actions
        "add" => {
            let out = sync::add_to(c, int(&p, "id")?, text(&p, "target")?).await?;
            rescore_soon(ctx);
            out
        }
        "remove" => sync::remove_from(c, int(&p, "id")?, int(&p, "list_id")?).await?,
        "rate" => {
            let out = sync::rate(c, int(&p, "id")?, p["stars"].as_f64().unwrap_or(0.0)).await?;
            rescore_soon(ctx);
            out
        }
        "done" => {
            let out = sync::mark_done(c, int(&p, "id")?, p["done"].as_bool().unwrap_or(true)).await?;
            rescore_soon(ctx);
            out
        }
        "dismiss" => {
            let id = int(&p, "id")?;
            let reason = opt(&p, "reason").unwrap_or("other");
            {
                let conn = c.db.lock();
                if reason == "seen" {
                    // Already seen it: that's history, not dislike.
                    store::record_event(&conn, id, "marquee", EventKind::Watched, 1.0, None, None, &Value::Null)?;
                } else {
                    store::record_event(&conn, id, "marquee", EventKind::Dismiss, 1.0, None, None, &json!({ "reason": reason }))?;
                    conn.execute("DELETE FROM scores WHERE item_id = ?1", [id])?;
                }
            }
            rescore_soon(ctx);
            Value::Null
        }
        "undismiss" => {
            store::remove_event(&c.db.lock(), int(&p, "id")?, "marquee", EventKind::Dismiss)?;
            rescore_soon(ctx);
            Value::Null
        }
        "more_like" => {
            store::record_event(&c.db.lock(), int(&p, "id")?, "marquee", EventKind::MoreLike, 1.0, None, None, &Value::Null)?;
            rescore_soon(ctx);
            Value::Null
        }
        "follow" => {
            let kind = text(&p, "kind")?;
            if !["item", "artist", "franchise", "person"].contains(&kind) {
                return Err(Error::msg("Marquee can follow a title, an artist or a franchise."));
            }
            let item_id = p["item_id"].as_i64();
            store::follow(&c.db.lock(), kind, text(&p, "reference")?, text(&p, "name")?, item_id)?;
            if kind == "artist" && p["on_spotify"] == true {
                if let Some(id) = item_id {
                    return Ok(json!({ "pushed": sync::push(c, "spotify", &integrations::WriteOp::Follow { item_id: id }).await }));
                }
            }
            Value::Null
        }
        "unfollow" => {
            store::unfollow(&c.db.lock(), text(&p, "kind")?, text(&p, "reference")?)?;
            Value::Null
        }
        "follows" => json!(store::follows(&c.db.lock())?),

        // ---------------------------------------------------------- lists
        "lists" => json!(store::lists(&c.db.lock())?),
        "list" => {
            let conn = c.db.lock();
            let id = int(&p, "id")?;
            let info = store::lists(&conn)?.into_iter().find(|l| l.id == id).ok_or_else(|| Error::msg("That list doesn't exist."))?;
            json!({ "info": info, "entries": store::list_entries(&conn, id)? })
        }
        "list.create" => json!({ "id": store::create_list(&c.db.lock(), text(&p, "name")?)? }),
        "list.rename" => {
            store::rename_list(&c.db.lock(), int(&p, "id")?, text(&p, "name")?)?;
            Value::Null
        }
        "list.delete" => {
            store::delete_list(&c.db.lock(), int(&p, "id")?)?;
            Value::Null
        }
        "list.reorder" => {
            let ids: Vec<i64> = p["item_ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            store::reorder_list(&c.db.lock(), int(&p, "id")?, &ids)?;
            Value::Null
        }
        "list.mirror" => {
            let mirror = opt(&p, "mirror");
            if let Some(m) = mirror {
                if !(m == "trakt:watchlist" || m == "steam:wishlist" || m.starts_with("spotify:playlist:")) {
                    return Err(Error::msg("A list can mirror your Trakt watchlist, your Steam wishlist or a Spotify playlist."));
                }
            }
            c.db.lock().execute("UPDATE lists SET mirror = ?2 WHERE id = ?1", rusqlite::params![int(&p, "id")?, mirror])?;
            Value::Null
        }
        "list.bulk" => {
            let list = int(&p, "id")?;
            let ids: Vec<i64> = p["item_ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            let action = text(&p, "action")?;
            let mut results = vec![];
            for id in ids {
                match action {
                    "remove" => results.push(sync::remove_from(c, id, list).await?),
                    "copy" | "move" => {
                        results.push(sync::add_to(c, id, &format!("list:{}", int(&p, "to")?)).await?);
                        if action == "move" {
                            sync::remove_from(c, id, list).await?;
                        }
                    }
                    "done" => results.push(sync::mark_done(c, id, true).await?),
                    _ => return Err(Error::msg("Unknown bulk action.")),
                }
            }
            rescore_soon(ctx);
            json!({ "count": results.len() })
        }
        "list.markdown" => json!(export::list_markdown(c, int(&p, "id")?)?),

        // ---------------------------------------------------------- taste
        "taste" => feeds::taste_overview(c)?,
        "taste.override" => {
            let feature = text(&p, "feature")?;
            let conn = c.db.lock();
            match opt(&p, "mode") {
                Some(mode @ ("pin" | "block")) => {
                    conn.execute("INSERT INTO taste_overrides (feature, mode) VALUES (?1, ?2) ON CONFLICT (feature) DO UPDATE SET mode = excluded.mode", [feature, mode])?;
                }
                _ => {
                    conn.execute("DELETE FROM taste_overrides WHERE feature = ?1", [feature])?;
                }
            }
            drop(conn);
            sync::rescore(c)?;
            Value::Null
        }
        "taste.reset" => {
            let conn = c.db.lock();
            let removed = match opt(&p, "source") {
                Some(source) => store::clear_source_events(&conn, source)?,
                None => {
                    conn.execute("DELETE FROM taste_overrides", [])?;
                    conn.execute("DELETE FROM user_events WHERE source = 'marquee' AND kind IN ('dismiss', 'more_like', 'pin', 'like')", [])?
                }
            };
            drop(conn);
            sync::rescore(c)?;
            json!({ "removed": removed })
        }
        "taste.seeds" => seeds(c).await?,
        "taste.seed" => {
            let ids: Vec<i64> = p["ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            {
                let conn = c.db.lock();
                for id in &ids {
                    store::record_event(&conn, *id, "marquee", EventKind::Like, 1.0, None, None, &json!({ "seed": true }))?;
                }
            }
            // Seeds are stubs: fetch their details so they carry real themes, then score.
            for id in ids.iter().take(20) {
                let _ = catalog::ensure_detail(c, *id).await;
            }
            json!({ "scored": sync::rescore(c)? })
        }

        // ---------------------------------------------------- connections
        "sources" => json!(integrations::statuses(c)?),
        "connect.steam" => json!({ "account": steam::connect(c).await? }),
        "connect.steam_id" => json!({ "account": steam::connect_with_id(c, text(&p, "steamid")?).await? }),
        "connect.trakt.start" => serde_json::to_value(trakt::device_start(c).await?)?,
        "connect.trakt.poll" => json!({ "account": trakt::device_poll(c, text(&p, "device_code")?).await? }),
        "connect.spotify" => json!({ "account": spotify::connect(c).await? }),
        "connect.lastfm" => json!({ "account": integrations::lastfm::connect(c, text(&p, "username")?).await? }),
        "connect.lastfm.auth_start" => json!({ "token": integrations::lastfm::auth_start(c).await? }),
        "connect.lastfm.auth_finish" => json!({ "account": integrations::lastfm::auth_finish(c, text(&p, "token")?).await? }),
        "connect.letterboxd" => json!({ "account": letterboxd::connect(c, text(&p, "username")?).await? }),
        "connect.mal" => json!({ "account": integrations::mal::connect(c, text(&p, "username")?).await? }),
        "connect.mal.signin" => json!({ "account": integrations::mal::sign_in(c).await? }),
        "disconnect" => {
            let source = text(&p, "source")?;
            if let Some(src) = integrations::get(source) {
                src.forget(c);
            }
            {
                let conn = c.db.lock();
                store::disconnect_source(&conn, source)?;
                if !p["keep_history"].as_bool().unwrap_or(false) {
                    store::clear_source_events(&conn, source)?;
                }
                conn.execute("UPDATE lists SET mirror = NULL WHERE mirror LIKE ?1", [format!("{source}:%")])?;
                conn.execute("DELETE FROM pending_writes WHERE source = ?1", [source])?;
            }
            sync::rescore(c)?;
            c.emit("sources:changed", json!({ "id": source }));
            Value::Null
        }
        "sync" => run_sync(ctx, opt(&p, "source")).await?,
        "sync.pending" => json!({ "count": sync::pending_count(c) }),
        "spotify.playlists" => json!(spotify::playlists(c).await?),
        "spotify.create_playlist" => json!({ "id": spotify::create_playlist(c, text(&p, "name")?, "Made in Marquee.").await? }),
        "letterboxd.export_steps" => json!(letterboxd::EXPORT_STEPS),

        // --------------------------------------------------------- import
        "import.preview" => {
            let (bytes, name) = read_file(text(&p, "path")?)?;
            serde_json::to_value(import::preview(&bytes, &name)?)?
        }
        "import.run" => {
            let (bytes, name) = read_file(text(&p, "path")?)?;
            let mapping: import::Mapping = serde_json::from_value(p["mapping"].clone()).map_err(|_| Error::msg("The column choices weren't understood."))?;
            c.set_busy(Some("importing"));
            let result = import::run(&bytes, &name, &mapping).and_then(|r| {
                let (events, wishlist, skipped) = (r.events.len(), r.wishlist.len(), r.skipped);
                let source = opt(&p, "source").unwrap_or("import");
                apply_import(c, source, r.events, r.wishlist)?;
                let conn = c.db.lock();
                store::set_source_config(&conn, source, json!({ "last_file": name, "imported_at": now() }))?;
                store::set_source(&conn, source, "connected", Some(&name), None)?;
                Ok(json!({ "events": events, "wishlist": wishlist, "skipped": skipped }))
            });
            c.set_busy(None);
            let out = result?;
            enrich_in_background(ctx);
            out
        }
        "import.letterboxd" => {
            let (bytes, name) = read_file(text(&p, "path")?)?;
            c.set_busy(Some("importing"));
            let result = letterboxd::parse_export(&bytes).and_then(|r| {
                let counts: Value = r.counts.iter().map(|(k, n)| (k.clone(), json!(n))).collect::<serde_json::Map<_, _>>().into();
                apply_import(c, "letterboxd", r.events, r.watchlist)?;
                let conn = c.db.lock();
                store::set_source_config(&conn, "letterboxd", json!({ "export_file": name, "export_imported_at": now(), "export_counts": counts }))?;
                if !store::is_connected(&conn, "letterboxd") {
                    store::set_source(&conn, "letterboxd", "connected", Some("Data export"), None)?;
                }
                Ok(json!({ "counts": counts }))
            });
            c.set_busy(None);
            let out = result?;
            enrich_in_background(ctx);
            out
        }

        // -------------------------------------------------- notifications
        "notifications" => json!(store::notifications(&c.db.lock(), p["limit"].as_i64().unwrap_or(100))?),
        "notifications.read" => {
            let conn = c.db.lock();
            match p["id"].as_i64() {
                Some(id) => conn.execute("UPDATE notifications SET read_at = ?2 WHERE id = ?1 AND read_at IS NULL", rusqlite::params![id, now()])?,
                None => conn.execute("UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL", [now()])?,
            };
            Value::Null
        }
        "notifications.clear" => {
            c.db.lock().execute("DELETE FROM notifications", [])?;
            Value::Null
        }
        "notifications.toast" => json!(notifications::toastable(&c.db)?),

        // ------------------------------------------------- backup, export
        "backup.create" => {
            let dest = opt(&p, "path").map(PathBuf::from);
            json!({ "path": backup::create(c, dest.as_deref(), false)?.to_string_lossy() })
        }
        "backup.list" => json!(backup::list(c)),
        "backup.preview" => backup::preview(c, Path::new(text(&p, "path")?))?,
        "backup.restore" => {
            c.set_busy(Some("restoring"));
            let result = backup::restore(c, Path::new(text(&p, "path")?));
            c.set_busy(None);
            let out = result?;
            sync::rescore(c)?;
            out
        }
        "export.library" => {
            let body = if opt(&p, "format") == Some("json") { export::library_json(c)? } else { export::library_csv(c)? };
            std::fs::write(text(&p, "path")?, body)?;
            Value::Null
        }
        "export.ics" => {
            std::fs::write(text(&p, "path")?, export::ics(c, opt(&p, "scope").unwrap_or("lists"))?)?;
            Value::Null
        }
        "export.calendar_file" => json!({ "path": export::write_calendar_file(c)?.to_string_lossy() }),
        "data.delete_all" => {
            if p["confirm"] != "DELETE" {
                return Err(Error::msg("Type DELETE to confirm."));
            }
            for src in integrations::all() {
                src.forget(c);
            }
            backup::delete_everything(c)?;
            Value::Null
        }

        // --------------------------------------------------------- themes
        "themes" => json!(themes::all(&c.data_dir)),
        "theme.save" => serde_json::to_value(themes::save(&c.data_dir, &p["theme"])?)?,
        "theme.delete" => {
            themes::delete(&c.data_dir, text(&p, "id")?)?;
            Value::Null
        }
        "theme.duplicate" => serde_json::to_value(themes::duplicate(&c.data_dir, text(&p, "id")?)?)?,
        "theme.import" => serde_json::to_value(themes::import(&c.data_dir, Path::new(text(&p, "path")?))?)?,
        "theme.export" => {
            themes::export(&c.data_dir, text(&p, "id")?, Path::new(text(&p, "path")?))?;
            Value::Null
        }

        other => return Err(Error::msg(format!("Marquee doesn't have an action called \"{other}\"."))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    async fn call(t: &crate::ctx::testing::TestCtx, method: &str, p: Value) -> Result<Value> {
        dispatch(&t.ctx, method, p).await
    }

    #[tokio::test]
    async fn bootstrap_describes_a_fresh_install() {
        let t = ctx();
        let b = call(&t, "bootstrap", json!({})).await.unwrap();
        assert_eq!(b["onboarded"], false);
        assert_eq!(b["has_taste"], false);
        assert_eq!(b["lists"].as_array().unwrap().len(), 4);
        assert_eq!(b["sources"].as_array().unwrap().len(), 8);
        assert!(b["themes"].as_array().unwrap().len() >= 9);
        assert!(b["keys"].as_array().unwrap().iter().all(|k| k["set"] == true && k["custom"] == false));
        // Key values never appear anywhere in what the UI receives.
        assert!(!b.to_string().contains("test-tmdb"));
    }

    #[tokio::test]
    async fn keys_are_stored_as_secrets_and_only_their_presence_is_reported() {
        let t = ctx();
        let keys = call(&t, "keys.set", json!({ "name": "tmdb", "value": "  my-real-key " })).await.unwrap();
        assert_eq!(t.ctx.secrets.get("key.tmdb").as_deref(), Some("my-real-key"));
        assert!(keys.as_array().unwrap().iter().any(|k| k["name"] == "tmdb" && k["custom"] == true));
        assert!(!keys.to_string().contains("my-real-key"));
        let settings: i64 = t.ctx.db.lock().query_row("SELECT count(*) FROM settings WHERE value LIKE '%my-real-key%'", [], |r| r.get(0)).unwrap();
        assert_eq!(settings, 0, "never in SQLite");
        call(&t, "keys.set", json!({ "name": "tmdb", "value": "" })).await.unwrap();
        assert_eq!(t.ctx.secrets.get("key.tmdb"), None);
        assert!(call(&t, "keys.set", json!({ "name": "aws", "value": "x" })).await.is_err());
    }

    #[tokio::test]
    async fn the_whole_loop_rate_recommend_save_export() {
        let t = ctx();
        // A tiny catalog.
        let (arrival, dune, romcom) = {
            let conn = t.ctx.db.lock();
            let mk = |title: &str, genres: &[&str], tags: &[&str], days: i64| {
                let mut d = ItemDraft::new(Media::Movie, title).id("tmdb", title.len() as i64 * 1000 + days.unsigned_abs() as i64);
                d.genres = genres.iter().map(|s| s.to_string()).collect();
                d.tags = tags.iter().map(|s| s.to_string()).collect();
                d.release_date = Some(days_from_today(days));
                d.detail_level = 1;
                d.poster = Some("https://image.tmdb.org/t/p/w500/x.jpg".into());
                store::upsert_item(&conn, &d).unwrap()
            };
            (mk("Arrival", &["Science Fiction"], &["space", "slow burn"], -3000), mk("Dune: Part Three", &["Science Fiction"], &["space", "epic"], 20), mk("Meet Cute", &["Romance"], &[], 20))
        };
        call(&t, "rate", json!({ "id": arrival, "stars": 5 })).await.unwrap();
        sync::rescore(&t.ctx).unwrap();
        let feed = call(&t, "for_you", json!({ "filters": { "status": "upcoming" } })).await.unwrap();
        assert_eq!(feed["cards"][0]["id"], dune);
        assert!(feed["cards"][0]["why"][0].as_str().unwrap().contains("*Arrival* 5★"));

        // Not interested removes it from the feed at once.
        call(&t, "dismiss", json!({ "id": romcom, "reason": "genre" })).await.unwrap();
        let feed = call(&t, "for_you", json!({ "filters": { "status": "upcoming" } })).await.unwrap();
        assert!(!feed["cards"].as_array().unwrap().iter().any(|c| c["id"] == romcom));

        // Save to a Marquee list, then read it back and export it.
        let lists = call(&t, "lists", json!({})).await.unwrap();
        let watchlist = lists.as_array().unwrap().iter().find(|l| l["slug"] == "watchlist").unwrap()["id"].as_i64().unwrap();
        let added = call(&t, "add", json!({ "id": dune, "target": format!("list:{watchlist}") })).await.unwrap();
        assert_eq!(added["sync_state"], "local");
        let list = call(&t, "list", json!({ "id": watchlist })).await.unwrap();
        assert_eq!(list["entries"][0]["title"], "Dune: Part Three");
        assert_eq!(list["info"]["count"], 1);
        let md = call(&t, "list.markdown", json!({ "id": watchlist })).await.unwrap();
        assert!(md.as_str().unwrap().contains("**Dune: Part Three**"));
        let out = t.dir.join("library.csv");
        call(&t, "export.library", json!({ "format": "csv", "path": out.to_string_lossy() })).await.unwrap();
        assert!(std::fs::read_to_string(out).unwrap().contains("Arrival"));

        // Search finds it locally without touching the network.
        let before = t.net.log.lock().unwrap().len();
        let found = call(&t, "search.local", json!({ "query": "dune" })).await.unwrap();
        assert_eq!(found["cards"][0]["id"], dune);
        assert_eq!(t.net.log.lock().unwrap().len(), before);
    }

    #[tokio::test]
    async fn importing_a_file_records_history_and_fills_wishlists() {
        let t = ctx();
        let file = t.dir.join("my-games.csv");
        std::fs::write(&file, "Game,Status,Score\nOuter Wilds,Completed,10\nHades II,Wishlist,\n").unwrap();
        let preview = call(&t, "import.preview", json!({ "path": file.to_string_lossy() })).await.unwrap();
        assert_eq!(preview["rows"], 2);
        let out = call(&t, "import.run", json!({ "path": file.to_string_lossy(), "mapping": preview["mapping"] })).await.unwrap();
        assert_eq!((out["events"].as_i64(), out["wishlist"].as_i64()), (Some(3), Some(1)));
        let conn = t.ctx.db.lock();
        let play_later = store::list_id(&conn, "play-later").unwrap();
        assert_eq!(store::list_entries(&conn, play_later).unwrap()[0].card.title, "Hades II");
        let rated: f64 = conn.query_row("SELECT e.value FROM user_events e JOIN items i ON i.id = e.item_id WHERE i.title = 'Outer Wilds' AND e.kind = 'rating'", [], |r| r.get(0)).unwrap();
        assert_eq!(rated, 5.0);
        assert_eq!(store::source_row(&conn, "import").unwrap().0, "connected");
        assert_eq!(t.ctx.busy_reason(), None);
    }

    #[tokio::test]
    async fn disconnecting_forgets_tokens_history_and_mirrors() {
        let t = ctx();
        t.ctx.secrets.set("trakt.access", "acc").unwrap();
        {
            let conn = t.ctx.db.lock();
            store::set_source(&conn, "trakt", "connected", Some("larosa"), None).unwrap();
            conn.execute("UPDATE lists SET mirror = 'trakt:watchlist' WHERE slug = 'watchlist'", []).unwrap();
            let id = store::upsert_item(&conn, &ItemDraft::new(Media::Movie, "A").id("tmdb", 1)).unwrap();
            store::record_event(&conn, id, "trakt", EventKind::Watched, 1.0, None, None, &Value::Null).unwrap();
        }
        call(&t, "disconnect", json!({ "source": "trakt" })).await.unwrap();
        assert_eq!(t.ctx.secrets.get("trakt.access"), None);
        let conn = t.ctx.db.lock();
        assert_eq!(store::source_row(&conn, "trakt").unwrap().0, "disconnected");
        assert_eq!(conn.query_row("SELECT count(*) FROM user_events", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(conn.query_row("SELECT mirror FROM lists WHERE slug = 'watchlist'", [], |r| r.get::<_, Option<String>>(0)).unwrap(), None);
    }

    #[tokio::test]
    async fn pins_and_blocks_change_recommendations_immediately() {
        let t = ctx();
        let id = {
            let conn = t.ctx.db.lock();
            let mut d = ItemDraft::new(Media::Game, "Turnip Hollow").id("steam", 5);
            d.tags = vec!["cozy".into()];
            d.release_date = Some(days_from_today(3));
            let id = store::upsert_item(&conn, &d).unwrap();
            let liked = store::upsert_item(&conn, &ItemDraft::new(Media::Game, "Other").id("steam", 6)).unwrap();
            store::record_event(&conn, liked, "marquee", EventKind::Like, 1.0, None, None, &Value::Null).unwrap();
            id
        };
        call(&t, "taste.override", json!({ "feature": "t:cozy", "mode": "pin" })).await.unwrap();
        let pct = |t: &crate::ctx::testing::TestCtx| t.ctx.db.lock().query_row("SELECT match_pct FROM scores WHERE item_id = ?1", [id], |r| r.get::<_, i64>(0)).ok();
        assert!(pct(&t).unwrap() > 70);
        call(&t, "taste.override", json!({ "feature": "t:cozy", "mode": "block" })).await.unwrap();
        assert_eq!(pct(&t), None, "blocked tags are never recommended");
        call(&t, "taste.override", json!({ "feature": "t:cozy", "mode": null })).await.unwrap();
        assert!(pct(&t).is_some());
    }

    #[tokio::test]
    async fn delete_all_needs_typed_confirmation_and_wipes_tokens() {
        let t = ctx();
        t.ctx.secrets.set("spotify.refresh", "r").unwrap();
        t.ctx.secrets.set("key.tmdb", "k").unwrap();
        store::set_setting(&t.ctx.db, "onboarded", &true).unwrap();
        assert!(call(&t, "data.delete_all", json!({})).await.is_err());
        call(&t, "data.delete_all", json!({ "confirm": "DELETE" })).await.unwrap();
        assert_eq!(t.ctx.secrets.get("spotify.refresh"), None);
        assert_eq!(t.ctx.secrets.get("key.tmdb"), None);
        assert_eq!(call(&t, "bootstrap", json!({})).await.unwrap()["onboarded"], false);
    }

    #[tokio::test]
    async fn only_one_sync_runs_at_a_time_and_unknown_methods_are_named() {
        let t = ctx();
        t.net.set_offline(true);
        let (a, b) = tokio::join!(run_sync(&t.ctx, None), run_sync(&t.ctx, None));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert!(a["already_running"] == true || b["already_running"] == true);
        assert!(a.get("scored").is_some() || b.get("scored").is_some());
        let err = call(&t, "launch.missiles", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("launch.missiles"));
    }
}
