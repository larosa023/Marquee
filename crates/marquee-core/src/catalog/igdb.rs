//! IGDB (via Twitch client credentials): the games catalog. Release dates by platform, genres,
//! themes, game modes, similar games, trailers and screenshots.
//! https://api-docs.igdb.com/
//!
//! IGDB renamed several enum fields in 2025 (`category` became `game_type`,
//! `external_games.category` became `external_game_source`). Marquee avoids both spellings:
//! editions and DLC are excluded with `version_parent`/`parent_game`, and Steam app ids are read
//! from store URLs, so the client works whichever names the API currently answers to.

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::*;
use serde_json::{json, Value};

const FIELDS: &str = "name,summary,storyline,first_release_date,cover.image_id,artworks.image_id,screenshots.image_id,\
genres.name,themes.name,keywords.name,game_modes.name,player_perspectives.name,platforms.abbreviation,platforms.name,\
involved_companies.company.name,involved_companies.developer,involved_companies.publisher,\
similar_games.name,similar_games.cover.image_id,similar_games.first_release_date,similar_games.genres.name,similar_games.themes.name,\
videos.video_id,videos.name,total_rating,total_rating_count,hypes,websites.url,external_games.uid,external_games.url,\
release_dates.date,release_dates.human,release_dates.platform.abbreviation,url";

const LIST_FIELDS: &str = "name,summary,first_release_date,cover.image_id,artworks.image_id,genres.name,themes.name,keywords.name,\
game_modes.name,platforms.abbreviation,involved_companies.company.name,involved_companies.developer,involved_companies.publisher,\
total_rating,total_rating_count,hypes,websites.url,external_games.uid,external_games.url";

fn image(id: &str, size: &str) -> String {
    format!("https://images.igdb.com/igdb/image/upload/t_{size}/{id}.jpg")
}

pub fn configured(ctx: &Ctx) -> bool {
    ctx.has_key("igdb_client_id") && ctx.has_key("igdb_client_secret")
}

async fn token(ctx: &Ctx) -> Result<String> {
    if let Some(t) = ctx.scratch_get("twitch.token") {
        return Ok(t);
    }
    let id = ctx.require_key("igdb_client_id")?;
    let secret = ctx.require_key("igdb_client_secret")?;
    let v: Value = ctx
        .http
        .post("Twitch", "https://id.twitch.tv/oauth2/token")
        .form(&[("client_id", &id), ("client_secret", &secret), ("grant_type", "client_credentials")])
        .json()
        .await
        .map_err(|e| match e {
            Error::Api { status: 400 | 403, .. } => Error::NotConfigured("Twitch rejected the IGDB client ID or secret. Check them in Settings → Connections → API keys.".into()),
            other => other,
        })?;
    let t = s(&v, "access_token").ok_or_else(|| Error::msg("Twitch didn't return an access token"))?;
    ctx.scratch_set("twitch.token", &t, i(&v, "expires_in").unwrap_or(3600).saturating_sub(300));
    Ok(t)
}

async fn query(ctx: &Ctx, endpoint: &str, body: String, ttl: i64) -> Result<Vec<Value>> {
    let t = token(ctx).await?;
    let id = ctx.require_key("igdb_client_id")?;
    ctx.http
        .post("IGDB", format!("https://api.igdb.com/v4/{endpoint}"))
        .header("Client-ID", id)
        .bearer(&t)
        .header("Accept", "application/json")
        .text_body(body)
        .ttl(ttl)
        .json()
        .await
}

/// Finds a Steam app id in IGDB's store links.
fn steam_appid(v: &Value) -> Option<String> {
    let from_url = |u: &str| -> Option<String> {
        let rest = u.split("store.steampowered.com/app/").nth(1)?;
        let id: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        (!id.is_empty()).then_some(id)
    };
    arr(v, "external_games")
        .iter()
        .filter_map(|e| e["url"].as_str())
        .chain(arr(v, "websites").iter().filter_map(|w| w["url"].as_str()))
        .find_map(from_url)
}

pub fn draft(v: &Value, detail: bool) -> Option<ItemDraft> {
    let mut d = ItemDraft::new(Media::Game, s(v, "name")?).id("igdb", i(v, "id")?);
    if let Some(app) = steam_appid(v) {
        d.ids.push(("steam".into(), app));
    }
    d.release_date = i(v, "first_release_date").and_then(unix_to_date);
    d.poster = s(&v["cover"], "image_id").map(|c| image(&c, "cover_big"));
    d.backdrop = arr(v, "artworks").first().or_else(|| arr(v, "screenshots").first()).and_then(|a| s(a, "image_id")).map(|a| image(&a, "1080p"));
    d.overview = s(v, "summary");
    d.rating = f(v, "total_rating").filter(|_| i(v, "total_rating_count").unwrap_or(0) >= 5).map(|r| (r / 10.0 * 10.0).round() / 10.0);
    let hypes = f(v, "hypes").unwrap_or(0.0);
    let votes = f(v, "total_rating_count").unwrap_or(0.0);
    d.popularity = (hypes * 2.0 + votes.sqrt() * 3.0).min(500.0);
    d.genres = names(v, "genres", "name");
    d.tags = names(v, "themes", "name").into_iter().chain(names(v, "keywords", "name")).chain(names(v, "game_modes", "name")).chain(names(v, "player_perspectives", "name")).collect();
    d.tag_source = TagSource::Igdb;
    for c in arr(v, "involved_companies") {
        if let Some(name) = s(&c["company"], "name") {
            if c["developer"] == true {
                d.creators.push(Creator::new(name.clone(), "developer"));
                if !d.data.contains_key("developer") {
                    d.set("developer", name.clone());
                }
            }
            if c["publisher"] == true {
                d.creators.push(Creator::new(name, "publisher"));
            }
        }
    }
    let platforms: Vec<String> = arr(v, "platforms").iter().filter_map(|p| s(p, "abbreviation").or_else(|| s(p, "name"))).collect();
    d.set("platforms", platforms);
    d.set("hypes", hypes);
    d.ttl_secs = 3 * 86_400;
    if detail {
        d.detail_level = 1;
        d.set("storyline", s(v, "storyline"));
        d.set("igdb_url", s(v, "url"));
        if let Some(vid) = arr(v, "videos").first() {
            d.set("trailer", json!({ "site": "youtube", "key": vid["video_id"], "name": vid["name"] }));
        }
        let gallery: Vec<String> = arr(v, "screenshots").iter().chain(arr(v, "artworks")).filter_map(|x| s(x, "image_id")).take(10).map(|x| image(&x, "screenshot_big")).collect();
        d.set("gallery", gallery);
        let mut dates: Vec<Value> = arr(v, "release_dates")
            .iter()
            .filter_map(|r| Some(json!({ "platform": s(&r["platform"], "abbreviation")?, "date": i(r, "date").and_then(unix_to_date), "human": r["human"] })))
            .collect();
        dates.sort_by(|a, b| a["date"].as_str().cmp(&b["date"].as_str()));
        dates.dedup_by(|a, b| a["platform"] == b["platform"]);
        d.set("release_dates", dates);
        d.similar = arr(v, "similar_games").iter().take(10).filter_map(|x| draft(x, false)).collect();
    }
    Some(d)
}

/// Games releasing between two dates, most anticipated first.
pub async fn games_between(ctx: &Ctx, from_ts: i64, to_ts: i64, limit: u32) -> Result<Vec<ItemDraft>> {
    let body = format!(
        "fields {LIST_FIELDS}; where first_release_date >= {from_ts} & first_release_date <= {to_ts} & version_parent = null & parent_game = null & cover != null; sort hypes desc; limit {limit};"
    );
    Ok(query(ctx, "games", body, 12 * 3600).await?.iter().filter_map(|v| draft(v, false)).collect())
}

/// Recently released games people actually rated.
pub async fn recent_releases(ctx: &Ctx, from_ts: i64, to_ts: i64, limit: u32) -> Result<Vec<ItemDraft>> {
    let body = format!(
        "fields {LIST_FIELDS}; where first_release_date >= {from_ts} & first_release_date <= {to_ts} & version_parent = null & parent_game = null & cover != null & total_rating_count > 3; sort total_rating_count desc; limit {limit};"
    );
    Ok(query(ctx, "games", body, 12 * 3600).await?.iter().filter_map(|v| draft(v, false)).collect())
}

/// The most-rated games of all time, for the "pick things you love" seeder.
pub async fn all_time(ctx: &Ctx, limit: u32) -> Result<Vec<ItemDraft>> {
    let body = format!("fields {LIST_FIELDS}; where version_parent = null & parent_game = null & cover != null & total_rating_count > 300; sort total_rating_count desc; limit {limit};");
    Ok(query(ctx, "games", body, 7 * 86_400).await?.iter().filter_map(|v| draft(v, false)).collect())
}

pub async fn details(ctx: &Ctx, igdb_id: &str) -> Result<ItemDraft> {
    let rows = query(ctx, "games", format!("fields {FIELDS}; where id = {igdb_id};"), 2 * 86_400).await?;
    rows.first().and_then(|v| draft(v, true)).ok_or_else(|| Error::msg("IGDB doesn't have that game anymore."))
}

pub async fn search(ctx: &Ctx, text: &str, limit: u32) -> Result<Vec<ItemDraft>> {
    let q = text.replace(['"', '\\', ';'], " ");
    let body = format!("search \"{q}\"; fields {LIST_FIELDS}; where version_parent = null; limit {limit};");
    Ok(query(ctx, "games", body, 86_400).await?.iter().filter_map(|v| draft(v, false)).collect())
}

/// Looks up the IGDB entries for Steam app ids, so library games get themes and keywords.
pub async fn by_steam_ids(ctx: &Ctx, appids: &[String]) -> Result<Vec<ItemDraft>> {
    let mut out = vec![];
    for chunk in appids.chunks(100) {
        let list = chunk.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(",");
        // `uid` is shared by every store, so only keep rows whose URL is the Steam page for that id.
        let rows = query(ctx, "external_games", format!("fields uid,url,game; where uid = ({list}); limit 500;"), 7 * 86_400).await?;
        let game_ids: Vec<String> = rows
            .iter()
            .filter(|r| match (r["url"].as_str(), r["uid"].as_str()) {
                (Some(u), Some(uid)) => u.contains(&format!("store.steampowered.com/app/{uid}")),
                _ => false,
            })
            .filter_map(|r| i(r, "game"))
            .map(|g| g.to_string())
            .collect();
        if game_ids.is_empty() {
            continue;
        }
        let rows = query(ctx, "games", format!("fields {LIST_FIELDS}; where id = ({}); limit 500;", game_ids.join(",")), 7 * 86_400).await?;
        out.extend(rows.iter().filter_map(|v| draft(v, false)).filter(|d| d.ids.iter().any(|(s, id)| s == "steam" && chunk.contains(id))));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    const GAMES: &str = include_str!("../../tests/fixtures/igdb_games.json");
    const TOKEN: &str = r#"{"access_token":"tw-token","expires_in":5000000,"token_type":"bearer"}"#;

    #[tokio::test]
    async fn fetches_a_token_once_and_sends_both_headers() {
        let t = ctx();
        t.net.on("id.twitch.tv/oauth2/token", 200, TOKEN);
        t.net.on("api.igdb.com/v4/games", 200, GAMES);
        let a = games_between(&t.ctx, 1, 2, 50).await.unwrap();
        let _ = recent_releases(&t.ctx, 1, 2, 50).await.unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(t.net.calls("id.twitch.tv"), 1);
        let sent = t.net.last("api.igdb.com/v4/games").unwrap();
        assert!(sent.headers.iter().any(|(k, v)| k == "Client-ID" && v == "test-igdb_client_id"));
        assert!(sent.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer tw-token"));
        let body = String::from_utf8(sent.body.unwrap()).unwrap();
        assert!(body.contains("version_parent = null"), "editions are excluded without the renamed enum fields");
        assert!(!body.contains("category"));
    }

    #[tokio::test]
    async fn drafts_carry_steam_ids_companies_and_tags() {
        let t = ctx();
        t.net.on("id.twitch.tv/oauth2/token", 200, TOKEN);
        t.net.on("api.igdb.com/v4/games", 200, GAMES);
        let d = details(&t.ctx, "11737").await.unwrap();
        assert_eq!(d.title, "Outer Wilds");
        assert!(d.ids.contains(&("steam".into(), "753640".into())));
        assert!(d.creators.contains(&Creator::new("Mobius Digital", "developer")));
        assert!(d.creators.contains(&Creator::new("Annapurna Interactive", "publisher")));
        assert!(d.tags.iter().any(|t| t == "Open world") && d.tags.iter().any(|t| t == "space"));
        assert_eq!(d.release_date.as_deref(), Some("2019-05-28"));
        assert_eq!(d.data["trailer"]["key"], "d6LGnVCL1_A");
        assert_eq!(d.data["platforms"][0], "PC");
        assert_eq!(d.similar.len(), 1);
    }

    #[tokio::test]
    async fn steam_lookup_ignores_other_stores_with_the_same_uid() {
        let t = ctx();
        t.net.on("id.twitch.tv/oauth2/token", 200, TOKEN);
        t.net.on(
            "api.igdb.com/v4/external_games",
            200,
            r#"[{"id":1,"uid":"753640","url":"https://store.steampowered.com/app/753640","game":11737},
                {"id":2,"uid":"753640","url":"https://www.gog.com/game/something_else","game":999}]"#,
        );
        t.net.on("api.igdb.com/v4/games", 200, GAMES);
        let found = by_steam_ids(&t.ctx, &["753640".to_string()]).await.unwrap();
        assert_eq!(found.len(), 1);
        let body = String::from_utf8(t.net.last("api.igdb.com/v4/games").unwrap().body.unwrap()).unwrap();
        assert!(body.contains("where id = (11737)"), "{body}");
    }

    #[tokio::test]
    async fn bad_credentials_point_at_settings() {
        let t = ctx();
        t.net.on("id.twitch.tv/oauth2/token", 400, r#"{"status":400,"message":"invalid client"}"#);
        let err = search(&t.ctx, "outer wilds", 5).await.unwrap_err();
        assert_eq!(err.kind(), "not_configured");
    }
}
