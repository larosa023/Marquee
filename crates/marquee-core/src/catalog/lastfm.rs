//! Last.fm API client: similar artists and tags (catalog), plus a user's listening history.
//! https://www.last.fm/api
//!
//! Reads need only an API key and a username. Loving a track needs a signed session.

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::util::*;
use md5::{Digest, Md5};
use serde_json::Value;

const BASE: &str = "https://ws.audioscrobbler.com/2.0/";

/// Tags that say nothing about how music sounds.
const NOISE_TAGS: &[&str] = &["seen live", "favorites", "favourites", "favorite", "albums i own", "love", "awesome", "beautiful", "all", "spotify", "under 2000 listeners", "female vocalists", "male vocalists", "american", "british", "usa", "uk", "00s", "10s", "20s", "90s", "70s", "60s"];

pub async fn call(ctx: &Ctx, method: &str, params: &[(&str, &str)], ttl: i64) -> Result<Value> {
    let key = ctx.require_key("lastfm_key")?;
    let v: Value = ctx.http.get("Last.fm", BASE).query(&[("method", method)]).query(params).query(&[("api_key", &key), ("format", "json")]).ttl(ttl).send_raw().await.and_then(|r| r.json())?;
    check(v).inspect_err(|_| ctx.http.forget(&format!("method={method}")))
}

/// Last.fm reports errors in the body, often with HTTP 200.
fn check(v: Value) -> Result<Value> {
    match i(&v, "error") {
        None => Ok(v),
        Some(6) => Err(Error::msg(format!("Last.fm couldn't find that: {}", s(&v, "message").unwrap_or_default()))),
        Some(4 | 9 | 14) => Err(Error::Reauth("Last.fm".into())),
        Some(10 | 26) => Err(Error::NotConfigured("Last.fm rejected the API key. Check it in Settings → Connections → API keys.".into())),
        Some(17) => Err(Error::msg("That Last.fm profile is private. Turn off \"Hide recent listening\" in your Last.fm privacy settings, or sign in.")),
        Some(code) => Err(Error::Api { service: "Last.fm".into(), status: code as u16, detail: s(&v, "message").unwrap_or_default() }),
    }
}

/// md5 of every parameter (sorted by name, name and value concatenated) plus the shared secret.
pub fn sign(params: &[(&str, &str)], secret: &str) -> String {
    let mut sorted: Vec<&(&str, &str)> = params.iter().filter(|(k, _)| *k != "format" && *k != "callback").collect();
    sorted.sort_by_key(|(k, _)| *k);
    let mut h = Md5::new();
    for (k, v) in sorted {
        h.update(k.as_bytes());
        h.update(v.as_bytes());
    }
    h.update(secret.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A signed write (POST). `sk` is the session key from `auth.getSession`.
pub async fn signed_post(ctx: &Ctx, method: &str, params: &[(&str, &str)], sk: Option<&str>) -> Result<Value> {
    let key = ctx.require_key("lastfm_key")?;
    let secret = ctx.require_key("lastfm_secret")?;
    let mut all: Vec<(&str, &str)> = params.to_vec();
    all.push(("method", method));
    all.push(("api_key", &key));
    if let Some(sk) = sk {
        all.push(("sk", sk));
    }
    let sig = sign(&all, &secret);
    all.push(("api_sig", &sig));
    all.push(("format", "json"));
    let v: Value = ctx.http.post("Last.fm", BASE).form(&all).send_raw().await.and_then(|r| r.json())?;
    check(v)
}

fn clean_tags(v: &Value, limit: usize) -> Vec<String> {
    list(&v["tag"])
        .into_iter()
        .filter(|t| i(t, "count").map(|c| c >= 8).unwrap_or(true))
        .filter_map(|t| s(t, "name"))
        .map(|t| t.to_lowercase())
        .filter(|t| !NOISE_TAGS.contains(&t.as_str()) && t.len() <= 30)
        .take(limit)
        .collect()
}

/// An artist's most-used tags, strongest first.
pub async fn artist_tags(ctx: &Ctx, artist: &str) -> Result<Vec<String>> {
    let v = call(ctx, "artist.getTopTags", &[("artist", artist), ("autocorrect", "1")], 30 * 86_400).await?;
    Ok(clean_tags(&v["toptags"], 10))
}

pub async fn album_tags(ctx: &Ctx, artist: &str, album: &str) -> Result<Vec<String>> {
    let v = call(ctx, "album.getTopTags", &[("artist", artist), ("album", album), ("autocorrect", "1")], 30 * 86_400).await?;
    Ok(clean_tags(&v["toptags"], 10))
}

/// (name, mbid, match 0..1)
pub async fn similar_artists(ctx: &Ctx, artist: &str, limit: u32) -> Result<Vec<(String, Option<String>, f64)>> {
    let v = call(ctx, "artist.getSimilar", &[("artist", artist), ("autocorrect", "1"), ("limit", &limit.to_string())], 14 * 86_400).await?;
    Ok(list(&v["similarartists"]["artist"]).into_iter().filter_map(|a| Some((s(a, "name")?, s(a, "mbid"), f(a, "match").unwrap_or(0.0)))).collect())
}

/// The global chart, used to seed music when no account is connected.
pub async fn chart_artists(ctx: &Ctx, limit: u32) -> Result<Vec<(String, Option<String>)>> {
    let v = call(ctx, "chart.getTopArtists", &[("limit", &limit.to_string())], 86_400).await?;
    Ok(list(&v["artists"]["artist"]).into_iter().filter_map(|a| Some((s(a, "name")?, s(a, "mbid")))).collect())
}

/// Well-known albums carrying a tag.
pub async fn tag_albums(ctx: &Ctx, tag: &str, limit: u32) -> Result<Vec<Played>> {
    let v = call(ctx, "tag.getTopAlbums", &[("tag", tag), ("limit", &limit.to_string())], 7 * 86_400).await?;
    Ok(list(&v["albums"]["album"])
        .into_iter()
        .filter_map(|a| Some(Played { artist: s(&a["artist"], "name")?, name: s(a, "name"), mbid: s(a, "mbid"), plays: 0, image: image(a) }))
        .collect())
}

pub async fn album_info(ctx: &Ctx, artist: &str, album: &str) -> Result<Value> {
    let v = call(ctx, "album.getInfo", &[("artist", artist), ("album", album), ("autocorrect", "1")], 14 * 86_400).await?;
    Ok(v["album"].clone())
}

// ------------------------------------------------------------ a user's history

#[derive(Debug, Clone)]
pub struct Played {
    pub artist: String,
    pub name: Option<String>,
    pub mbid: Option<String>,
    pub plays: i64,
    pub image: Option<String>,
}

fn image(v: &Value) -> Option<String> {
    list(&v["image"]).into_iter().rev().find_map(|im| s(im, "#text")).filter(|u| !u.contains("2a96cbd8b46e442fc41c2b86b821562f"))
}

pub async fn user_info(ctx: &Ctx, user: &str) -> Result<Value> {
    Ok(call(ctx, "user.getInfo", &[("user", user)], 0).await?["user"].clone())
}

/// period: overall | 7day | 1month | 3month | 6month | 12month
pub async fn top_artists(ctx: &Ctx, user: &str, period: &str, limit: u32) -> Result<Vec<Played>> {
    let v = call(ctx, "user.getTopArtists", &[("user", user), ("period", period), ("limit", &limit.to_string())], 3600).await?;
    Ok(list(&v["topartists"]["artist"])
        .into_iter()
        .filter_map(|a| Some(Played { artist: s(a, "name")?, name: None, mbid: s(a, "mbid"), plays: i(a, "playcount").unwrap_or(0), image: None }))
        .collect())
}

pub async fn top_albums(ctx: &Ctx, user: &str, period: &str, limit: u32) -> Result<Vec<Played>> {
    let v = call(ctx, "user.getTopAlbums", &[("user", user), ("period", period), ("limit", &limit.to_string())], 3600).await?;
    Ok(list(&v["topalbums"]["album"])
        .into_iter()
        .filter_map(|a| Some(Played { artist: s(&a["artist"], "name")?, name: s(a, "name"), mbid: s(a, "mbid"), plays: i(a, "playcount").unwrap_or(0), image: image(a) }))
        .collect())
}

/// (artist, track, loved-at unix)
pub async fn loved_tracks(ctx: &Ctx, user: &str, limit: u32) -> Result<Vec<(String, String, Option<i64>)>> {
    let v = call(ctx, "user.getLovedTracks", &[("user", user), ("limit", &limit.to_string())], 3600).await?;
    Ok(list(&v["lovedtracks"]["track"]).into_iter().filter_map(|t| Some((s(&t["artist"], "name")?, s(t, "name")?, i(&t["date"], "uts")))).collect())
}

/// How many scrobbles fall between two unix timestamps (reads only the total).
pub async fn scrobbles_between(ctx: &Ctx, user: &str, from: i64, to: i64) -> Result<i64> {
    let v = call(ctx, "user.getRecentTracks", &[("user", user), ("from", &from.to_string()), ("to", &to.to_string()), ("limit", "1")], if to < chrono::Utc::now().timestamp() - 86_400 { 30 * 86_400 } else { 3600 }).await?;
    Ok(i(&v["recenttracks"]["@attr"], "total").unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn signatures_match_the_documented_algorithm() {
        // md5("api_keyxxxmethodauth.getSessiontokenyyy" + "secret")
        let sig = sign(&[("token", "yyy"), ("method", "auth.getSession"), ("api_key", "xxx"), ("format", "json")], "secret");
        let mut h = Md5::new();
        h.update(b"api_keyxxxmethodauth.getSessiontokenyyysecret");
        let expected: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(sig, expected);
        assert_eq!(sig.len(), 32);
    }

    #[tokio::test]
    async fn top_artists_and_tags_parse() {
        let t = ctx();
        t.net.on("method=user.getTopArtists", 200, include_str!("../../tests/fixtures/lastfm_top_artists.json"));
        t.net.on("method=artist.getTopTags", 200, include_str!("../../tests/fixtures/lastfm_artist_tags.json"));
        let top = top_artists(&t.ctx, "anthony", "overall", 50).await.unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].artist, "Tycho");
        assert_eq!(top[0].plays, 1842);
        let tags = artist_tags(&t.ctx, "Tycho").await.unwrap();
        assert_eq!(tags, vec!["ambient", "chillwave", "electronic"], "noise tags and rarely-used tags are dropped");
    }

    #[tokio::test]
    async fn one_result_arrives_as_an_object_not_an_array() {
        let t = ctx();
        t.net.on("method=artist.getSimilar", 200, r#"{"similarartists":{"artist":{"name":"Boards of Canada","mbid":"abc","match":"0.83"}}}"#);
        let sim = similar_artists(&t.ctx, "Tycho", 5).await.unwrap();
        assert_eq!(sim, vec![("Boards of Canada".to_string(), Some("abc".to_string()), 0.83)]);
    }

    #[tokio::test]
    async fn errors_in_a_200_body_are_still_errors() {
        let t = ctx();
        t.net.on("method=user.getTopArtists", 200, r#"{"error":17,"message":"Login: User required to be logged in"}"#);
        let e = top_artists(&t.ctx, "private", "overall", 5).await.unwrap_err();
        assert!(e.to_string().contains("private"));
        t.net.on("method=user.getTopArtists", 403, r#"{"error":10,"message":"Invalid API key"}"#);
        assert_eq!(top_artists(&t.ctx, "x", "7day", 5).await.unwrap_err().kind(), "not_configured");
    }

    #[tokio::test]
    async fn signed_posts_include_the_session_and_signature() {
        let t = ctx();
        t.net.on("POST https://ws.audioscrobbler.com", 200, "{}");
        signed_post(&t.ctx, "track.love", &[("artist", "Tycho"), ("track", "Awake")], Some("sess")).await.unwrap();
        let body = String::from_utf8(t.net.last("POST").unwrap().body.unwrap()).unwrap();
        assert!(body.contains("sk=sess") && body.contains("api_sig=") && body.contains("method=track.love"));
    }
}
