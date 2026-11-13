//! Spotify: top artists and tracks, saved albums, followed artists, playlists and recently
//! played; plus saving albums, following artists and adding to playlists.
//! https://developer.spotify.com/documentation/web-api
//!
//! Written against the Web API as it stands for Development Mode apps after the February 2026
//! changes:
//! * saving and following go through `PUT /me/library` with Spotify URIs
//! * playlist contents are `/playlists/{id}/items`; playlists are created with `POST /me/playlists`
//! * `GET /browse/new-releases`, artist top tracks and the batch endpoints are gone, and
//!   `popularity`/`label`/`followers` fields are no longer returned, so none are used
//! * search returns at most 10 results per page
//! * recommendations and audio features were closed to new apps in 2024 and are never called:
//!   music recommendations come from Last.fm similarity, MusicBrainz tags and the taste engine
//! * refresh tokens expire six months after the original sign-in, so an `invalid_grant` on
//!   refresh asks the user to reconnect rather than failing silently

use super::{cfg, item_for_write, oauth, set_cfg, Source, Synced, WriteOp, WriteOutcome};
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use crate::util::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

const API: &str = "https://api.spotify.com/v1";
const ACCOUNTS: &str = "https://accounts.spotify.com";
pub const SCOPES: &str = "user-top-read user-library-read user-library-modify user-follow-read user-follow-modify playlist-read-private playlist-modify-private playlist-modify-public user-read-recently-played";
pub const PICKS_PLAYLIST: &str = "Marquee Picks";

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{}/spotify", oauth::PORT)
}

pub struct Spotify;

pub fn authorize_url(client_id: &str, challenge: &str, state: &str) -> String {
    let params = [
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", &redirect_uri()),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge),
        ("state", state),
        ("scope", SCOPES),
    ];
    format!("{ACCOUNTS}/authorize?{}", params.iter().map(|(k, v)| format!("{k}={}", urlencoding::encode(v))).collect::<Vec<_>>().join("&"))
}

/// Runs the whole PKCE sign-in: opens the browser, waits for the redirect, swaps the code for tokens.
pub async fn connect(ctx: &Ctx) -> Result<String> {
    let client_id = ctx.require_key("spotify_client_id")?;
    let (verifier, challenge) = oauth::pkce();
    let state = oauth::random_string(24);
    ctx.host.open_url(&authorize_url(&client_id, &challenge, &state));
    let params = oauth::wait_for_redirect(oauth::PORT, "/spotify", Duration::from_secs(300)).await?;
    finish_connect(ctx, &params, &verifier, &state).await
}

pub async fn finish_connect(ctx: &Ctx, params: &HashMap<String, String>, verifier: &str, state: &str) -> Result<String> {
    if let Some(err) = params.get("error") {
        return Err(Error::msg(if err == "access_denied" { "You declined the request on Spotify, so nothing was connected.".to_string() } else { format!("Spotify sign-in failed: {err}") }));
    }
    if params.get("state").map(String::as_str) != Some(state) {
        return Err(Error::msg("The Spotify sign-in response didn't match this request. Try connecting again."));
    }
    let code = params.get("code").ok_or_else(|| Error::msg("Spotify didn't return a sign-in code."))?;
    let client_id = ctx.require_key("spotify_client_id")?;
    let v: Value = ctx
        .http
        .post("Spotify", format!("{ACCOUNTS}/api/token"))
        .form(&[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", &redirect_uri()), ("client_id", &client_id), ("code_verifier", verifier)])
        .json()
        .await
        .map_err(|e| match e {
            Error::Api { status: 400, detail, .. } if detail.contains("redirect") => Error::NotConfigured(format!(
                "Spotify rejected the redirect address. In your Spotify app's settings, add exactly {} as a Redirect URI.",
                redirect_uri()
            )),
            other => other,
        })?;
    save_tokens(ctx, &v)?;
    let me = get(ctx, "/me", &[], 0).await?;
    let name = s(&me, "display_name").or_else(|| s(&me, "id")).unwrap_or_else(|| "Spotify".into());
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "spotify", json!({ "user_id": s(&me, "id"), "connected_at": chrono::Utc::now().timestamp() }))?;
    store::set_source(&conn, "spotify", "connected", Some(&name), None)?;
    Ok(name)
}

fn save_tokens(ctx: &Ctx, v: &Value) -> Result<()> {
    let access = s(v, "access_token").ok_or_else(|| Error::msg("Spotify didn't return an access token"))?;
    ctx.secrets.set("spotify.access", &access)?;
    // Spotify only sends a new refresh token sometimes; keep the old one otherwise.
    if let Some(r) = s(v, "refresh_token") {
        ctx.secrets.set("spotify.refresh", &r)?;
    }
    set_cfg(ctx, "spotify", json!({ "expires_at": chrono::Utc::now().timestamp() + i(v, "expires_in").unwrap_or(3600) }))
}

async fn token(ctx: &Ctx) -> Result<String> {
    let access = ctx.secrets.get("spotify.access").ok_or_else(|| Error::Reauth("Spotify".into()))?;
    if i(&cfg(ctx, "spotify"), "expires_at").unwrap_or(0) - chrono::Utc::now().timestamp() > 120 {
        return Ok(access);
    }
    let refresh = ctx.secrets.get("spotify.refresh").ok_or_else(|| Error::Reauth("Spotify".into()))?;
    let client_id = ctx.require_key("spotify_client_id")?;
    let v: Value = ctx
        .http
        .post("Spotify", format!("{ACCOUNTS}/api/token"))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client_id)])
        .json()
        .await
        .map_err(|e| match e {
            // invalid_grant: revoked, or past the six-month refresh-token lifetime.
            Error::Api { status: 400 | 401, .. } => Error::Reauth("Spotify".into()),
            other => other,
        })?;
    save_tokens(ctx, &v)?;
    s(&v, "access_token").ok_or_else(|| Error::Reauth("Spotify".into()))
}

fn explain(e: Error) -> Error {
    match e {
        Error::Api { status: 403, detail, .. } => Error::msg(if detail.to_lowercase().contains("premium") {
            "Spotify only allows this for apps whose owner has Premium.".to_string()
        } else {
            "Spotify refused this request. Development Mode apps are limited: the Spotify account must be added under \"User Management\" in the app's dashboard, and its owner needs Premium.".to_string()
        }),
        other => other,
    }
}

async fn get(ctx: &Ctx, path: &str, query: &[(&str, &str)], ttl: i64) -> Result<Value> {
    let t = token(ctx).await?;
    let user = s(&cfg(ctx, "spotify"), "user_id").unwrap_or_default();
    let url = if path.starts_with("http") { path.to_string() } else { format!("{API}{path}") };
    let resp = ctx.http.get("Spotify", url).bearer(&t).query(query).ttl(ttl).vary(user).send().await.map_err(explain)?;
    if resp.body.is_empty() {
        return Ok(Value::Null); // 204: nothing played recently, for example
    }
    resp.json()
}

async fn send(ctx: &Ctx, method: &str, path: &str, body: Value) -> Result<Value> {
    let t = token(ctx).await?;
    let resp = ctx.http.request("Spotify", method, format!("{API}{path}")).bearer(&t).json_body(&body).send().await.map_err(explain)?;
    Ok(if resp.body.is_empty() { Value::Null } else { resp.json().unwrap_or(Value::Null) })
}

/// Follows `next` links until everything is read (bounded).
async fn paged(ctx: &Ctx, path: &str, query: &[(&str, &str)], root: Option<&str>, max_pages: usize) -> Result<Vec<Value>> {
    let mut out = vec![];
    let mut v = get(ctx, path, query, 900).await?;
    for _ in 0..max_pages {
        let page = match root {
            Some(r) => &v[r],
            None => &v,
        };
        out.extend(arr(page, "items").iter().cloned());
        match s(page, "next") {
            Some(next) => v = get(ctx, &next, &[], 900).await?,
            None => break,
        }
    }
    Ok(out)
}

fn best_image(v: &Value) -> Option<String> {
    arr(v, "images").iter().max_by_key(|im| i(im, "width").unwrap_or(0).min(700)).and_then(|im| s(im, "url"))
}

fn artist_draft(a: &Value) -> Option<ItemDraft> {
    let name = s(a, "name")?;
    let mut d = ItemDraft::new(Media::Artist, name.clone()).id("spotify", s(a, "id")?).id("artist", slug(&name));
    d.poster = best_image(a);
    d.tags = arr(a, "genres").iter().filter_map(|g| g.as_str().map(String::from)).collect();
    d.tag_source = TagSource::Lastfm;
    d.creators.push(Creator::new(name, "artist"));
    Some(d)
}

fn album_draft(al: &Value) -> Option<ItemDraft> {
    let artist = s(&al["artists"][0], "name")?;
    let mut d = ItemDraft::new(Media::Album, s(al, "name")?).id("spotify", s(al, "id")?);
    d.release_date = s(al, "release_date").and_then(|x| partial_date(&x));
    d.poster = best_image(al);
    d.creators.push(Creator::new(artist.clone(), "artist"));
    d.set("artist", artist);
    d.set("album_type", s(al, "album_type").map(|t| if t == "compilation" { "album".to_string() } else { t }));
    d.set("spotify_url", s(&al["external_urls"], "spotify"));
    d.tag_source = TagSource::Lastfm;
    Some(d)
}

/// The user's own playlists (the ones they can add to).
pub async fn playlists(ctx: &Ctx) -> Result<Vec<Value>> {
    let me = s(&cfg(ctx, "spotify"), "user_id").unwrap_or_default();
    let rows = paged(ctx, "/me/playlists", &[("limit", "50")], None, 6).await?;
    Ok(rows
        .iter()
        .filter(|p| s(&p["owner"], "id").as_deref() == Some(me.as_str()) || p["collaborative"] == true)
        .map(|p| json!({ "id": p["id"], "name": p["name"], "image": best_image(p), "count": p["items"]["total"].as_i64().or(p["tracks"]["total"].as_i64()) }))
        .collect())
}

pub async fn create_playlist(ctx: &Ctx, name: &str, description: &str) -> Result<String> {
    let v = send(ctx, "POST", "/me/playlists", json!({ "name": name, "description": description, "public": false })).await?;
    ctx.http.forget("/me/playlists");
    s(&v, "id").ok_or_else(|| Error::msg("Spotify didn't return the new playlist."))
}

/// Finds or creates the "Marquee Picks" playlist and returns its id.
pub async fn picks_playlist(ctx: &Ctx) -> Result<String> {
    if let Some(id) = s(&cfg(ctx, "spotify"), "picks_playlist") {
        return Ok(id);
    }
    let existing = playlists(ctx).await?.into_iter().find(|p| p["name"] == PICKS_PLAYLIST).and_then(|p| s(&p, "id"));
    let id = match existing {
        Some(id) => id,
        None => create_playlist(ctx, PICKS_PLAYLIST, "Albums and tracks you saved from Marquee.").await?,
    };
    set_cfg(ctx, "spotify", json!({ "picks_playlist": id }))?;
    Ok(id)
}

/// Finds an album on Spotify. Returns (album id, web url).
pub async fn find_album(ctx: &Ctx, artist: &str, title: &str) -> Result<Option<(String, String)>> {
    let q = format!("album:{} artist:{}", title.replace(':', " "), artist.replace(':', " "));
    let v = get(ctx, "/search", &[("q", &q), ("type", "album"), ("limit", "5")], 30 * 86_400).await?;
    let norm = |x: &str| x.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    Ok(arr(&v["albums"], "items")
        .iter()
        .find(|a| norm(&s(a, "name").unwrap_or_default()).starts_with(&norm(title)) || norm(title).starts_with(&norm(&s(a, "name").unwrap_or_default())))
        .or_else(|| arr(&v["albums"], "items").first())
        .and_then(|a| Some((s(a, "id")?, s(&a["external_urls"], "spotify").unwrap_or_default()))))
}

pub async fn find_artist(ctx: &Ctx, name: &str) -> Result<Option<String>> {
    let v = get(ctx, "/search", &[("q", &format!("artist:{}", name.replace(':', " "))), ("type", "artist"), ("limit", "5")], 30 * 86_400).await?;
    Ok(arr(&v["artists"], "items").iter().find(|a| s(a, "name").map(|n| n.to_lowercase() == name.to_lowercase()).unwrap_or(false)).and_then(|a| s(a, "id")))
}

async fn album_track_uris(ctx: &Ctx, album_id: &str) -> Result<Vec<String>> {
    let rows = paged(ctx, &format!("/albums/{album_id}/tracks"), &[("limit", "50")], None, 4).await?;
    Ok(rows.iter().filter_map(|t| s(t, "uri")).collect())
}

/// Makes sure an album item has a Spotify id, searching for it if needed.
async fn spotify_album_id(ctx: &Ctx, item: &Item) -> Result<String> {
    if let Some(id) = item.ext("spotify") {
        return Ok(id.to_string());
    }
    let artist = item.data["artist"].as_str().unwrap_or("");
    let (id, url) = find_album(ctx, artist, &item.title).await?.ok_or_else(|| Error::msg(format!("Spotify doesn't seem to have {} by {artist}.", item.title)))?;
    let conn = ctx.db.lock();
    conn.execute("INSERT OR IGNORE INTO item_ids (source, ext_id, media, item_id) VALUES ('spotify', ?1, 'album', ?2)", rusqlite::params![id, item.id])?;
    conn.execute("UPDATE items SET data = json_set(data, '$.spotify_url', ?1) WHERE id = ?2", rusqlite::params![url, item.id])?;
    Ok(id)
}

#[async_trait]
impl Source for Spotify {
    fn id(&self) -> &'static str {
        "spotify"
    }
    fn name(&self) -> &'static str {
        "Spotify"
    }
    fn reads(&self) -> Vec<&'static str> {
        vec!["Top artists and tracks", "Saved albums", "Followed artists", "Playlists", "Recently played"]
    }
    fn writes(&self) -> Vec<&'static str> {
        vec!["Add to playlist", "Save album", "Follow artist", "Create \"Marquee Picks\""]
    }
    fn note(&self) -> &'static str {
        "Spotify limits personal apps: the app owner needs Premium and at most five accounts can use it. Recommendations come from Last.fm and MusicBrainz, not Spotify."
    }
    fn configured(&self, ctx: &Ctx) -> bool {
        ctx.has_key("spotify_client_id")
    }

    async fn sync(&self, ctx: &Ctx) -> Result<Synced> {
        let mut out = Synced::default();
        let now = chrono::Utc::now();
        let ago = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        // Top artists across three windows. Being in several windows means sustained listening.
        let mut artists: HashMap<String, (ItemDraft, f64, i64, i64)> = HashMap::new(); // id -> (draft, weight, newest days, oldest days)
        for (range, days_new, days_old, scale) in [("short_term", 0, 28, 1.0), ("medium_term", 60, 180, 1.2), ("long_term", 200, 500, 1.5)] {
            let v = get(ctx, "/me/top/artists", &[("time_range", range), ("limit", "50")], 3600).await?;
            for (rank, a) in arr(&v, "items").iter().enumerate() {
                let (Some(id), Some(d)) = (s(a, "id"), artist_draft(a)) else { continue };
                let w = (50.0 - rank as f64).max(1.0) * 4.0 * scale;
                let e = artists.entry(id).or_insert((d, 0.0, days_new, days_old));
                e.1 += w;
                e.2 = e.2.min(days_new);
                e.3 = e.3.max(days_old);
            }
        }
        for (_, (d, w, newest, oldest)) in artists {
            let mut ev = RawEvent::new(d, EventKind::Play, w).at(Some(ago(newest)));
            ev.first_at = Some(ago(oldest));
            out.events.push(ev);
        }

        // Top tracks count toward their albums.
        let mut albums: HashMap<String, (ItemDraft, f64, i64, i64)> = HashMap::new();
        for (range, days_new, days_old, scale) in [("short_term", 0, 28, 1.0), ("medium_term", 60, 180, 1.2), ("long_term", 200, 500, 1.5)] {
            let v = get(ctx, "/me/top/tracks", &[("time_range", range), ("limit", "50")], 3600).await?;
            for (rank, tr) in arr(&v, "items").iter().enumerate() {
                let al = &tr["album"];
                if al["album_type"] == "compilation" {
                    continue;
                }
                let (Some(id), Some(d)) = (s(al, "id"), album_draft(al)) else { continue };
                let w = (50.0 - rank as f64).max(1.0) * 1.5 * scale;
                let e = albums.entry(id).or_insert((d, 0.0, days_new, days_old));
                e.1 += w;
                e.2 = e.2.min(days_new);
                e.3 = e.3.max(days_old);
            }
        }
        // Recently played adds a little, dated to the actual play.
        let recent = get(ctx, "/me/player/recently-played", &[("limit", "50")], 600).await.unwrap_or(Value::Null);
        for play in arr(&recent, "items") {
            let al = &play["track"]["album"];
            let (Some(id), Some(d)) = (s(al, "id"), album_draft(al)) else { continue };
            let e = albums.entry(id).or_insert((d, 0.0, 0, 7));
            e.1 += 3.0;
            e.2 = 0;
        }
        for (_, (d, w, newest, oldest)) in albums {
            let mut ev = RawEvent::new(d, EventKind::Play, w).at(Some(ago(newest)));
            ev.first_at = Some(ago(oldest));
            out.events.push(ev);
        }

        for row in paged(ctx, "/me/albums", &[("limit", "50")], None, 20).await? {
            if let Some(d) = album_draft(&row["album"]) {
                out.events.push(RawEvent::new(d, EventKind::Like, 1.0).at(s(&row, "added_at")));
            }
        }

        // Followed artists use cursor paging under an "artists" key.
        let mut followed = vec![];
        for a in paged(ctx, "/me/following", &[("type", "artist"), ("limit", "50")], Some("artists"), 20).await? {
            if let Some(d) = artist_draft(&a) {
                out.events.push(RawEvent::new(d.clone(), EventKind::Like, 1.0));
                followed.push(d);
            }
        }
        out.lists.push(("spotify:following".into(), followed));

        // Any Marquee list mirroring a playlist needs that playlist's albums.
        let mirrors: Vec<String> = {
            let conn = ctx.db.lock();
            let mut stmt = conn.prepare("SELECT mirror FROM lists WHERE mirror LIKE 'spotify:playlist:%'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        for mirror in mirrors {
            let pid = mirror.trim_start_matches("spotify:playlist:");
            let mut seen = std::collections::HashSet::new();
            let mut drafts = vec![];
            match paged(ctx, &format!("/playlists/{pid}/items"), &[("limit", "50")], None, 10).await {
                Ok(rows) => {
                    for row in rows {
                        let track = if row["item"].is_object() { &row["item"] } else { &row["track"] };
                        if let (Some(id), Some(d)) = (s(&track["album"], "id"), album_draft(&track["album"])) {
                            if seen.insert(id) {
                                drafts.push(d);
                            }
                        }
                    }
                    out.lists.push((mirror.clone(), drafts));
                }
                Err(e) => out.warnings.push(format!("Couldn't read a mirrored Spotify playlist: {e}")),
            }
        }
        let pl = playlists(ctx).await.unwrap_or_default();
        set_cfg(ctx, "spotify", json!({ "playlists": pl }))?;
        Ok(out)
    }

    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome> {
        let item = item_for_write(ctx, op.item_id())?;
        match (op, item.media) {
            (WriteOp::AddToList { target, .. }, Media::Album) => {
                let album = spotify_album_id(ctx, &item).await?;
                if target == "library" {
                    send(ctx, "PUT", "/me/library", json!({ "uris": [format!("spotify:album:{album}")] })).await?;
                } else {
                    let playlist = match target.strip_prefix("playlist:") {
                        Some("picks") | None => picks_playlist(ctx).await?,
                        Some(id) => id.to_string(),
                    };
                    let uris = album_track_uris(ctx, &album).await?;
                    for chunk in uris.chunks(100) {
                        send(ctx, "POST", &format!("/playlists/{playlist}/items"), json!({ "uris": chunk })).await?;
                    }
                    ctx.http.forget(&format!("/playlists/{playlist}/items"));
                }
                Ok(WriteOutcome::Synced)
            }
            (WriteOp::RemoveFromList { target, .. }, Media::Album) => {
                let album = spotify_album_id(ctx, &item).await?;
                if target == "library" {
                    send(ctx, "DELETE", "/me/library", json!({ "uris": [format!("spotify:album:{album}")] })).await?;
                } else if let Some(playlist) = target.strip_prefix("playlist:") {
                    let playlist = if playlist == "picks" { picks_playlist(ctx).await? } else { playlist.to_string() };
                    let uris = album_track_uris(ctx, &album).await?;
                    for chunk in uris.chunks(100) {
                        let items: Vec<Value> = chunk.iter().map(|u| json!({ "uri": u })).collect();
                        send(ctx, "DELETE", &format!("/playlists/{playlist}/items"), json!({ "items": items })).await?;
                    }
                    ctx.http.forget(&format!("/playlists/{playlist}/items"));
                }
                Ok(WriteOutcome::Synced)
            }
            (WriteOp::Love { .. }, Media::Album) => {
                let album = spotify_album_id(ctx, &item).await?;
                send(ctx, "PUT", "/me/library", json!({ "uris": [format!("spotify:album:{album}")] })).await?;
                Ok(WriteOutcome::Synced)
            }
            (WriteOp::Follow { .. }, _) => {
                let name = if item.media == Media::Artist { item.title.clone() } else { item.data["artist"].as_str().unwrap_or("").to_string() };
                let id = match (item.media, item.ext("spotify")) {
                    (Media::Artist, Some(id)) => id.to_string(),
                    _ => find_artist(ctx, &name).await?.ok_or_else(|| Error::msg(format!("Spotify doesn't seem to have an artist called {name}.")))?,
                };
                send(ctx, "PUT", "/me/library", json!({ "uris": [format!("spotify:artist:{id}")] })).await?;
                Ok(WriteOutcome::Synced)
            }
            _ => Err(Error::msg("Spotify can only save music.")),
        }
    }

    fn forget(&self, ctx: &Ctx) {
        let _ = ctx.secrets.delete("spotify.access");
        let _ = ctx.secrets.delete("spotify.refresh");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::{ctx, TestCtx};

    fn signed_in() -> TestCtx {
        let t = ctx();
        t.ctx.secrets.set("spotify.access", "acc").unwrap();
        t.ctx.secrets.set("spotify.refresh", "ref").unwrap();
        store::set_source_config(&t.ctx.db.lock(), "spotify", json!({ "expires_at": chrono::Utc::now().timestamp() + 3000, "user_id": "anthony" })).unwrap();
        t.net.on("api.spotify.com/v1/me/", 200, r#"{"items":[],"next":null}"#);
        t.net.on("api.spotify.com/v1/me/following", 200, r#"{"artists":{"items":[],"next":null}}"#);
        t
    }

    #[test]
    fn the_authorize_url_uses_pkce_and_the_loopback_redirect() {
        let url = authorize_url("client", "chal", "st");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?client_id=client&response_type=code"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A43117%2Fspotify"));
        assert!(url.contains("code_challenge_method=S256") && url.contains("code_challenge=chal"));
        assert!(url.contains("playlist-modify-private") && url.contains("user-top-read"));
    }

    #[tokio::test]
    async fn finishing_sign_in_checks_state_and_stores_tokens() {
        let t = ctx();
        let mut params = HashMap::from([("code".to_string(), "abc".to_string()), ("state".to_string(), "wrong".to_string())]);
        assert!(finish_connect(&t.ctx, &params, "ver", "right").await.unwrap_err().to_string().contains("didn't match"));
        params.insert("state".into(), "right".into());
        t.net.on("accounts.spotify.com/api/token", 200, r#"{"access_token":"acc","refresh_token":"ref","expires_in":3600,"token_type":"Bearer"}"#);
        t.net.on("api.spotify.com/v1/me", 200, r#"{"id":"anthony","display_name":"Anthony"}"#);
        assert_eq!(finish_connect(&t.ctx, &params, "ver", "right").await.unwrap(), "Anthony");
        assert_eq!(t.ctx.secrets.get("spotify.refresh").as_deref(), Some("ref"));
        let body = String::from_utf8(t.net.last("api/token").unwrap().body.unwrap()).unwrap();
        assert!(body.contains("code_verifier=ver") && body.contains("grant_type=authorization_code"));
        assert!(!body.contains("client_secret"), "PKCE never sends a client secret");
        // Tokens never land in the database.
        let cfg: String = t.ctx.db.lock().query_row("SELECT config FROM sources WHERE id = 'spotify'", [], |r| r.get(0)).unwrap();
        assert!(!cfg.contains("acc") || !cfg.contains("\"access"));
        assert!(!cfg.contains("ref\""));
    }

    #[tokio::test]
    async fn sync_reads_top_artists_tracks_and_saved_albums() {
        let t = signed_in();
        t.net.on("/me/top/artists", 200, include_str!("../../tests/fixtures/spotify_top_artists.json"));
        t.net.on("/me/top/tracks", 200, include_str!("../../tests/fixtures/spotify_top_tracks.json"));
        t.net.on("/me/albums", 200, r#"{"items":[{"added_at":"2025-05-01T00:00:00Z","album":{"id":"al2","name":"Dive","album_type":"album","release_date":"2011-11-08","artists":[{"name":"Tycho"}],"images":[{"url":"https://i/d.jpg","width":640}]}}],"next":null}"#);
        let synced = Spotify.sync(&t.ctx).await.unwrap();
        let artist = synced.events.iter().find(|e| e.draft.media() == Media::Artist && e.kind == EventKind::Play).unwrap();
        assert_eq!(artist.draft.title, "Tycho");
        assert!(artist.value > 400.0, "in all three windows, so weights add up: {}", artist.value);
        assert!(artist.first_at.as_deref().unwrap() < artist.at.as_deref().unwrap());
        assert!(artist.draft.tags.contains(&"chillwave".to_string()));
        let album_play = synced.events.iter().find(|e| e.draft.media() == Media::Album && e.kind == EventKind::Play).unwrap();
        assert_eq!(album_play.draft.title, "Awake");
        assert_eq!(album_play.draft.data["artist"], "Tycho");
        assert!(synced.events.iter().any(|e| e.kind == EventKind::Like && e.draft.title == "Dive"));
        assert_eq!(t.net.calls("browse/new-releases"), 0);
        assert_eq!(t.net.calls("/recommendations"), 0);
        assert_eq!(t.net.calls("audio-features"), 0);
    }

    #[tokio::test]
    async fn saving_and_playlist_adds_use_the_2026_endpoints() {
        let t = signed_in();
        let id = {
            let mut d = ItemDraft::new(Media::Album, "Dive").id("mbid", "rg-3");
            d.set("artist", "Tycho");
            store::upsert_item(&t.ctx.db.lock(), &d).unwrap()
        };
        t.net.on("/v1/search", 200, r#"{"albums":{"items":[{"id":"al2","name":"Dive","external_urls":{"spotify":"https://open.spotify.com/album/al2"}}]}}"#);
        t.net.on("PUT https://api.spotify.com/v1/me/library", 200, "");
        Spotify.write(&t.ctx, &WriteOp::AddToList { item_id: id, target: "library".into() }).await.unwrap();
        let put = t.net.last("PUT https://api.spotify.com/v1/me/library").unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&put.body.unwrap()).unwrap()["uris"][0], "spotify:album:al2");
        assert_eq!(t.net.calls("/me/albums?ids"), 0, "the removed per-type endpoint is not used");
        // The Spotify id found by search is remembered.
        assert_eq!(store::get_item(&t.ctx.db.lock(), id).unwrap().ext("spotify"), Some("al2"));

        t.net.on("/v1/albums/al2/tracks", 200, r#"{"items":[{"uri":"spotify:track:t1"},{"uri":"spotify:track:t2"}],"next":null}"#);
        t.net.on("POST https://api.spotify.com/v1/playlists/pl9/items", 201, r#"{"snapshot_id":"x"}"#);
        Spotify.write(&t.ctx, &WriteOp::AddToList { item_id: id, target: "playlist:pl9".into() }).await.unwrap();
        let post = t.net.last("POST https://api.spotify.com/v1/playlists/pl9/items").unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&post.body.unwrap()).unwrap()["uris"].as_array().unwrap().len(), 2);
        assert_eq!(t.net.calls("/playlists/pl9/tracks"), 0);
    }

    #[tokio::test]
    async fn marquee_picks_is_created_once_with_the_me_endpoint() {
        let t = signed_in();
        t.net.on("GET https://api.spotify.com/v1/me/playlists", 200, r#"{"items":[{"id":"other","name":"Gym","owner":{"id":"anthony"}}],"next":null}"#);
        t.net.on("POST https://api.spotify.com/v1/me/playlists", 201, r#"{"id":"picks1","name":"Marquee Picks"}"#);
        assert_eq!(picks_playlist(&t.ctx).await.unwrap(), "picks1");
        assert_eq!(picks_playlist(&t.ctx).await.unwrap(), "picks1");
        assert_eq!(t.net.calls("POST https://api.spotify.com/v1/me/playlists"), 1);
        assert_eq!(t.net.calls("/users/"), 0, "the per-user playlist endpoint was removed");
    }

    #[tokio::test]
    async fn a_dead_refresh_token_asks_to_reconnect_and_403_explains_dev_mode() {
        let t = signed_in();
        store::set_source_config(&t.ctx.db.lock(), "spotify", json!({ "expires_at": 0 })).unwrap();
        t.net.on("accounts.spotify.com/api/token", 400, r#"{"error":"invalid_grant","error_description":"Refresh token revoked"}"#);
        assert_eq!(Spotify.sync(&t.ctx).await.unwrap_err().kind(), "reauth");

        let t = signed_in();
        t.net.on("/me/top/artists", 403, r#"{"error":{"status":403,"message":"User not registered in the Developer Dashboard"}}"#);
        assert!(Spotify.sync(&t.ctx).await.unwrap_err().to_string().contains("User Management"));
    }
}
