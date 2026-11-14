//! Trakt: watched history, ratings, watchlist and collection for movies and shows, and the
//! main write path for film and TV (watchlist, mark watched, rate).
//! https://trakt.docs.apiary.io/
//!
//! 2026 API notes this client follows:
//! * every list endpoint is paginated; a request without `page`/`limit` returns only the first
//!   100 items, so every read sends both and follows `X-Pagination-Page-Count`
//! * watched endpoints take `extended=min` for the compact shape
//! * a descriptive `User-Agent` is required

use super::{item_for_write, Source, Synced, WriteOp, WriteOutcome};
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use crate::util::*;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};

const API: &str = "https://api.trakt.tv";
const PAGE: &str = "250";

pub struct Trakt;

fn call<'a>(ctx: &'a Ctx, method: &str, path: &str, client_id: &str) -> crate::net::Call<'a> {
    ctx.http.request("Trakt", method, format!("{API}{path}")).header("Content-Type", "application/json").header("trakt-api-version", "2").header("trakt-api-key", client_id).header("User-Agent", ctx.user_agent())
}

#[derive(Serialize, Debug, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_url: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// Step 1 of the device flow: get a code for the user to type at trakt.tv/activate.
pub async fn device_start(ctx: &Ctx) -> Result<DeviceCode> {
    let id = ctx.require_key("trakt_client_id")?;
    ctx.require_key("trakt_client_secret")?;
    let v: Value = call(ctx, "POST", "/oauth/device/code", &id).json_body(&json!({ "client_id": id })).json().await.map_err(|e| match e {
        Error::Api { status: 403 | 404, .. } | Error::Reauth(_) => {
            Error::NotConfigured("Trakt didn't recognise the client ID. Trakt now requires a VIP account to create API apps; check the app still exists at trakt.tv/oauth/applications.".into())
        }
        other => other,
    })?;
    let code = DeviceCode {
        user_code: s(&v, "user_code").unwrap_or_default(),
        verification_url: s(&v, "verification_url").unwrap_or_else(|| "https://trakt.tv/activate".into()),
        device_code: s(&v, "device_code").unwrap_or_default(),
        interval: i(&v, "interval").unwrap_or(5).max(1) as u64,
        expires_in: i(&v, "expires_in").unwrap_or(600) as u64,
    };
    ctx.host.open_url(&code.verification_url);
    Ok(code)
}

/// Step 2: poll until the user approves. Returns `Ok(None)` while still waiting.
pub async fn device_poll(ctx: &Ctx, device_code: &str) -> Result<Option<String>> {
    let id = ctx.require_key("trakt_client_id")?;
    let secret = ctx.require_key("trakt_client_secret")?;
    let resp = call(ctx, "POST", "/oauth/device/token", &id).json_body(&json!({ "code": device_code, "client_id": id, "client_secret": secret })).send_raw().await?;
    match resp.status {
        200 => {
            save_tokens(ctx, &resp.json()?)?;
            let name = username(ctx).await.unwrap_or_else(|_| "Trakt".into());
            let conn = ctx.db.lock();
            store::set_source(&conn, "trakt", "connected", Some(&name), None)?;
            conn.execute("UPDATE lists SET mirror = 'trakt:watchlist' WHERE slug = 'watchlist' AND mirror IS NULL", [])?;
            Ok(Some(name))
        }
        400 => Ok(None), // still waiting for the user
        429 => Ok(None), // polling too fast; the caller's interval handles it
        404 => Err(Error::msg("Trakt doesn't recognise that code. Start connecting again.")),
        409 => Err(Error::msg("That code was already used. Start connecting again.")),
        410 => Err(Error::msg("The code expired before it was entered. Start connecting again.")),
        418 => Err(Error::msg("You declined the request on Trakt, so nothing was connected.")),
        status => Err(Error::Api { service: "Trakt".into(), status, detail: resp.text() }),
    }
}

fn save_tokens(ctx: &Ctx, v: &Value) -> Result<()> {
    let access = s(v, "access_token").ok_or_else(|| Error::msg("Trakt didn't return an access token"))?;
    ctx.secrets.set("trakt.access", &access)?;
    if let Some(r) = s(v, "refresh_token") {
        ctx.secrets.set("trakt.refresh", &r)?;
    }
    let expires = i(v, "created_at").unwrap_or_else(|| chrono::Utc::now().timestamp()) + i(v, "expires_in").unwrap_or(7 * 86_400);
    store::set_source_config(&ctx.db.lock(), "trakt", json!({ "expires_at": expires }))
}

async fn token(ctx: &Ctx) -> Result<String> {
    let access = ctx.secrets.get("trakt.access").ok_or_else(|| Error::Reauth("Trakt".into()))?;
    let expires = i(&super::cfg(ctx, "trakt"), "expires_at").unwrap_or(i64::MAX);
    if expires - chrono::Utc::now().timestamp() > 3600 {
        return Ok(access);
    }
    let refresh = ctx.secrets.get("trakt.refresh").ok_or_else(|| Error::Reauth("Trakt".into()))?;
    let id = ctx.require_key("trakt_client_id")?;
    let secret = ctx.require_key("trakt_client_secret")?;
    let v: Value =
        call(ctx, "POST", "/oauth/token", &id).json_body(&json!({ "refresh_token": refresh, "client_id": id, "client_secret": secret, "redirect_uri": "urn:ietf:wg:oauth:2.0:oob", "grant_type": "refresh_token" })).json().await.map_err(
            |e| match e {
                Error::Api { status: 400 | 401 | 403, .. } => Error::Reauth("Trakt".into()),
                other => other,
            },
        )?;
    save_tokens(ctx, &v)?;
    s(&v, "access_token").ok_or_else(|| Error::Reauth("Trakt".into()))
}

async fn username(ctx: &Ctx) -> Result<String> {
    let id = ctx.require_key("trakt_client_id")?;
    let t = token(ctx).await?;
    let v: Value = call(ctx, "GET", "/users/settings", &id).bearer(&t).json().await?;
    Ok(s(&v["user"], "username").unwrap_or_else(|| "Trakt".into()))
}

/// Reads every page of a list endpoint.
async fn all_pages(ctx: &Ctx, path: &str, extra: &[(&str, &str)]) -> Result<Vec<Value>> {
    let id = ctx.require_key("trakt_client_id")?;
    let t = token(ctx).await?;
    let mut out = vec![];
    let mut page = 1;
    loop {
        let p = page.to_string();
        let resp = call(ctx, "GET", path, &id).bearer(&t).query(&[("page", &p), ("limit", PAGE)]).query(extra).send().await?;
        let pages = resp.header("x-pagination-page-count").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1);
        let rows: Vec<Value> = resp.json()?;
        let got = rows.len();
        out.extend(rows);
        if page >= pages || got == 0 || page >= 200 {
            return Ok(out);
        }
        page += 1;
    }
}

/// A draft from Trakt's `{ "movie": {...} }` / `{ "show": {...} }` wrapper.
fn draft(row: &Value) -> Option<ItemDraft> {
    let (media, obj) = if row["movie"].is_object() {
        (Media::Movie, &row["movie"])
    } else if row["show"].is_object() {
        (Media::Tv, &row["show"])
    } else {
        return None;
    };
    let ids = &obj["ids"];
    let mut d = ItemDraft::new(media, s(obj, "title")?);
    // TMDB first: it's the id the rest of the catalog is keyed on.
    if let Some(tmdb) = s(ids, "tmdb") {
        d.ids.push(("tmdb".into(), tmdb));
    }
    if let Some(imdb) = s(ids, "imdb") {
        d.ids.push(("imdb".into(), imdb));
    }
    if let Some(trakt) = s(ids, "trakt") {
        d.ids.push(("trakt".into(), trakt));
    }
    if let Some(tvdb) = s(ids, "tvdb") {
        d.ids.push(("tvdb".into(), tvdb));
    }
    if d.ids.is_empty() {
        return None;
    }
    d.year = i(obj, "year").map(|y| y as i32);
    Some(d)
}

fn ids_body(item: &Item, extra: Value) -> Result<Value> {
    let mut ids = serde_json::Map::new();
    for (ours, theirs) in [("trakt", "trakt"), ("tmdb", "tmdb"), ("imdb", "imdb")] {
        if let Some(v) = item.ext(ours) {
            ids.insert(theirs.into(), if ours == "imdb" { json!(v) } else { v.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(v)) });
        }
    }
    if ids.is_empty() {
        return Err(Error::msg(format!("Trakt can't match {} (it has no TMDB or IMDb id).", item.title)));
    }
    let mut entry = json!({ "ids": ids });
    if let (Some(e), Some(x)) = (entry.as_object_mut(), extra.as_object()) {
        e.extend(x.clone());
    }
    let key = if item.media == Media::Tv { "shows" } else { "movies" };
    Ok(json!({ key: [entry] }))
}

#[async_trait]
impl Source for Trakt {
    fn id(&self) -> &'static str {
        "trakt"
    }
    fn name(&self) -> &'static str {
        "Trakt"
    }
    fn reads(&self) -> Vec<&'static str> {
        vec!["Watched history", "Ratings", "Watchlist", "Collection"]
    }
    fn writes(&self) -> Vec<&'static str> {
        vec!["Watchlist", "Mark watched", "Ratings"]
    }
    fn note(&self) -> &'static str {
        "The main way Marquee writes for movies and TV. Sign in with a code at trakt.tv/activate."
    }
    fn configured(&self, ctx: &Ctx) -> bool {
        ctx.has_key("trakt_client_id") && ctx.has_key("trakt_client_secret")
    }

    async fn sync(&self, ctx: &Ctx) -> Result<Synced> {
        let mut out = Synced::default();
        for kind in ["movies", "shows"] {
            for row in all_pages(ctx, &format!("/sync/watched/{kind}"), &[("extended", "min")]).await? {
                let Some(d) = draft(&row) else { continue };
                let plays = f(&row, "plays").unwrap_or(1.0).max(1.0);
                let mut ev = RawEvent::new(d, EventKind::Watched, plays).at(s(&row, "last_watched_at"));
                ev.meta = json!({ "plays": plays });
                out.events.push(ev);
            }
            for row in all_pages(ctx, &format!("/sync/ratings/{kind}"), &[]).await? {
                let Some(d) = draft(&row) else { continue };
                // Trakt rates out of 10; Marquee uses five stars with halves.
                let Some(stars) = f(&row, "rating").map(|r| r / 2.0) else { continue };
                out.events.push(RawEvent::new(d, EventKind::Rating, stars).at(s(&row, "rated_at")));
            }
            for row in all_pages(ctx, &format!("/sync/collection/{kind}"), &[]).await? {
                let Some(d) = draft(&row) else { continue };
                out.events.push(RawEvent::new(d, EventKind::Owned, 1.0).at(s(&row, "collected_at").or_else(|| s(&row, "last_collected_at"))));
            }
        }
        let mut watchlist = vec![];
        for kind in ["movies", "shows"] {
            for row in all_pages(ctx, &format!("/sync/watchlist/{kind}"), &[]).await? {
                let Some(d) = draft(&row) else { continue };
                out.events.push(RawEvent::new(d.clone(), EventKind::Wishlist, 1.0).at(s(&row, "listed_at")));
                watchlist.push(d);
            }
        }
        out.lists.push(("trakt:watchlist".into(), watchlist));
        Ok(out)
    }

    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome> {
        let item = item_for_write(ctx, op.item_id())?;
        if !matches!(item.media, Media::Movie | Media::Tv) {
            return Err(Error::msg("Trakt only tracks movies and shows."));
        }
        let id = ctx.require_key("trakt_client_id")?;
        let t = token(ctx).await?;
        let (path, body) = match op {
            WriteOp::AddToList { .. } => ("/sync/watchlist", ids_body(&item, json!({}))?),
            WriteOp::RemoveFromList { .. } => ("/sync/watchlist/remove", ids_body(&item, json!({}))?),
            WriteOp::MarkDone { at, .. } => ("/sync/history", ids_body(&item, json!({ "watched_at": at }))?),
            WriteOp::Rate { stars, .. } if *stars <= 0.0 => ("/sync/ratings/remove", ids_body(&item, json!({}))?),
            WriteOp::Rate { stars, .. } => ("/sync/ratings", ids_body(&item, json!({ "rating": (stars * 2.0).round().clamp(1.0, 10.0) as i64 }))?),
            _ => return Err(Error::msg("Trakt doesn't support that.")),
        };
        let v: Value = call(ctx, "POST", path, &id).bearer(&t).json_body(&body).json().await?;
        let missing = v["not_found"]["movies"].as_array().map(|a| a.len()).unwrap_or(0) + v["not_found"]["shows"].as_array().map(|a| a.len()).unwrap_or(0);
        if missing > 0 {
            return Err(Error::msg(format!("Trakt couldn't find {} in its catalog.", item.title)));
        }
        Ok(WriteOutcome::Synced)
    }

    fn forget(&self, ctx: &Ctx) {
        let _ = ctx.secrets.delete("trakt.access");
        let _ = ctx.secrets.delete("trakt.refresh");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::{ctx, TestCtx};

    fn signed_in() -> TestCtx {
        let t = ctx();
        t.ctx.secrets.set("trakt.access", "acc").unwrap();
        t.ctx.secrets.set("trakt.refresh", "ref").unwrap();
        store::set_source_config(&t.ctx.db.lock(), "trakt", json!({ "expires_at": chrono::Utc::now().timestamp() + 80_000 })).unwrap();
        for path in ["watched", "ratings", "collection", "watchlist"] {
            t.net.on(&format!("/sync/{path}/"), 200, "[]");
        }
        t
    }

    #[tokio::test]
    async fn device_flow_waits_then_connects() {
        let t = ctx();
        t.net.on("/oauth/device/code", 200, r#"{"device_code":"dev","user_code":"5055CC52","verification_url":"https://trakt.tv/activate","expires_in":600,"interval":5}"#);
        let code = device_start(&t.ctx).await.unwrap();
        assert_eq!(code.user_code, "5055CC52");
        assert_eq!(t.host.opened.lock().unwrap()[0], "https://trakt.tv/activate");

        t.net.on("/oauth/device/token", 400, "");
        assert_eq!(device_poll(&t.ctx, "dev").await.unwrap(), None);

        t.net.on("/oauth/device/token", 200, r#"{"access_token":"acc","refresh_token":"ref","expires_in":7776000,"created_at":1700000000,"token_type":"bearer"}"#);
        t.net.on("/users/settings", 200, r#"{"user":{"username":"larosa"}}"#);
        store::set_source_config(&t.ctx.db.lock(), "trakt", json!({})).unwrap();
        // created_at in the fixture is in the past, so pretend it is fresh for the username call.
        let name = {
            t.net.on("/oauth/token", 200, r#"{"access_token":"acc2","refresh_token":"ref2","expires_in":7776000}"#);
            device_poll(&t.ctx, "dev").await.unwrap()
        };
        assert_eq!(name.as_deref(), Some("larosa"));
        assert!(t.ctx.secrets.get("trakt.access").is_some());
        let body = String::from_utf8(t.net.last("/oauth/device/token").unwrap().body.unwrap()).unwrap();
        assert!(body.contains("test-trakt_client_secret"));
    }

    #[tokio::test]
    async fn denied_and_expired_codes_explain_themselves() {
        let t = ctx();
        t.net.on("/oauth/device/token", 418, "");
        assert!(device_poll(&t.ctx, "dev").await.unwrap_err().to_string().contains("declined"));
        t.net.on("/oauth/device/token", 410, "");
        assert!(device_poll(&t.ctx, "dev").await.unwrap_err().to_string().contains("expired"));
    }

    #[tokio::test]
    async fn sync_follows_pagination_and_converts_ratings() {
        let t = signed_in();
        t.net.on_with_headers("/sync/watched/movies?page=1", 200, &[("X-Pagination-Page-Count", "2")], include_str!("../../tests/fixtures/trakt_watched_movies.json"));
        t.net.on_with_headers(
            "/sync/watched/movies?page=2",
            200,
            &[("X-Pagination-Page-Count", "2")],
            r#"[{"plays":1,"last_watched_at":"2025-02-01T00:00:00.000Z","movie":{"title":"Ex Machina","year":2015,"ids":{"trakt":3,"imdb":"tt0470752","tmdb":264660}}}]"#,
        );
        t.net.on("/sync/ratings/shows", 200, r#"[{"rated_at":"2025-03-01T00:00:00.000Z","rating":9,"type":"show","show":{"title":"Severance","year":2022,"ids":{"trakt":9,"tmdb":95396}}}]"#);
        t.net.on("/sync/watchlist/movies", 200, r#"[{"rank":1,"listed_at":"2026-01-01T00:00:00.000Z","type":"movie","movie":{"title":"Dune: Part Three","year":2026,"ids":{"trakt":77,"tmdb":1170608}}}]"#);
        let synced = Trakt.sync(&t.ctx).await.unwrap();
        let watched: Vec<_> = synced.events.iter().filter(|e| e.kind == EventKind::Watched).collect();
        assert_eq!(watched.len(), 3, "both pages were read");
        assert_eq!(watched[0].draft.ids[0], ("tmdb".to_string(), "329865".to_string()));
        assert_eq!(watched[0].value, 2.0);
        let rating = synced.events.iter().find(|e| e.kind == EventKind::Rating).unwrap();
        assert_eq!(rating.value, 4.5);
        assert_eq!(rating.draft.media(), Media::Tv);
        assert_eq!(synced.lists[0].1.len(), 1);
        let first = t.net.last("/sync/watched/movies?page=1").unwrap();
        assert!(first.url.contains("limit=250") && first.url.contains("extended=min"));
        assert!(first.headers.iter().any(|(k, v)| k == "trakt-api-version" && v == "2"));
        assert!(first.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer acc"));
    }

    #[tokio::test]
    async fn writes_send_ids_and_the_ten_point_rating() {
        let t = signed_in();
        let id = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865).id("imdb", "tt2543164")).unwrap();
        t.net.on("POST https://api.trakt.tv/sync/", 200, r#"{"added":{"movies":1},"not_found":{"movies":[],"shows":[]}}"#);
        assert_eq!(Trakt.write(&t.ctx, &WriteOp::AddToList { item_id: id, target: "watchlist".into() }).await.unwrap(), WriteOutcome::Synced);
        assert!(t.net.last("POST https://api.trakt.tv/sync/watchlist").is_some());
        Trakt.write(&t.ctx, &WriteOp::Rate { item_id: id, stars: 4.5 }).await.unwrap();
        let body: Value = serde_json::from_slice(&t.net.last("/sync/ratings").unwrap().body.unwrap()).unwrap();
        assert_eq!(body["movies"][0]["rating"], 9);
        assert_eq!(body["movies"][0]["ids"]["tmdb"], 329865);
        assert_eq!(body["movies"][0]["ids"]["imdb"], "tt2543164");
    }

    #[tokio::test]
    async fn an_expiring_token_is_refreshed_and_a_dead_one_asks_to_reconnect() {
        let t = signed_in();
        store::set_source_config(&t.ctx.db.lock(), "trakt", json!({ "expires_at": chrono::Utc::now().timestamp() + 60 })).unwrap();
        t.net.on("/oauth/token", 200, r#"{"access_token":"fresh","refresh_token":"ref2","expires_in":7776000}"#);
        Trakt.sync(&t.ctx).await.unwrap();
        assert_eq!(t.ctx.secrets.get("trakt.access").as_deref(), Some("fresh"));
        assert!(t.net.last("/sync/watched/movies").unwrap().headers.iter().any(|(_, v)| v == "Bearer fresh"));

        store::set_source_config(&t.ctx.db.lock(), "trakt", json!({ "expires_at": 0 })).unwrap();
        t.net.on("/oauth/token", 401, "");
        assert_eq!(Trakt.sync(&t.ctx).await.unwrap_err().kind(), "reauth");
    }
}
