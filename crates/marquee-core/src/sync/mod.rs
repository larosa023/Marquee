//! The sync engine: pulls from every connected service, refreshes the catalog, fills in
//! details, rebuilds the taste profile and scores, and replays writes made while offline.

use crate::catalog;
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::integrations::{self, Synced, WriteOp, WriteOutcome};
use crate::model::*;
use crate::notifications;
use crate::store;
use crate::taste;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;

fn progress(ctx: &Ctx, label: &str, pct: f64) {
    ctx.emit("sync:progress", json!({ "label": label, "pct": (pct * 100.0).round() / 100.0 }));
}

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub sources: Vec<Value>,
    pub catalog: Option<catalog::RefreshReport>,
    pub events: usize,
    pub scored: usize,
    pub offline: bool,
    pub warnings: Vec<String>,
}

/// Sources whose sync returns the complete current state, so anything missing was removed remotely.
fn is_full_state(source: &str) -> bool {
    matches!(source, "steam" | "trakt" | "spotify" | "lastfm")
}

/// Stores what a source returned: items, events and mirrored lists.
pub fn apply(ctx: &Ctx, source: &str, synced: &Synced) -> Result<usize> {
    let conn = ctx.db.lock();
    let tx = conn.unchecked_transaction()?;
    let mut seen: HashSet<(i64, &'static str)> = HashSet::new();
    let mut changed = 0;
    for ev in &synced.events {
        let Ok(id) = store::upsert_item(&tx, &ev.draft) else { continue };
        if store::record_event(&tx, id, source, ev.kind, ev.value, ev.at.as_deref(), ev.first_at.as_deref(), &ev.meta)? {
            changed += 1;
        }
        seen.insert((id, ev.kind.as_str()));
    }
    if is_full_state(source) && !synced.events.is_empty() {
        let mut stmt = tx.prepare("SELECT id, item_id, kind FROM user_events WHERE source = ?1")?;
        let stale: Vec<i64> = stmt
            .query_map([source], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?
            .filter_map(|r| r.ok())
            .filter(|(_, item, kind)| !seen.iter().any(|(i, k)| i == item && k == kind))
            .map(|r| r.0)
            .collect();
        drop(stmt);
        for id in stale {
            tx.execute("DELETE FROM user_events WHERE id = ?1", [id])?;
            changed += 1;
        }
    }

    for (target, drafts) in &synced.lists {
        let remote: Vec<i64> = drafts.iter().filter_map(|d| store::upsert_item(&tx, d).ok()).collect();
        if target == "spotify:following" {
            for (d, id) in drafts.iter().zip(&remote) {
                store::follow(&tx, "artist", &slug(&d.title), &d.title, Some(*id))?;
            }
            continue;
        }
        let mut lists = tx.prepare("SELECT id FROM lists WHERE mirror = ?1")?;
        let list_ids: Vec<i64> = lists.query_map([target], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        drop(lists);
        for list in list_ids {
            reconcile_list(&tx, list, &remote)?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

/// Makes a mirrored Marquee list agree with the remote list.
/// * on the remote and not here: added as `synced`
/// * `pending` here and now on the remote: confirmed, becomes `synced`
/// * `synced` here and gone from the remote: removed (it was removed over there)
/// * `local`/`pending`/`failed` here and not on the remote: left alone, they haven't been pushed yet
pub fn reconcile_list(conn: &rusqlite::Connection, list_id: i64, remote: &[i64]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT item_id, sync_state FROM list_items WHERE list_id = ?1")?;
    let current: Vec<(i64, String)> = stmt.query_map([list_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    for (item, state) in &current {
        let on_remote = remote.contains(item);
        match (state.as_str(), on_remote) {
            ("synced", false) => store::remove_from_list(conn, list_id, *item)?,
            (s, true) if s != "synced" => store::set_sync_state(conn, list_id, *item, "synced")?,
            _ => {}
        }
    }
    for item in remote {
        if !current.iter().any(|(i, _)| i == item) {
            store::add_to_list(conn, list_id, *item, "synced")?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------- writes

fn source_for_target(target: &str) -> Option<(&'static str, String)> {
    let (source, rest) = target.split_once(':')?;
    let id = match source {
        "trakt" => "trakt",
        "steam" => "steam",
        "letterboxd" => "letterboxd",
        "spotify" => "spotify",
        "lastfm" => "lastfm",
        _ => return None,
    };
    Some((id, rest.to_string()))
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Pushed {
    Synced,
    /// The service has no write API; the right page was opened for the user.
    Opened { hint: String },
    /// Offline: saved and will be sent when the connection is back.
    Queued,
    Failed { message: String },
}

impl Pushed {
    pub fn list_state(&self) -> &'static str {
        match self {
            Pushed::Synced => "synced",
            Pushed::Opened { .. } | Pushed::Queued => "pending",
            Pushed::Failed { .. } => "failed",
        }
    }
}

/// Sends one change to a service now, or queues it if the machine is offline.
pub async fn push(ctx: &Ctx, source: &str, op: &WriteOp) -> Pushed {
    let Some(src) = integrations::get(source) else { return Pushed::Failed { message: format!("Marquee doesn't know a service called {source}.") } };
    if !store::is_connected(&ctx.db.lock(), source) {
        return Pushed::Failed { message: format!("{} isn't connected. Connect it in Settings → Connections.", src.name()) };
    }
    match src.write(ctx, op).await {
        Ok(WriteOutcome::Synced) => Pushed::Synced,
        Ok(WriteOutcome::Opened { hint, .. }) => Pushed::Opened { hint },
        Err(e) if e.is_offline() => {
            let queued = serde_json::to_string(op).ok().and_then(|json| ctx.db.lock().execute("INSERT INTO pending_writes (source, op, created_at) VALUES (?1, ?2, ?3)", rusqlite::params![source, json, now()]).ok());
            if queued.is_some() { Pushed::Queued } else { Pushed::Failed { message: e.to_string() } }
        }
        Err(e) => {
            if matches!(e, Error::Reauth(_)) {
                let _ = store::set_source(&ctx.db.lock(), source, "error", None, Some(&e.to_string()));
            }
            Pushed::Failed { message: e.to_string() }
        }
    }
}

/// Retries queued writes in order. Stops at the first one that is still offline.
pub async fn replay_pending(ctx: &Ctx) -> Result<(usize, usize)> {
    let pending: Vec<(i64, String, String, i64)> = {
        let conn = ctx.db.lock();
        let mut stmt = conn.prepare("SELECT id, source, op, attempts FROM pending_writes ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let (mut sent, mut left) = (0, 0);
    for (n, (id, source, op_json, attempts)) in pending.iter().enumerate() {
        let Ok(op) = serde_json::from_str::<WriteOp>(op_json) else {
            ctx.db.lock().execute("DELETE FROM pending_writes WHERE id = ?1", [id])?;
            continue;
        };
        let Some(src) = integrations::get(source) else { continue };
        match src.write(ctx, &op).await {
            Ok(outcome) => {
                let conn = ctx.db.lock();
                conn.execute("DELETE FROM pending_writes WHERE id = ?1", [id])?;
                if matches!(outcome, WriteOutcome::Synced) {
                    if let WriteOp::AddToList { item_id, .. } = &op {
                        conn.execute("UPDATE list_items SET sync_state = 'synced' WHERE item_id = ?1 AND sync_state = 'pending'", [item_id])?;
                    }
                }
                sent += 1;
            }
            Err(e) if e.is_offline() => {
                left = pending.len() - n;
                break;
            }
            Err(e) => {
                let conn = ctx.db.lock();
                if *attempts >= 4 {
                    // Give up after five tries and show the item as failed rather than pending forever.
                    conn.execute("DELETE FROM pending_writes WHERE id = ?1", [id])?;
                    conn.execute("UPDATE list_items SET sync_state = 'failed' WHERE item_id = ?1 AND sync_state = 'pending'", [op.item_id()])?;
                } else {
                    conn.execute("UPDATE pending_writes SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1", rusqlite::params![id, e.to_string()])?;
                    left += 1;
                }
            }
        }
    }
    Ok((sent, left))
}

pub fn pending_count(ctx: &Ctx) -> i64 {
    ctx.db.lock().query_row("SELECT count(*) FROM pending_writes", [], |r| r.get(0)).unwrap_or(0)
}

/// Adds an item to a destination: a Marquee list ("list:<id>") or a service list
/// ("trakt:watchlist", "steam:wishlist", "letterboxd:watchlist", "spotify:library",
/// "spotify:playlist:<id>"). A service add is always mirrored in the matching Marquee list.
pub async fn add_to(ctx: &Ctx, item_id: i64, target: &str) -> Result<Value> {
    let item = store::get_item(&ctx.db.lock(), item_id)?;
    if let Some(list) = target.strip_prefix("list:") {
        let list_id: i64 = list.parse().map_err(|_| Error::msg("That list doesn't exist."))?;
        let mirror: Option<String> = ctx.db.lock().query_row("SELECT mirror FROM lists WHERE id = ?1", [list_id], |r| r.get(0)).map_err(|_| Error::msg("That list doesn't exist."))?;
        // A list that mirrors a service pushes the add there too.
        let pushed = match mirror.as_deref().and_then(source_for_target) {
            Some((source, rest)) => Some(push(ctx, source, &WriteOp::AddToList { item_id, target: rest }).await),
            None => None,
        };
        let state = pushed.as_ref().map(|p| p.list_state()).unwrap_or("local");
        store::add_to_list(&ctx.db.lock(), list_id, item_id, state)?;
        return Ok(json!({ "list_id": list_id, "sync_state": state, "pushed": pushed, "title": item.title }));
    }
    let (source, rest) = source_for_target(target).ok_or_else(|| Error::msg("Marquee doesn't know that destination."))?;
    let pushed = push(ctx, source, &WriteOp::AddToList { item_id, target: rest }).await;
    // Mirror it locally either way, so it's in Marquee even if the service step is manual.
    let conn = ctx.db.lock();
    let list_id = store::list_id(&conn, item.media.default_list())?;
    store::add_to_list(&conn, list_id, item_id, pushed.list_state())?;
    if matches!(pushed, Pushed::Synced | Pushed::Opened { .. }) {
        store::record_event(&conn, item_id, "marquee", EventKind::Wishlist, 1.0, None, None, &Value::Null)?;
    }
    Ok(json!({ "list_id": list_id, "sync_state": pushed.list_state(), "pushed": pushed, "title": item.title }))
}

pub async fn remove_from(ctx: &Ctx, item_id: i64, list_id: i64) -> Result<Value> {
    let (mirror, state): (Option<String>, Option<String>) = {
        let conn = ctx.db.lock();
        (
            conn.query_row("SELECT mirror FROM lists WHERE id = ?1", [list_id], |r| r.get(0)).unwrap_or(None),
            conn.query_row("SELECT sync_state FROM list_items WHERE list_id = ?1 AND item_id = ?2", [list_id, item_id], |r| r.get(0)).ok(),
        )
    };
    let pushed = match (mirror.as_deref().and_then(source_for_target), state.as_deref()) {
        // Only items that made it to the service need removing there.
        (Some((source, rest)), Some("synced")) => Some(push(ctx, source, &WriteOp::RemoveFromList { item_id, target: rest }).await),
        _ => None,
    };
    store::remove_from_list(&ctx.db.lock(), list_id, item_id)?;
    Ok(json!({ "pushed": pushed }))
}

/// Rates an item (0 clears the rating) and syncs the rating where a service supports it.
pub async fn rate(ctx: &Ctx, item_id: i64, stars: f64) -> Result<Value> {
    let stars = (stars * 2.0).round() / 2.0;
    let item = {
        let conn = ctx.db.lock();
        let item = store::get_item(&conn, item_id)?;
        if stars <= 0.0 {
            store::remove_event(&conn, item_id, "marquee", EventKind::Rating)?;
        } else {
            store::record_event(&conn, item_id, "marquee", EventKind::Rating, stars.clamp(0.5, 5.0), None, None, &Value::Null)?;
            // Rating something means you've seen, played or heard it.
            if !store::has_event(&conn, item_id, &["watched", "play"]) {
                store::record_event(&conn, item_id, "marquee", EventKind::Watched, 1.0, None, None, &Value::Null)?;
            }
        }
        item
    };
    let mut pushed = vec![];
    let connected = |s: &str| store::is_connected(&ctx.db.lock(), s);
    if matches!(item.media, Media::Movie | Media::Tv) && connected("trakt") {
        pushed.push(json!({ "source": "trakt", "result": push(ctx, "trakt", &WriteOp::Rate { item_id, stars }).await }));
    }
    if item.media == Media::Album && stars >= 4.0 && ctx.secrets.get("lastfm.session").is_some() {
        pushed.push(json!({ "source": "lastfm", "result": push(ctx, "lastfm", &WriteOp::Rate { item_id, stars }).await }));
    }
    Ok(json!({ "stars": stars, "pushed": pushed }))
}

/// Marks an item watched / played / listened (or clears that).
pub async fn mark_done(ctx: &Ctx, item_id: i64, done: bool) -> Result<Value> {
    let item = {
        let conn = ctx.db.lock();
        let item = store::get_item(&conn, item_id)?;
        if done {
            store::record_event(&conn, item_id, "marquee", EventKind::Watched, 1.0, None, None, &Value::Null)?;
        } else {
            store::remove_event(&conn, item_id, "marquee", EventKind::Watched)?;
        }
        item
    };
    let mut pushed = vec![];
    if done && matches!(item.media, Media::Movie | Media::Tv) && store::is_connected(&ctx.db.lock(), "trakt") {
        pushed.push(json!({ "source": "trakt", "result": push(ctx, "trakt", &WriteOp::MarkDone { item_id, at: now() }).await }));
    }
    Ok(json!({ "done": done, "pushed": pushed }))
}

// --------------------------------------------------------------- enrichment

/// Gives stub items (titles that arrived from a service with only a name and an id) the genres,
/// tags and creators the taste engine needs. Bounded per run; the rest are picked up next time.
pub async fn enrich(ctx: &Ctx, budget: usize) -> Result<usize> {
    let stubs: Vec<(i64, String)> = {
        let conn = ctx.db.lock();
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT i.id, i.media FROM items i
             WHERE i.detail_level = 0
               AND EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id)
               AND NOT EXISTS (SELECT 1 FROM item_features f WHERE f.item_id = i.id AND (f.feature LIKE 'g:%' OR f.feature LIKE 't:%'))
               AND COALESCE(json_extract(i.data, '$.enrich_tried'), '') < ?1
             ORDER BY (SELECT max(abs(e.value)) FROM user_events e WHERE e.item_id = i.id) DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![week_ago, budget as i64 * 4], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut done = 0;

    // Games: one IGDB call covers a hundred Steam ids.
    if catalog::igdb::configured(ctx) {
        let steam_ids: Vec<String> = {
            let conn = ctx.db.lock();
            stubs.iter().filter(|s| s.1 == "game").filter_map(|s| conn.query_row("SELECT ext_id FROM item_ids WHERE item_id = ?1 AND source = 'steam'", [s.0], |r| r.get(0)).ok()).collect()
        };
        if !steam_ids.is_empty() {
            match catalog::igdb::by_steam_ids(ctx, &steam_ids).await {
                Ok(drafts) => done += catalog::save_all(ctx, &drafts)?.len(),
                Err(e) if e.is_offline() => return Err(e),
                Err(e) => tracing::warn!("IGDB enrichment failed: {e}"),
            }
        }
    }

    let (mut screen, mut music, mut games) = (budget, budget / 2, 20usize);
    for (id, media) in &stubs {
        let allowance = match media.as_str() {
            "movie" | "tv" => &mut screen,
            "game" => &mut games,
            _ => &mut music,
        };
        if *allowance == 0 {
            continue;
        }
        let still_stub: bool = ctx.db.lock().query_row("SELECT NOT EXISTS (SELECT 1 FROM item_features f WHERE f.item_id = ?1 AND (f.feature LIKE 'g:%' OR f.feature LIKE 't:%'))", [id], |r| r.get(0)).unwrap_or(true);
        if !still_stub {
            continue;
        }
        *allowance -= 1;
        let result = match media.as_str() {
            "album" => light_album(ctx, *id).await,
            _ => catalog::ensure_detail(ctx, *id).await,
        };
        match result {
            Ok(()) => done += 1,
            Err(e) if e.is_offline() => return Err(e),
            Err(Error::NotConfigured(_)) => {}
            Err(e) => tracing::debug!("couldn't enrich item {id}: {e}"),
        }
        // Remember the attempt so an unmatchable title isn't retried on every sync.
        ctx.db.lock().execute("UPDATE items SET data = json_set(data, '$.enrich_tried', ?1) WHERE id = ?2", rusqlite::params![now(), id])?;
    }
    Ok(done)
}

/// Albums in the listening history only need their artist's tags to be scoreable; the full
/// MusicBrainz lookup (1 request per second) waits until the album's page is opened.
async fn light_album(ctx: &Ctx, item_id: i64) -> Result<()> {
    let item = store::get_item(&ctx.db.lock(), item_id)?;
    let Some(artist) = item.data["artist"].as_str().filter(|a| !a.is_empty()) else { return Ok(()) };
    let tags = catalog::lastfm::artist_tags(ctx, artist).await?;
    if tags.is_empty() {
        return Ok(());
    }
    let mut d = ItemDraft::new(Media::Album, item.title.clone());
    d.tags = tags;
    d.tag_source = TagSource::Lastfm;
    d.creators.push(Creator::new(artist, "artist"));
    store::upsert_item_at(&ctx.db.lock(), &d, Some(item_id))?;
    Ok(())
}

/// Keeps time-sensitive titles fresh: shows being watched (next episode), wishlisted games
/// (price), and franchises being followed (new entries).
async fn refresh_tracked(ctx: &Ctx) -> Result<()> {
    let (shows, games, franchises): (Vec<i64>, Vec<i64>, Vec<String>) = {
        let conn = ctx.db.lock();
        let now = now();
        let ids = |sql: &str| -> Result<Vec<i64>> {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map([&now], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        };
        let shows = ids(
            "SELECT i.id FROM items i WHERE i.media = 'tv' AND (i.expires_at IS NULL OR i.expires_at < ?1 OR i.detail_level = 0)
               AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind != 'dismiss')
                 OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id)
                 OR EXISTS (SELECT 1 FROM follows f WHERE f.item_id = i.id))
             ORDER BY i.popularity DESC LIMIT 80",
        )?;
        let games = ids(
            "SELECT i.id FROM items i WHERE i.media = 'game' AND (i.expires_at IS NULL OR i.expires_at < ?1 OR i.detail_level = 0)
               AND EXISTS (SELECT 1 FROM item_ids x WHERE x.item_id = i.id AND x.source = 'steam')
               AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'wishlist')
                 OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id)
                 OR EXISTS (SELECT 1 FROM follows f WHERE f.item_id = i.id))
             LIMIT 40",
        )?;
        let mut stmt = conn.prepare("SELECT ref FROM follows WHERE kind = 'franchise'")?;
        let franchises = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        (shows, games, franchises)
    };
    for id in shows.into_iter().chain(games) {
        match catalog::ensure_detail(ctx, id).await {
            Err(e) if e.is_offline() => return Err(e),
            _ => {}
        }
    }
    for collection in franchises {
        if let Ok(parts) = catalog::tmdb::collection(ctx, &collection).await {
            let ids = catalog::save_all(ctx, &parts)?;
            let conn = ctx.db.lock();
            for id in ids {
                conn.execute("UPDATE items SET data = json_set(data, '$.followed_franchise', ?1) WHERE id = ?2", rusqlite::params![collection, id])?;
            }
        }
    }
    Ok(())
}

/// Rebuilds the taste profile and every score. Fast enough to run after any rating.
pub fn rescore(ctx: &Ctx) -> Result<usize> {
    taste::profile::rebuild(&ctx.db)?;
    let n = taste::score::score_all(&ctx.db)?;
    ctx.emit("taste:updated", json!({ "scored": n }));
    Ok(n)
}

async fn sync_source(ctx: &Ctx, src: &dyn integrations::Source, report: &mut SyncReport) {
    let id = src.id();
    let _ = store::set_source_status(&ctx.db.lock(), id, "syncing");
    ctx.emit("sources:changed", json!({ "id": id }));
    let outcome = src.sync(ctx).await.and_then(|synced| {
        let changed = apply(ctx, id, &synced)?;
        Ok((synced, changed))
    });
    let conn = ctx.db.lock();
    match outcome {
        Ok((synced, changed)) => {
            report.events += changed;
            let warning = synced.warnings.first().cloned();
            // A warning (a private Steam profile, say) is shown on the connection without marking it broken.
            let _ = store::set_source(&conn, id, "connected", synced.account.as_deref(), None);
            if let Some(w) = &warning {
                let _ = conn.execute("UPDATE sources SET error = ?2 WHERE id = ?1", rusqlite::params![id, w]);
            }
            report.warnings.extend(synced.warnings.iter().map(|w| format!("{}: {w}", src.name())));
            report.sources.push(json!({ "id": id, "ok": true, "events": synced.events.len(), "changed": changed, "warning": warning }));
        }
        Err(e) => {
            if e.is_offline() {
                report.offline = true;
                // Offline isn't the connection's fault: keep it "connected" with its last sync time.
                let _ = store::set_source_status(&conn, id, "connected");
            } else {
                let _ = store::set_source(&conn, id, "error", None, Some(&e.to_string()));
            }
            report.sources.push(json!({ "id": id, "ok": false, "error": e.to_string(), "kind": e.kind() }));
        }
    }
    drop(conn);
    ctx.emit("sources:changed", json!({ "id": id }));
}

/// A full sync, or one source when `only` is given.
pub async fn run(ctx: &Ctx, only: Option<&str>) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    progress(ctx, "Starting", 0.0);
    let _ = replay_pending(ctx).await;

    let connected: Vec<Box<dyn integrations::Source>> = {
        let conn = ctx.db.lock();
        integrations::all().into_iter().filter(|s| only.map(|o| o == s.id()).unwrap_or(true)).filter(|s| store::is_connected(&conn, s.id())).collect()
    };
    let n = connected.len().max(1) as f64;
    for (i, src) in connected.iter().enumerate() {
        progress(ctx, &format!("Syncing {}", src.name()), 0.02 + 0.28 * (i as f64 / n));
        sync_source(ctx, src.as_ref(), &mut report).await;
    }

    if only.is_none() && !report.offline {
        let ctx_ref = ctx;
        let cat = catalog::refresh(ctx, &|label, pct| progress(ctx_ref, label, 0.3 + 0.4 * pct)).await;
        report.offline |= cat.skipped.iter().any(|s| s.contains("offline"));
        report.catalog = Some(cat);
    }

    if !report.offline {
        progress(ctx, "Learning your taste", 0.72);
        match enrich(ctx, 250).await {
            Err(e) if e.is_offline() => report.offline = true,
            Err(e) => report.warnings.push(e.to_string()),
            Ok(_) => {}
        }
        if only.is_none() && !report.offline {
            if let Err(e) = refresh_tracked(ctx).await {
                report.offline |= e.is_offline();
            }
        }
    }

    progress(ctx, "Scoring picks for you", 0.88);
    report.scored = rescore(ctx)?;
    if only.is_none() && !report.offline {
        // Give the best upcoming picks full details (trailers, providers, better reasons), then rescore.
        let top: Vec<i64> = {
            let conn = ctx.db.lock();
            let mut stmt = conn.prepare(
                "SELECT i.id FROM items i JOIN scores s ON s.item_id = i.id
                 WHERE i.detail_level = 0 AND i.media IN ('movie', 'tv', 'game') AND i.release_date >= ?1
                 ORDER BY s.score DESC LIMIT 36",
            )?;
            let rows = stmt.query_map([days_from_today(-30)], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let mut fetched = 0;
        for id in top {
            match catalog::ensure_detail(ctx, id).await {
                Ok(()) => fetched += 1,
                Err(e) if e.is_offline() => break,
                Err(_) => {}
            }
        }
        if fetched > 0 {
            report.scored = rescore(ctx)?;
        }
        if let Ok(n) = crate::ai::embed_missing(ctx, 300).await {
            if n > 0 {
                report.scored = rescore(ctx)?;
            }
        }
    }

    progress(ctx, "Checking for alerts", 0.96);
    let _ = notifications::generate(ctx);
    if only.is_none() {
        store::set_setting(&ctx.db, "last_sync", &json!({ "at": now(), "offline": report.offline }))?;
        let _ = ctx.http.prune_cache();
    }
    progress(ctx, if report.offline { "Offline: showing what's saved" } else { "Up to date" }, 1.0);
    ctx.emit("sync:done", json!({ "offline": report.offline, "events": report.events, "warnings": report.warnings }));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;
    use crate::integrations::Source;

    fn game(title: &str, app: i64) -> ItemDraft {
        ItemDraft::new(Media::Game, title).id("steam", app)
    }

    #[test]
    fn applying_a_sync_stores_events_and_drops_ones_removed_remotely() {
        let t = ctx();
        let first = Synced { events: vec![RawEvent::new(game("Hades", 1), EventKind::Wishlist, 1.0), RawEvent::new(game("Celeste", 2), EventKind::Owned, 1.0)], ..Default::default() };
        assert_eq!(apply(&t.ctx, "steam", &first).unwrap(), 2);
        // Hades came off the wishlist on Steam.
        let second = Synced { events: vec![RawEvent::new(game("Celeste", 2), EventKind::Owned, 1.0)], ..Default::default() };
        apply(&t.ctx, "steam", &second).unwrap();
        let kinds: Vec<String> = {
            let conn = t.ctx.db.lock();
            let mut s = conn.prepare("SELECT kind FROM user_events WHERE source = 'steam'").unwrap();
            let rows = s.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            rows
        };
        assert_eq!(kinds, vec!["owned"]);
        // A partial source (the Letterboxd feed only has recent entries) never deletes history.
        let lb = Synced { events: vec![RawEvent::new(ItemDraft::new(Media::Movie, "A").id("tmdb", 1), EventKind::Watched, 1.0)], ..Default::default() };
        apply(&t.ctx, "letterboxd", &lb).unwrap();
        let lb2 = Synced { events: vec![RawEvent::new(ItemDraft::new(Media::Movie, "B").id("tmdb", 2), EventKind::Watched, 1.0)], ..Default::default() };
        apply(&t.ctx, "letterboxd", &lb2).unwrap();
        let n: i64 = t.ctx.db.lock().query_row("SELECT count(*) FROM user_events WHERE source = 'letterboxd'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn a_mirrored_list_follows_the_remote_and_confirms_pending_adds() {
        let t = ctx();
        let conn = t.ctx.db.lock();
        let list = store::list_id(&conn, "play-later").unwrap();
        let a = store::upsert_item(&conn, &game("A", 1)).unwrap();
        let b = store::upsert_item(&conn, &game("B", 2)).unwrap();
        let c = store::upsert_item(&conn, &game("C", 3)).unwrap();
        let d = store::upsert_item(&conn, &game("D", 4)).unwrap();
        store::add_to_list(&conn, list, a, "synced").unwrap(); // removed remotely -> goes
        store::add_to_list(&conn, list, b, "pending").unwrap(); // now on remote -> confirmed
        store::add_to_list(&conn, list, c, "pending").unwrap(); // not yet on remote -> stays pending
        reconcile_list(&conn, list, &[b, d]).unwrap();
        let mut state: Vec<(i64, String)> = store::list_entries(&conn, list).unwrap().into_iter().map(|e| (e.card.id, e.sync_state)).collect();
        state.sort();
        assert_eq!(state, vec![(b, "synced".into()), (c, "pending".into()), (d, "synced".into())]);
    }

    #[tokio::test]
    async fn adding_to_steam_mirrors_locally_as_pending_then_sync_confirms_it() {
        let t = ctx();
        crate::integrations::steam::connect_with_id(&t.ctx, "76561198000000001").await.ok();
        t.net.on("GetPlayerSummaries", 200, r#"{"response":{"players":[{"personaname":"larosa"}]}}"#);
        crate::integrations::steam::connect_with_id(&t.ctx, "76561198000000001").await.unwrap();
        let id = store::upsert_item(&t.ctx.db.lock(), &game("Hades", 1145360)).unwrap();
        let out = add_to(&t.ctx, id, "steam:wishlist").await.unwrap();
        assert_eq!(out["sync_state"], "pending");
        assert_eq!(out["pushed"]["state"], "opened");
        assert_eq!(t.host.opened.lock().unwrap().last().unwrap(), "steam://store/1145360");
        // The next sync sees it on the real wishlist.
        t.net.on("IPlayerService/GetOwnedGames", 200, r#"{"response":{"games":[]}}"#);
        t.net.on("IWishlistService/GetWishlist", 200, r#"{"response":{"items":[{"appid":1145360,"date_added":1700000000}]}}"#);
        let synced = crate::integrations::steam::Steam.sync(&t.ctx).await.unwrap();
        apply(&t.ctx, "steam", &synced).unwrap();
        let conn = t.ctx.db.lock();
        let list = store::list_id(&conn, "play-later").unwrap();
        assert_eq!(store::list_entries(&conn, list).unwrap()[0].sync_state, "synced");
    }

    #[tokio::test]
    async fn offline_writes_are_queued_and_replayed_on_reconnect() {
        let t = ctx();
        t.ctx.secrets.set("trakt.access", "acc").unwrap();
        {
            let conn = t.ctx.db.lock();
            store::set_source_config(&conn, "trakt", json!({ "expires_at": chrono::Utc::now().timestamp() + 90_000 })).unwrap();
            store::set_source(&conn, "trakt", "connected", Some("larosa"), None).unwrap();
            conn.execute("UPDATE lists SET mirror = 'trakt:watchlist' WHERE slug = 'watchlist'", []).unwrap();
        }
        let id = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865)).unwrap();
        let list = store::list_id(&t.ctx.db.lock(), "watchlist").unwrap();

        t.net.set_offline(true);
        let out = add_to(&t.ctx, id, &format!("list:{list}")).await.unwrap();
        assert_eq!(out["pushed"]["state"], "queued");
        assert_eq!(out["sync_state"], "pending");
        assert_eq!(pending_count(&t.ctx), 1);
        let rated = rate(&t.ctx, id, 4.5).await.unwrap();
        assert_eq!(rated["pushed"][0]["result"]["state"], "queued");
        assert_eq!(pending_count(&t.ctx), 2);
        // Still offline: nothing is lost.
        assert_eq!(replay_pending(&t.ctx).await.unwrap(), (0, 2));

        t.net.set_offline(false);
        t.net.on("POST https://api.trakt.tv/sync/", 200, r#"{"not_found":{"movies":[],"shows":[]}}"#);
        assert_eq!(replay_pending(&t.ctx).await.unwrap(), (2, 0));
        assert_eq!(pending_count(&t.ctx), 0);
        assert!(t.net.last("/sync/watchlist").is_some() && t.net.last("/sync/ratings").is_some());
        assert_eq!(store::list_entries(&t.ctx.db.lock(), list).unwrap()[0].sync_state, "synced");
    }

    #[tokio::test]
    async fn rating_records_locally_rounds_to_half_stars_and_implies_seen() {
        let t = ctx();
        let id = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Game, "Hades").id("steam", 1)).unwrap();
        let out = rate(&t.ctx, id, 4.3).await.unwrap();
        assert_eq!(out["stars"], 4.5);
        let conn = t.ctx.db.lock();
        assert_eq!(store::user_rating(&conn, id), Some(4.5));
        assert!(store::has_event(&conn, id, &["watched"]));
        drop(conn);
        rate(&t.ctx, id, 0.0).await.unwrap();
        assert_eq!(store::user_rating(&t.ctx.db.lock(), id), None);
    }

    #[tokio::test]
    async fn a_failing_write_gives_up_after_five_tries_and_marks_the_item_failed() {
        let t = ctx();
        t.ctx.secrets.set("trakt.access", "acc").unwrap();
        let (id, list) = {
            let conn = t.ctx.db.lock();
            store::set_source_config(&conn, "trakt", json!({ "expires_at": chrono::Utc::now().timestamp() + 90_000 })).unwrap();
            store::set_source(&conn, "trakt", "connected", None, None).unwrap();
            let id = store::upsert_item(&conn, &ItemDraft::new(Media::Movie, "Ghost Film").id("tmdb", 1)).unwrap();
            let list = store::list_id(&conn, "watchlist").unwrap();
            store::add_to_list(&conn, list, id, "pending").unwrap();
            conn.execute("INSERT INTO pending_writes (source, op, created_at) VALUES ('trakt', ?1, 'now')", [serde_json::to_string(&WriteOp::AddToList { item_id: id, target: "watchlist".into() }).unwrap()]).unwrap();
            (id, list)
        };
        t.net.on("POST https://api.trakt.tv/sync/", 200, r#"{"not_found":{"movies":[{"ids":{"tmdb":1}}],"shows":[]}}"#);
        for _ in 0..5 {
            replay_pending(&t.ctx).await.unwrap();
        }
        assert_eq!(pending_count(&t.ctx), 0);
        let conn = t.ctx.db.lock();
        assert_eq!(store::list_entries(&conn, list).unwrap().iter().find(|e| e.card.id == id).unwrap().sync_state, "failed");
    }

    #[tokio::test]
    async fn a_full_sync_offline_keeps_connections_and_still_scores_from_cache() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            store::set_source_config(&conn, "lastfm", json!({ "username": "anthony" })).unwrap();
            store::set_source(&conn, "lastfm", "connected", Some("anthony"), None).unwrap();
            let id = store::upsert_item(&conn, &ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 1)).unwrap();
            store::record_event(&conn, id, "marquee", EventKind::Rating, 5.0, None, None, &Value::Null).unwrap();
        }
        t.net.set_offline(true);
        let report = run(&t.ctx, None).await.unwrap();
        assert!(report.offline);
        assert_eq!(report.scored, 1);
        let (status, _, _, error) = store::source_row(&t.ctx.db.lock(), "lastfm").unwrap();
        assert_eq!(status, "connected");
        assert_eq!(error, None);
        let events = t.host.events.lock().unwrap();
        assert!(events.iter().any(|(e, p)| e == "sync:done" && p["offline"] == true));
        assert!(events.iter().any(|(e, _)| e == "sync:progress"));
    }

    #[tokio::test]
    async fn a_source_that_needs_sign_in_shows_an_error_without_stopping_the_others() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            store::set_source(&conn, "trakt", "connected", Some("larosa"), None).unwrap(); // no token stored
            store::set_source_config(&conn, "letterboxd", json!({ "username": "larosa" })).unwrap();
            store::set_source(&conn, "letterboxd", "connected", Some("larosa"), None).unwrap();
        }
        t.net.on("letterboxd.com/larosa/rss/", 200, include_str!("../../tests/fixtures/letterboxd_rss.xml"));
        let mut report = SyncReport::default();
        for src in [Box::new(crate::integrations::trakt::Trakt) as Box<dyn Source>, Box::new(crate::integrations::letterboxd::Letterboxd)] {
            sync_source(&t.ctx, src.as_ref(), &mut report).await;
        }
        let conn = t.ctx.db.lock();
        let (status, _, _, error) = store::source_row(&conn, "trakt").unwrap();
        assert_eq!(status, "error");
        assert!(error.unwrap().contains("sign in again"));
        assert_eq!(store::source_row(&conn, "letterboxd").unwrap().0, "connected");
        assert!(report.events >= 4);
    }
}
