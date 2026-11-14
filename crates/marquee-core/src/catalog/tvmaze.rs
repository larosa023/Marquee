//! TVmaze (api.tvmaze.com): free TV listings with no key. Marquee uses it so TV works out of
//! the box: what premieres in the coming weeks (network and streaming), show details and
//! posters for shows from Trakt or Letterboxd history, next-episode dates, and per-episode
//! ratings for the heatmap. TMDB, when a key is set, remains the main source.
//!
//! Data and images are CC BY-SA; the credit is in Settings → About.

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::{arr, i, s};
use serde_json::{json, Value};
use std::collections::HashMap;

const API: &str = "https://api.tvmaze.com";
pub const ATTRIBUTION: &str = "TV listings, posters and episode ratings from TVmaze (tvmaze.com), licensed CC BY-SA.";

/// Show types that are listings noise rather than shows people follow for new seasons.
const SKIP_TYPES: [&str; 6] = ["News", "Talk Show", "Sports", "Game Show", "Award Show", "Panel Show"];

fn lang_code(name: &str) -> Option<&'static str> {
    Some(match name {
        "English" => "en",
        "Japanese" => "ja",
        "Korean" => "ko",
        "Chinese" => "zh",
        "Spanish" => "es",
        "French" => "fr",
        "German" => "de",
        "Italian" => "it",
        "Portuguese" => "pt",
        "Hindi" => "hi",
        "Swedish" => "sv",
        "Danish" => "da",
        "Norwegian" => "no",
        "Finnish" => "fi",
        "Dutch" => "nl",
        "Polish" => "pl",
        "Russian" => "ru",
        "Turkish" => "tr",
        "Thai" => "th",
        "Arabic" => "ar",
        "Hebrew" => "he",
        _ => return None,
    })
}

/// TVmaze summaries are HTML (`<p>…</p>`); Marquee shows plain text.
pub fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn get(ctx: &Ctx, path: &str, ttl: i64) -> Result<Value> {
    ctx.http.get("TVmaze", format!("{API}{path}")).ttl(ttl).json().await
}

fn episode(ep: &Value) -> Value {
    json!({
        "air_date": s(ep, "airdate").filter(|d| !d.is_empty()),
        "season": i(ep, "season"),
        "episode": i(ep, "number"),
        "name": s(ep, "name"),
    })
}

/// A show as a Marquee item.
pub fn show_draft(v: &Value) -> Option<ItemDraft> {
    let id = i(v, "id")?;
    let mut d = ItemDraft::new(Media::Tv, s(v, "name")?).id("tvmaze", id);
    let ext = &v["externals"];
    if let Some(imdb) = s(ext, "imdb") {
        d.ids.push(("imdb".into(), imdb));
    }
    if let Some(tvdb) = i(ext, "thetvdb") {
        d.ids.push(("tvdb".into(), tvdb.to_string()));
    }
    d.release_date = s(v, "premiered");
    d.poster = s(&v["image"], "original").or_else(|| s(&v["image"], "medium"));
    d.overview = s(v, "summary").map(|h| plain(&h)).filter(|t| !t.is_empty());
    d.runtime = i(v, "averageRuntime").or_else(|| i(v, "runtime"));
    d.popularity = v["weight"].as_f64().unwrap_or(0.0);
    d.rating = v["rating"]["average"].as_f64();
    d.genres = arr(v, "genres").iter().filter_map(|g| g.as_str().map(str::to_string)).collect();
    if s(v, "type").as_deref() == Some("Animation") && !d.genres.iter().any(|g| g == "Animation") {
        d.genres.push("Animation".into());
    }
    d.language = s(v, "language").and_then(|l| lang_code(&l)).map(str::to_string);
    if let Some(l) = &d.language {
        d.set("language", l.clone());
    }
    let channel = if v["network"].is_object() { &v["network"] } else { &v["webChannel"] };
    if let Some(n) = s(channel, "name") {
        d.set("network", n);
    }
    if let Some(c) = s(&channel["country"], "code") {
        d.set("origin_country", json!([c]));
    }
    if let Some(st) = s(v, "status") {
        // TVmaze says "To Be Determined" where TMDB says "Returning Series"; both read fine.
        d.set("status", st);
    }
    if let Some(u) = s(v, "url") {
        d.set("tvmaze_url", u);
    }
    if let Some(u) = s(v, "officialSite") {
        d.set("homepage", u);
    }
    d.ttl_secs = 86_400;
    Some(d)
}

/// Season and series premieres between two dates, on networks in the user's country and on
/// streaming services, plus brand-new shows. Each show once, at its premiere.
pub async fn premieres(ctx: &Ctx, from: i64, to: i64) -> Result<Vec<ItemDraft>> {
    let mut found: HashMap<i64, ItemDraft> = HashMap::new();
    let country = ctx.region().to_uppercase();
    let new_since = days_from_today(from - 45);
    let mut failures = 0;
    let mut successes = 0;
    for day in from..=to {
        // If TVmaze is answering nothing but errors, stop rather than spend a minute on it.
        if successes == 0 && failures >= 4 {
            return Err(Error::msg("TVmaze isn't answering right now."));
        }
        let date = days_from_today(day);
        let ttl = if day < 0 { 3 * 86_400 } else { 6 * 3600 };
        let web = get(ctx, &format!("/schedule/web?date={date}"), ttl).await;
        let local = get(ctx, &format!("/schedule?country={country}&date={date}"), ttl).await;
        // One failed day doesn't sink the rest, but going offline does.
        for r in [web, local] {
            let v = match r {
                Ok(v) => {
                    successes += 1;
                    v
                }
                Err(e) if e.is_offline() => return Err(e),
                Err(e) => {
                    failures += 1;
                    tracing::debug!("TVmaze schedule {date}: {e}");
                    continue;
                }
            };
            for ep in v.as_array().into_iter().flatten() {
                let show = if ep["_embedded"]["show"].is_object() { &ep["_embedded"]["show"] } else { &ep["show"] };
                if SKIP_TYPES.contains(&show["type"].as_str().unwrap_or("")) {
                    continue;
                }
                let number = i(ep, "number");
                let brand_new = s(show, "premiered").map(|p| p >= new_since).unwrap_or(false);
                if number != Some(1) && !brand_new {
                    continue;
                }
                let Some(mut d) = show_draft(show) else { continue };
                let air = s(ep, "airdate").unwrap_or_else(|| date.clone());
                if number == Some(1) {
                    d.set("season_start", air.clone());
                    d.set("season_number", i(ep, "season").unwrap_or(1));
                }
                if air >= today() {
                    d.set("next_episode", episode(ep));
                }
                d.set("airing", true);
                let id = i(show, "id").unwrap_or(0);
                // Keep the earliest premiere seen for each show.
                let keep = found.get(&id).and_then(|x| x.data.get("season_start")).and_then(|x| x.as_str()).map(|have| air.as_str() < have).unwrap_or(true);
                if keep || !found.contains_key(&id) {
                    found.insert(id, d);
                }
            }
        }
    }
    Ok(found.into_values().collect())
}

/// Full details for one show: cast, next episode and every episode's rating.
pub async fn details(ctx: &Ctx, tvmaze_id: &str) -> Result<ItemDraft> {
    let v = get(ctx, &format!("/shows/{tvmaze_id}?embed[]=nextepisode&embed[]=episodes&embed[]=cast"), 86_400).await?;
    let mut d = show_draft(&v).ok_or_else(|| Error::msg("TVmaze returned a show without a name."))?;
    let emb = &v["_embedded"];
    d.creators = arr(emb, "cast").iter().take(8).filter_map(|c| s(&c["person"], "name")).map(|n| Creator::new(&n, "cast")).collect();
    if emb["nextepisode"].is_object() {
        d.set("next_episode", episode(&emb["nextepisode"]));
    }
    let eps: Vec<Value> = arr(emb, "episodes")
        .iter()
        .filter(|e| i(e, "number").is_some())
        .map(|e| json!({ "s": i(e, "season"), "e": i(e, "number"), "name": s(e, "name"), "air": s(e, "airdate").filter(|a| !a.is_empty()), "rating": e["rating"]["average"].as_f64() }))
        .collect();
    let mut seasons: Vec<i64> = eps.iter().filter_map(|e| e["s"].as_i64()).collect();
    seasons.dedup();
    if let Some(last) = seasons.last() {
        // The last episode of each finished season is its finale.
        if let Some(next) = d.data.get_mut("next_episode") {
            let (ns, ne) = (next["season"].as_i64(), next["episode"].as_i64());
            let season_len = eps.iter().filter(|e| e["s"].as_i64() == ns).count() as i64;
            if ns.is_some() && ne == Some(season_len) && season_len > 1 {
                next["finale"] = json!(true);
            }
        }
        d.set("seasons_count", *last);
    }
    d.set("seasons", json!(seasons.iter().map(|n| json!({ "number": n, "name": format!("Season {n}"), "episodes": eps.iter().filter(|e| e["s"].as_i64() == Some(*n)).count() })).collect::<Vec<_>>()));
    d.set("tvmaze_episodes", json!(eps));
    d.detail_level = 1;
    Ok(d)
}

/// Finds a show on TVmaze by IMDb or TheTVDB id, or by title (and year, when known).
pub async fn find(ctx: &Ctx, imdb: Option<&str>, tvdb: Option<&str>, title: &str, year: Option<i32>) -> Result<Option<String>> {
    for (key, id) in [("imdb", imdb), ("thetvdb", tvdb)] {
        if let Some(id) = id {
            match get(ctx, &format!("/lookup/shows?{key}={id}"), 7 * 86_400).await {
                Ok(v) => {
                    if let Some(found) = i(&v, "id") {
                        return Ok(Some(found.to_string()));
                    }
                }
                Err(Error::Api { status: 404, .. }) => {}
                Err(e) => return Err(e),
            }
        }
    }
    let q = urlencoding(title);
    let hits = get(ctx, &format!("/search/shows?q={q}"), 7 * 86_400).await?;
    let best = hits.as_array().into_iter().flatten().map(|h| &h["show"]).find(|show| match (year, s(show, "premiered").and_then(|p| p.get(..4).and_then(|y| y.parse::<i32>().ok()))) {
        (Some(want), Some(got)) => (want - got).abs() <= 1,
        _ => true,
    });
    Ok(best.and_then(|show| i(show, "id")).map(|id| id.to_string()))
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Episodes stored by `details`, as heatmap rows (season by season).
pub fn heatmap(data: &Value) -> Vec<Vec<crate::catalog::tmdb::Episode>> {
    let mut by: std::collections::BTreeMap<i64, Vec<crate::catalog::tmdb::Episode>> = Default::default();
    for e in data["tvmaze_episodes"].as_array().into_iter().flatten() {
        let (Some(season), Some(number)) = (e["s"].as_i64(), e["e"].as_i64()) else { continue };
        if season <= 0 {
            continue;
        }
        by.entry(season).or_default().push(crate::catalog::tmdb::Episode { season, number, name: e["name"].as_str().unwrap_or("").to_string(), air_date: e["air"].as_str().map(str::to_string), rating: e["rating"].as_f64(), votes: 0 });
    }
    by.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    fn show(id: i64, name: &str, premiered: &str, kind: &str, lang: &str) -> Value {
        json!({ "id": id, "name": name, "type": kind, "language": lang, "genres": ["Drama"], "premiered": premiered, "weight": 90,
                "image": { "medium": "https://static.tvmaze.com/m.jpg", "original": "https://static.tvmaze.com/o.jpg" },
                "summary": "<p>A <b>great</b> show &amp; more.</p>", "rating": { "average": 8.4 },
                "externals": { "imdb": "tt123", "thetvdb": 42 }, "network": { "name": "HBO", "country": { "code": "US" } }, "status": "Running" })
    }

    #[test]
    fn shows_become_items_with_plain_text_and_ids() {
        let d = show_draft(&show(1, "Severance", "2022-02-18", "Scripted", "English")).unwrap();
        assert_eq!(d.overview.as_deref(), Some("A great show & more."));
        assert_eq!(d.poster.as_deref(), Some("https://static.tvmaze.com/o.jpg"));
        assert!(d.ids.contains(&("imdb".into(), "tt123".into())) && d.ids.contains(&("tvdb".into(), "42".into())));
        assert_eq!(d.data["network"], "HBO");
        assert_eq!(d.language.as_deref(), Some("en"));
        let anime = show_draft(&show(2, "Frieren", "2023-09-29", "Animation", "Japanese")).unwrap();
        assert!(anime.genres.contains(&"Animation".to_string()));
        assert_eq!(anime.language.as_deref(), Some("ja"));
    }

    #[tokio::test]
    async fn premieres_keep_season_starts_and_new_shows_and_skip_talk_shows() {
        let t = ctx();
        let date = days_from_today(0);
        let eps = json!([
            { "airdate": date, "season": 3, "number": 1, "name": "Return", "show": show(1, "Old Favourite", "2015-01-01", "Scripted", "English") },
            { "airdate": date, "season": 5, "number": 4, "name": "Midseason", "show": show(2, "Ongoing", "2012-01-01", "Scripted", "English") },
            { "airdate": date, "season": 1, "number": 1, "name": "Tonight", "show": show(3, "Late Show", "2015-09-08", "Talk Show", "English") },
        ]);
        let web = json!([{ "airdate": date, "season": 1, "number": 2, "name": "Pilot 2", "_embedded": { "show": show(4, "Brand New", &days_from_today(-3), "Scripted", "English") } }]);
        t.net.on(&format!("schedule/web?date={date}"), 200, &web.to_string());
        t.net.on(&format!("schedule?country=US&date={date}"), 200, &eps.to_string());
        let got = premieres(&t.ctx, 0, 0).await.unwrap();
        let mut titles: Vec<&str> = got.iter().map(|d| d.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["Brand New", "Old Favourite"]);
        let fav = got.iter().find(|d| d.title == "Old Favourite").unwrap();
        assert_eq!(fav.data["season_start"], date);
        assert_eq!(fav.data["next_episode"]["season"], 3);
    }

    #[tokio::test]
    async fn details_bring_cast_next_episode_finale_and_episode_ratings() {
        let t = ctx();
        let mut v = show(1, "Severance", "2022-02-18", "Scripted", "English");
        v["_embedded"] = json!({
            "cast": [{ "person": { "name": "Adam Scott" } }],
            "nextepisode": { "airdate": "2030-01-10", "season": 2, "number": 3, "name": "Finale" },
            "episodes": [
                { "season": 1, "number": 1, "name": "Good News", "airdate": "2022-02-18", "rating": { "average": 8.1 } },
                { "season": 1, "number": 2, "name": "Half Loop", "airdate": "2022-02-18", "rating": { "average": 8.0 } },
                { "season": 2, "number": 1, "name": "Hello", "airdate": "2029-12-27", "rating": { "average": null } },
                { "season": 2, "number": 2, "name": "Goodbye", "airdate": "2030-01-03", "rating": { "average": null } },
                { "season": 2, "number": 3, "name": "Finale", "airdate": "2030-01-10", "rating": { "average": null } }
            ]
        });
        t.net.on("shows/1?embed", 200, &v.to_string());
        let d = details(&t.ctx, "1").await.unwrap();
        assert_eq!(d.creators[0].name, "Adam Scott");
        assert_eq!(d.data["next_episode"]["finale"], true, "the last episode of the season");
        assert_eq!(d.data["seasons_count"], 2);
        let rows = heatmap(&Value::Object(d.data.clone()));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].rating, Some(8.1));
    }

    #[tokio::test]
    async fn find_tries_ids_then_title_and_year() {
        let t = ctx();
        t.net.on("lookup/shows?imdb=tt999", 404, "{}");
        t.net.on("search/shows?q=The+Bear", 200, &json!([{ "show": { "id": 5, "name": "The Bear", "premiered": "1999-01-01" } }, { "show": { "id": 6, "name": "The Bear", "premiered": "2022-06-23" } }]).to_string());
        assert_eq!(find(&t.ctx, Some("tt999"), None, "The Bear", Some(2022)).await.unwrap().as_deref(), Some("6"));
    }
}
