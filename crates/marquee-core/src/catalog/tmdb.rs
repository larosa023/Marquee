//! TMDB: movies and TV. Metadata, release dates by region, trailers, credits, keywords and
//! watch providers. https://developer.themoviedb.org/reference
//!
//! Accepts either kind of TMDB credential: a v3 API key (sent as `api_key`) or a v4 read
//! access token (sent as a Bearer header).

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::util::*;
use serde_json::{json, Value};

const BASE: &str = "https://api.themoviedb.org/3";
const IMG: &str = "https://image.tmdb.org/t/p";

const MOVIE_GENRES: &[(i64, &str)] = &[
    (28, "Action"),
    (12, "Adventure"),
    (16, "Animation"),
    (35, "Comedy"),
    (80, "Crime"),
    (99, "Documentary"),
    (18, "Drama"),
    (10751, "Family"),
    (14, "Fantasy"),
    (36, "History"),
    (27, "Horror"),
    (10402, "Music"),
    (9648, "Mystery"),
    (10749, "Romance"),
    (878, "Science Fiction"),
    (10770, "TV Movie"),
    (53, "Thriller"),
    (10752, "War"),
    (37, "Western"),
];
const TV_GENRES: &[(i64, &str)] = &[
    (10759, "Action & Adventure"),
    (16, "Animation"),
    (35, "Comedy"),
    (80, "Crime"),
    (99, "Documentary"),
    (18, "Drama"),
    (10751, "Family"),
    (10762, "Kids"),
    (9648, "Mystery"),
    (10763, "News"),
    (10764, "Reality"),
    (10765, "Sci-Fi & Fantasy"),
    (10766, "Soap"),
    (10767, "Talk"),
    (10768, "War & Politics"),
    (37, "Western"),
];

pub fn poster(path: &str) -> String {
    format!("{IMG}/w500{path}")
}
pub fn backdrop(path: &str) -> String {
    format!("{IMG}/w1280{path}")
}
fn small(path: &str) -> String {
    format!("{IMG}/w185{path}")
}

async fn get(ctx: &Ctx, path: &str, query: &[(&str, &str)], ttl: i64) -> Result<Value> {
    let key = ctx.require_key("tmdb")?;
    let mut call = ctx.http.get("TMDB", format!("{BASE}{path}")).query(query).ttl(ttl);
    // v4 tokens are JWTs; v3 keys are 32 hex characters.
    call = if key.starts_with("eyJ") { call.bearer(&key) } else { call.query(&[("api_key", &key)]) };
    call.json().await
}

fn kind(media: Media) -> &'static str {
    if media == Media::Tv {
        "tv"
    } else {
        "movie"
    }
}

/// A draft from a list endpoint (discover, trending, search, similar).
pub fn stub(v: &Value, media: Media) -> Option<ItemDraft> {
    let title = s(v, if media == Media::Tv { "name" } else { "title" }).or_else(|| s(v, "name")).or_else(|| s(v, "title"))?;
    let mut d = ItemDraft::new(media, title).id("tmdb", i(v, "id")?);
    d.release_date = s(v, if media == Media::Tv { "first_air_date" } else { "release_date" }).and_then(|x| date_only(&x));
    d.poster = s(v, "poster_path").map(|p| poster(&p));
    d.backdrop = s(v, "backdrop_path").map(|p| backdrop(&p));
    d.overview = s(v, "overview");
    d.popularity = f(v, "popularity").unwrap_or(0.0);
    d.rating = f(v, "vote_average").filter(|r| *r > 0.0 && i(v, "vote_count").unwrap_or(0) >= 5);
    d.language = s(v, "original_language");
    let table = if media == Media::Tv { TV_GENRES } else { MOVIE_GENRES };
    d.genres = arr(v, "genre_ids").iter().filter_map(|g| g.as_i64()).filter_map(|g| table.iter().find(|t| t.0 == g).map(|t| t.1.to_string())).collect();
    d.tag_source = TagSource::Tmdb;
    d.ttl_secs = 2 * 86_400;
    Some(d)
}

fn stubs(v: &Value, media: Option<Media>) -> Vec<ItemDraft> {
    arr(v, "results")
        .iter()
        .filter_map(|r| {
            let m = match media {
                Some(m) => m,
                None => match r["media_type"].as_str() {
                    Some("movie") => Media::Movie,
                    Some("tv") => Media::Tv,
                    _ => return None,
                },
            };
            stub(r, m)
        })
        .collect()
}

pub async fn trending(ctx: &Ctx, media: Media) -> Result<Vec<ItemDraft>> {
    let v = get(ctx, &format!("/trending/{}/week", kind(media)), &[("language", &ctx.language())], 6 * 3600).await?;
    Ok(stubs(&v, Some(media))
        .into_iter()
        .map(|mut d| {
            d.set("trending", true);
            d
        })
        .collect())
}

/// The most-voted titles of all time: familiar faces for the "pick things you love" seeder.
pub async fn all_time(ctx: &Ctx, media: Media, pages: u32) -> Result<Vec<ItemDraft>> {
    let mut out = vec![];
    for page in 1..=pages {
        let p = page.to_string();
        let v = get(ctx, &format!("/discover/{}", kind(media)), &[("language", &ctx.language()), ("sort_by", "vote_count.desc"), ("include_adult", "false"), ("page", &p)], 7 * 86_400).await?;
        out.extend(stubs(&v, Some(media)));
    }
    Ok(out)
}

/// Movies released in the region between two dates (theatrical, limited or digital).
pub async fn movies_between(ctx: &Ctx, from: &str, to: &str, pages: u32) -> Result<Vec<ItemDraft>> {
    let region = ctx.region();
    let mut out = vec![];
    for page in 1..=pages {
        let p = page.to_string();
        let v = get(
            ctx,
            "/discover/movie",
            &[("language", &ctx.language()), ("region", &region), ("sort_by", "popularity.desc"), ("include_adult", "false"), ("with_release_type", "2|3|4"), ("release_date.gte", from), ("release_date.lte", to), ("page", &p)],
            12 * 3600,
        )
        .await?;
        out.extend(stubs(&v, Some(Media::Movie)));
        if page as i64 >= i(&v, "total_pages").unwrap_or(1) {
            break;
        }
    }
    Ok(out)
}

/// Shows that premiere between two dates.
pub async fn shows_premiering(ctx: &Ctx, from: &str, to: &str, pages: u32) -> Result<Vec<ItemDraft>> {
    let mut out = vec![];
    for page in 1..=pages {
        let p = page.to_string();
        let v = get(
            ctx,
            "/discover/tv",
            &[("language", &ctx.language()), ("sort_by", "popularity.desc"), ("include_adult", "false"), ("watch_region", &ctx.region()), ("first_air_date.gte", from), ("first_air_date.lte", to), ("page", &p)],
            12 * 3600,
        )
        .await?;
        out.extend(stubs(&v, Some(Media::Tv)));
        if page as i64 >= i(&v, "total_pages").unwrap_or(1) {
            break;
        }
    }
    Ok(out)
}

/// Shows with any episode airing between two dates (returning seasons included).
pub async fn shows_airing(ctx: &Ctx, from: &str, to: &str, pages: u32) -> Result<Vec<ItemDraft>> {
    let mut out = vec![];
    for page in 1..=pages {
        let p = page.to_string();
        let v = get(ctx, "/discover/tv", &[("language", &ctx.language()), ("sort_by", "popularity.desc"), ("air_date.gte", from), ("air_date.lte", to), ("page", &p)], 12 * 3600).await?;
        out.extend(stubs(&v, Some(Media::Tv)).into_iter().map(|mut d| {
            d.set("airing", true);
            d
        }));
        if page as i64 >= i(&v, "total_pages").unwrap_or(1) {
            break;
        }
    }
    Ok(out)
}

pub async fn search(ctx: &Ctx, query: &str, media: Option<Media>, year: Option<i32>) -> Result<Vec<ItemDraft>> {
    let path = match media {
        Some(Media::Movie) => "/search/movie",
        Some(Media::Tv) => "/search/tv",
        _ => "/search/multi",
    };
    let y = year.map(|y| y.to_string()).unwrap_or_default();
    let mut q = vec![("query", query), ("language", "en-US"), ("include_adult", "false")];
    if year.is_some() {
        q.push((if media == Some(Media::Tv) { "first_air_date_year" } else { "year" }, &y));
    }
    let v = get(ctx, path, &q, 86_400).await?;
    Ok(stubs(&v, media))
}

pub async fn find_by_imdb(ctx: &Ctx, imdb_id: &str) -> Result<Option<ItemDraft>> {
    let v = get(ctx, &format!("/find/{imdb_id}"), &[("external_source", "imdb_id")], 30 * 86_400).await?;
    if let Some(m) = arr(&v, "movie_results").first() {
        return Ok(stub(m, Media::Movie).map(|d| d.id("imdb", imdb_id)));
    }
    Ok(arr(&v, "tv_results").first().and_then(|t| stub(t, Media::Tv)).map(|d| d.id("imdb", imdb_id)))
}

fn providers(v: &Value, region: &str) -> Value {
    let r = &v["watch/providers"]["results"][region];
    if r.is_null() {
        return Value::Null;
    }
    let group = |key: &str| -> Vec<Value> { arr(r, key).iter().map(|p| json!({ "id": p["provider_id"], "name": p["provider_name"], "logo": s(p, "logo_path").map(|l| small(&l)) })).collect() };
    let free: Vec<Value> = group("free").into_iter().chain(group("ads")).collect();
    json!({ "link": r["link"], "stream": group("flatrate"), "free": free, "rent": group("rent"), "buy": group("buy") })
}

fn trailer(v: &Value) -> Option<Value> {
    let vids = arr(&v["videos"], "results");
    let pick = |kind: &str| vids.iter().find(|x| x["site"] == "YouTube" && x["type"] == kind && x["official"] != false);
    pick("Trailer").or_else(|| pick("Teaser")).or_else(|| vids.iter().find(|x| x["site"] == "YouTube")).map(|x| json!({ "site": "youtube", "key": x["key"], "name": x["name"] }))
}

fn credits(v: &Value, d: &mut ItemDraft) {
    let c = &v["credits"];
    for person in arr(c, "crew") {
        let job = person["job"].as_str().unwrap_or("");
        let role = match job {
            "Director" => "director",
            "Screenplay" | "Writer" | "Story" | "Novel" => "writer",
            "Original Music Composer" => "composer",
            _ => continue,
        };
        if let Some(name) = s(person, "name") {
            if !d.creators.iter().any(|x| x.name == name && x.role == role) {
                d.creators.push(Creator::new(name, role));
            }
        }
    }
    let cast: Vec<Value> = arr(c, "cast").iter().take(12).map(|p| json!({ "id": p["id"], "name": p["name"], "character": p["character"], "photo": s(p, "profile_path").map(|x| small(&x)) })).collect();
    for p in arr(c, "cast").iter().take(4) {
        if let Some(name) = s(p, "name") {
            d.creators.push(Creator::new(name, "cast"));
        }
    }
    d.set("cast", cast);
}

/// Full details for one movie or show.
pub async fn details(ctx: &Ctx, media: Media, tmdb_id: &str) -> Result<ItemDraft> {
    let region = ctx.region();
    let extra = if media == Media::Tv { "credits,keywords,videos,watch/providers,external_ids,recommendations,similar,images,content_ratings" } else { "credits,keywords,videos,watch/providers,release_dates,recommendations,similar,images" };
    let v = get(ctx, &format!("/{}/{tmdb_id}", kind(media)), &[("language", &ctx.language()), ("append_to_response", extra), ("include_image_language", "en,null")], 86_400).await?;
    let mut d = stub(&v, media).ok_or_else(|| crate::Error::msg("TMDB returned a title with no name"))?;
    d.detail_level = 1;
    d.genres = names(&v, "genres", "name");
    d.set("tagline", s(&v, "tagline"));
    d.set("homepage", s(&v, "homepage"));
    d.set("status", s(&v, "status"));
    d.set("trailer", trailer(&v));
    d.set("providers", providers(&v, &region));
    d.set("vote_count", i(&v, "vote_count"));
    let gallery: Vec<String> = arr(&v["images"], "backdrops").iter().take(10).filter_map(|b| s(b, "file_path")).map(|p| backdrop(&p)).collect();
    d.set("gallery", gallery);
    credits(&v, &mut d);
    for c in arr(&v, "production_companies").iter().take(2) {
        if let Some(n) = s(c, "name") {
            d.creators.push(Creator::new(n, "studio"));
        }
    }

    if media == Media::Movie {
        d.runtime = i(&v, "runtime").filter(|r| *r > 0);
        d.tags = names(&v["keywords"], "keywords", "name");
        if let Some(imdb) = s(&v, "imdb_id") {
            d.ids.push(("imdb".into(), imdb));
        }
        if let Some(c) = v.get("belongs_to_collection").filter(|c| c.is_object()) {
            d.set("collection", json!({ "id": c["id"], "name": c["name"] }));
        }
        // Prefer the region's own date: theatrical (3), then limited (2), then digital (4).
        let dates = arr(&v["release_dates"], "results").iter().find(|r| r["iso_3166_1"] == region.as_str()).map(|r| arr(r, "release_dates")).unwrap_or(&[]);
        for wanted in [3, 2, 4] {
            if let Some(found) = dates.iter().find(|x| x["type"] == wanted).and_then(|x| s(x, "release_date")).and_then(|x| date_only(&x)) {
                d.release_date = Some(found);
                break;
            }
        }
        if let Some(digital) = dates.iter().find(|x| x["type"] == 4).and_then(|x| s(x, "release_date")).and_then(|x| date_only(&x)) {
            d.set("digital_date", digital);
        }
        if let Some(cert) = dates.iter().filter_map(|x| s(x, "certification")).next() {
            d.set("certification", cert);
        }
        if let Some(dir) = d.creators.iter().find(|c| c.role == "director").map(|c| c.name.clone()) {
            d.set("director", dir);
        }
    } else {
        d.runtime = arr(&v, "episode_run_time").first().and_then(|x| x.as_i64()).or_else(|| i(&v["last_episode_to_air"], "runtime")).filter(|r| *r > 0);
        d.tags = names(&v["keywords"], "results", "name");
        if let Some(imdb) = s(&v["external_ids"], "imdb_id") {
            d.ids.push(("imdb".into(), imdb));
        }
        for c in arr(&v, "created_by") {
            if let Some(n) = s(c, "name") {
                d.creators.push(Creator::new(n, "creator"));
            }
        }
        if let Some(n) = arr(&v, "networks").first().and_then(|n| s(n, "name")) {
            d.creators.push(Creator::new(n.clone(), "network"));
            d.set("network", n);
        }
        let seasons: Vec<Value> = arr(&v, "seasons")
            .iter()
            .filter(|x| i(x, "season_number").unwrap_or(0) > 0)
            .map(|x| json!({ "number": x["season_number"], "name": x["name"], "episodes": x["episode_count"], "air_date": x["air_date"], "poster": s(x, "poster_path").map(|p| poster(&p)) }))
            .collect();
        d.set("seasons_count", seasons.len() as i64);
        d.set("seasons", seasons);
        d.set("in_production", v["in_production"].as_bool());
        let ep = |e: &Value| -> Value {
            if e.is_object() {
                json!({ "air_date": e["air_date"], "season": e["season_number"], "episode": e["episode_number"], "name": e["name"], "finale": e["episode_type"] == "finale" })
            } else {
                Value::Null
            }
        };
        d.set("next_episode", ep(&v["next_episode_to_air"]));
        d.set("last_episode", ep(&v["last_episode_to_air"]));
        if let Some(cert) = arr(&v["content_ratings"], "results").iter().find(|r| r["iso_3166_1"] == region.as_str()).and_then(|r| s(r, "rating")) {
            d.set("certification", cert);
        }
        d.ttl_secs = 86_400; // next-episode data goes stale quickly
    }

    let mut seen = std::collections::HashSet::new();
    for key in ["recommendations", "similar"] {
        for sim in stubs(&v[key], Some(media)) {
            if d.similar.len() < 14 && seen.insert(sim.ids[0].1.clone()) {
                d.similar.push(sim);
            }
        }
    }
    Ok(d)
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Episode {
    pub season: i64,
    pub number: i64,
    pub name: String,
    pub air_date: Option<String>,
    pub rating: Option<f64>,
    pub votes: i64,
}

/// Every episode's rating for a show: the data behind the heatmap.
pub async fn episode_ratings(ctx: &Ctx, tmdb_id: &str, seasons: &[i64]) -> Result<Vec<Vec<Episode>>> {
    let mut out = vec![];
    for n in seasons.iter().take(40) {
        let v = get(ctx, &format!("/tv/{tmdb_id}/season/{n}"), &[("language", &ctx.language())], 2 * 86_400).await?;
        out.push(
            arr(&v, "episodes")
                .iter()
                .map(|e| Episode {
                    season: *n,
                    number: i(e, "episode_number").unwrap_or(0),
                    name: s(e, "name").unwrap_or_default(),
                    air_date: s(e, "air_date"),
                    votes: i(e, "vote_count").unwrap_or(0),
                    rating: f(e, "vote_average").filter(|r| *r > 0.0),
                })
                .collect(),
        );
    }
    Ok(out)
}

/// The films in a franchise (a TMDB "collection"), for following a series.
pub async fn collection(ctx: &Ctx, collection_id: &str) -> Result<Vec<ItemDraft>> {
    let v = get(ctx, &format!("/collection/{collection_id}"), &[("language", &ctx.language())], 86_400).await?;
    Ok(arr(&v, "parts").iter().filter_map(|p| stub(p, Media::Movie)).collect())
}

/// Streaming services available in the region, for "services I subscribe to".
pub async fn provider_list(ctx: &Ctx) -> Result<Vec<Value>> {
    let v = get(ctx, "/watch/providers/movie", &[("watch_region", &ctx.region()), ("language", "en-US")], 7 * 86_400).await?;
    let mut list: Vec<&Value> = arr(&v, "results").iter().collect();
    list.sort_by_key(|p| p["display_priorities"][ctx.region().as_str()].as_i64().or(p["display_priority"].as_i64()).unwrap_or(999));
    Ok(list.into_iter().take(60).map(|p| json!({ "id": p["provider_id"], "name": p["provider_name"], "logo": s(p, "logo_path").map(|l| small(&l)) })).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    const MOVIE: &str = include_str!("../../tests/fixtures/tmdb_movie.json");
    const TV: &str = include_str!("../../tests/fixtures/tmdb_tv.json");
    const DISCOVER: &str = include_str!("../../tests/fixtures/tmdb_discover_movie.json");
    const SEASON: &str = include_str!("../../tests/fixtures/tmdb_season.json");

    #[tokio::test]
    async fn movie_details_become_a_full_draft() {
        let t = ctx();
        t.net.on("/3/movie/329865", 200, MOVIE);
        let d = details(&t.ctx, Media::Movie, "329865").await.unwrap();
        assert_eq!(d.title, "Arrival");
        assert_eq!(d.detail_level, 1);
        assert_eq!(d.release_date.as_deref(), Some("2016-11-11"), "uses the US theatrical date, not the festival premiere");
        assert_eq!(d.runtime, Some(116));
        assert!(d.ids.contains(&("imdb".into(), "tt2543164".into())));
        assert!(d.creators.contains(&Creator::new("Denis Villeneuve", "director")));
        assert!(d.tags.iter().any(|k| k == "first contact"));
        assert_eq!(d.data["trailer"]["key"], "tFMo3UJ4B4g");
        assert_eq!(d.data["providers"]["stream"][0]["name"], "Paramount Plus");
        assert_eq!(d.similar.len(), 2);
        // The key is sent, and never as part of the cache-visible path.
        let sent = t.net.last("/3/movie/329865").unwrap();
        assert!(sent.url.contains("api_key=test-tmdb"));
    }

    #[tokio::test]
    async fn v4_tokens_are_sent_as_bearer() {
        let t = ctx();
        t.ctx.secrets.set("key.tmdb", "eyJhbGciOiJIUzI1NiJ9.x.y").unwrap();
        t.net.on("/3/movie/329865", 200, MOVIE);
        details(&t.ctx, Media::Movie, "329865").await.unwrap();
        let sent = t.net.last("/3/movie/329865").unwrap();
        assert!(!sent.url.contains("api_key"));
        assert!(sent.headers.iter().any(|(k, v)| k == "Authorization" && v.starts_with("Bearer eyJ")));
    }

    #[tokio::test]
    async fn tv_details_carry_next_episode_and_seasons() {
        let t = ctx();
        t.net.on("/3/tv/95396", 200, TV);
        let d = details(&t.ctx, Media::Tv, "95396").await.unwrap();
        assert_eq!(d.title, "Severance");
        assert_eq!(d.data["next_episode"]["season"], 3);
        assert_eq!(d.data["next_episode"]["episode"], 1);
        assert_eq!(d.data["seasons_count"], 2);
        assert!(d.creators.contains(&Creator::new("Dan Erickson", "creator")));
        assert!(d.creators.contains(&Creator::new("Apple TV+", "network")));
    }

    #[tokio::test]
    async fn discover_maps_genre_ids_and_stops_at_the_last_page() {
        let t = ctx();
        t.net.on("/3/discover/movie", 200, DISCOVER);
        let list = movies_between(&t.ctx, "2026-11-01", "2026-12-31", 3).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].genres, vec!["Science Fiction", "Thriller"]);
        assert_eq!(t.net.calls("/3/discover/movie"), 1);
        let sent = t.net.last("/3/discover/movie").unwrap();
        assert!(sent.url.contains("region=US") && sent.url.contains("release_date.gte=2026-11-01"));
    }

    #[tokio::test]
    async fn episode_ratings_feed_the_heatmap() {
        let t = ctx();
        t.net.on("/3/tv/95396/season/1", 200, SEASON);
        let seasons = episode_ratings(&t.ctx, "95396", &[1]).await.unwrap();
        assert_eq!(seasons[0].len(), 3);
        assert_eq!(seasons[0][2].rating, Some(9.4));
        assert_eq!(seasons[0][1].rating, None, "an unrated episode has no rating rather than 0");
    }

    #[tokio::test]
    async fn a_missing_key_explains_itself() {
        let t = ctx();
        let mut keys = t.ctx.default_keys.clone();
        keys.remove("tmdb");
        let bare = Ctx::new(t.ctx.db.clone(), t.ctx.http.clone(), t.ctx.secrets.clone(), t.host.clone(), t.dir.clone(), "0.1.0", keys);
        let err = trending(&bare, Media::Movie).await.unwrap_err();
        assert_eq!(err.kind(), "not_configured");
        assert!(err.to_string().contains("Settings"));
    }
}
