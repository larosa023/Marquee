//! The Steam store's public endpoints: price, discount, reviews and Steam Deck status.
//! None of these need a key, but the store allows only about 200 requests per 5 minutes,
//! so the scheduler keeps this host slow and the results are cached for hours.

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::util::*;
use serde_json::{json, Value};

/// "Nov 5, 2026", "5 Nov, 2026", "November 2026", "Q1 2027", "2027", "Coming soon".
pub fn parse_store_date(text: &str) -> Option<String> {
    let t = text.trim().replace(',', "");
    let parts: Vec<&str> = t.split_whitespace().collect();
    let is_year = |y: &str| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit());
    // "November 2026": a month with no day. Checked first so it isn't read as "November 20, 26".
    if let [month, year] = parts.as_slice() {
        if is_year(year) && month.chars().all(|c| c.is_alphabetic()) {
            for fmt in ["%d %b %Y", "%d %B %Y"] {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(&format!("28 {month} {year}"), fmt) {
                    return Some(d.format("%Y-%m-%d").to_string());
                }
            }
        }
    }
    if parts.len() == 3 && parts.iter().any(|p| is_year(p)) {
        for fmt in ["%b %d %Y", "%d %b %Y", "%B %d %Y", "%d %B %Y"] {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&t, fmt) {
                return Some(d.format("%Y-%m-%d").to_string());
            }
        }
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    if let [q, year] = parts.as_slice() {
        if let (Some(n), Ok(y)) = (q.strip_prefix('Q').and_then(|n| n.parse::<u32>().ok()), year.parse::<i32>()) {
            if (1..=4).contains(&n) {
                return chrono::NaiveDate::from_ymd_opt(y, n * 3, if n == 1 || n == 4 { 31 } else { 30 }).map(|d| d.format("%Y-%m-%d").to_string());
            }
        }
    }
    if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("{t}-12-31"));
    }
    None
}

const ASSETS: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";

/// What Steam's store knows about an app at a glance: its name and real artwork addresses.
/// Newer apps keep art under hashed folders, so `apps/<id>/library_600x900.jpg` 404s for them;
/// only the store's asset list has the right path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoreItem {
    pub name: Option<String>,
    pub poster: Option<String>,
    pub header: Option<String>,
    pub hero: Option<String>,
    pub release_date: Option<String>,
    pub coming_soon: Option<bool>,
    pub release_text: Option<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub overview: Option<String>,
}

fn asset(a: &Value, format: &str, key: &str) -> Option<String> {
    let file = s(a, key)?;
    Some(format!("{ASSETS}{}", format.replace("${FILENAME}", &file)))
}

/// Looks up names and artwork for many apps at once (100 per call) through the store's
/// IStoreBrowseService/GetItems. Missing apps are simply absent from the map.
pub async fn items(ctx: &Ctx, appids: &[String]) -> Result<std::collections::HashMap<String, StoreItem>> {
    let mut out = std::collections::HashMap::new();
    let cc = ctx.region().to_uppercase();
    let key = ctx.key("steam");
    for chunk in appids.chunks(100) {
        let ids: Vec<Value> = chunk.iter().filter_map(|a| a.parse::<u64>().ok()).map(|a| json!({ "appid": a })).collect();
        let input = json!({
            "ids": ids,
            "context": { "language": "english", "country_code": cc },
            "data_request": { "include_assets": true, "include_release": true, "include_basic_info": true }
        });
        let input = input.to_string();
        let mut q = vec![("input_json", input.as_str())];
        if let Some(k) = &key {
            q.push(("key", k.as_str()));
        }
        let v: Value = ctx.http.get("Steam store", "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/").query(&q).ttl(24 * 3600).json().await?;
        for it in arr(&v["response"], "store_items") {
            let Some(appid) = s(it, "appid").or_else(|| s(it, "id")) else { continue };
            if it["success"].as_i64().unwrap_or(1) != 1 {
                continue;
            }
            let a = &it["assets"];
            let format = s(a, "asset_url_format").unwrap_or_else(|| format!("steam/apps/{appid}/${{FILENAME}}"));
            let rel = &it["release"];
            let date = i(rel, "steam_release_date").or_else(|| i(rel, "original_release_date")).filter(|t| *t > 0).and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|d| d.format("%Y-%m-%d").to_string());
            let info = &it["basic_info"];
            out.insert(
                appid,
                StoreItem {
                    name: s(it, "name"),
                    poster: asset(a, &format, "library_capsule_2x").or_else(|| asset(a, &format, "library_capsule")),
                    header: asset(a, &format, "header"),
                    hero: asset(a, &format, "library_hero").or_else(|| asset(a, &format, "page_background")),
                    release_date: date,
                    coming_soon: rel["is_coming_soon"].as_bool(),
                    release_text: s(rel, "custom_release_date_message").or_else(|| s(rel, "coming_soon_display")),
                    developers: arr(info, "developers").iter().filter_map(|d| s(d, "name")).collect(),
                    publishers: arr(info, "publishers").iter().filter_map(|d| s(d, "name")).collect(),
                    overview: s(info, "short_description"),
                },
            );
        }
    }
    Ok(out)
}

/// Fills a game draft from a store item (name, art, date, studio) where the draft lacks them.
pub fn apply(d: &mut ItemDraft, it: &StoreItem) {
    if let Some(n) = &it.name {
        d.title = n.clone();
    }
    if it.poster.is_some() {
        d.poster = it.poster.clone();
    }
    if let Some(h) = &it.header {
        d.set("header", h.clone());
    }
    if d.backdrop.is_none() {
        d.backdrop = it.hero.clone().or_else(|| it.header.clone());
    }
    if d.release_date.is_none() {
        d.release_date = it.release_date.clone();
    }
    if let Some(c) = it.coming_soon {
        d.set("coming_soon", c);
    }
    if let Some(t) = &it.release_text {
        d.set("release_text", t.clone());
    }
    if d.overview.is_none() {
        d.overview = it.overview.clone();
    }
    if !d.data.contains_key("developer") {
        if let Some(dev) = it.developers.first() {
            d.set("developer", dev.clone());
        }
    }
    if d.creators.is_empty() {
        d.creators.extend(it.developers.iter().map(|n| Creator::new(n, "developer")));
        d.creators.extend(it.publishers.iter().map(|n| Creator::new(n, "publisher")));
    }
}

/// Store details for one app. Returns `None` for delisted or region-locked apps.
pub async fn app_details(ctx: &Ctx, appid: &str) -> Result<Option<ItemDraft>> {
    let cc = ctx.region().to_lowercase();
    let v: Value = ctx.http.get("Steam store", "https://store.steampowered.com/api/appdetails").query(&[("appids", appid), ("cc", &cc), ("l", "english")]).ttl(6 * 3600).json().await?;
    let entry = &v[appid];
    if entry["success"] != true {
        return Ok(None);
    }
    let a = &entry["data"];
    let Some(name) = s(a, "name") else { return Ok(None) };
    let mut d = ItemDraft::new(Media::Game, name).id("steam", appid);
    d.detail_level = 1;
    d.tag_source = TagSource::Steam;
    d.overview = s(a, "short_description");
    d.backdrop = s(a, "background_raw").or_else(|| s(a, "header_image"));
    // appdetails has no portrait art; the asset list does (left unset if it can't be reached, so a
    // poster already saved isn't replaced by a guess).
    if let Ok(found) = items(ctx, &[appid.to_string()]).await {
        if let Some(it) = found.get(appid) {
            d.poster = it.poster.clone();
        }
    }
    d.set("header", s(a, "header_image"));
    d.genres = names(a, "genres", "description");
    d.tags = names(a, "categories", "description");
    for dev in arr(a, "developers").iter().filter_map(|x| x.as_str()) {
        d.creators.push(Creator::new(dev, "developer"));
        if !d.data.contains_key("developer") {
            d.set("developer", dev);
        }
    }
    for p in arr(a, "publishers").iter().filter_map(|x| x.as_str()) {
        d.creators.push(Creator::new(p, "publisher"));
    }
    let rd = &a["release_date"];
    d.release_date = s(rd, "date").and_then(|t| parse_store_date(&t));
    d.set("coming_soon", rd["coming_soon"].as_bool());
    d.set("release_text", s(rd, "date"));
    d.rating = f(&a["metacritic"], "score").map(|m| m / 10.0);
    d.popularity = f(&a["recommendations"], "total").map(|r| (r.sqrt() / 4.0).min(300.0)).unwrap_or(0.0);
    let mut platforms = vec!["PC"];
    if a["platforms"]["mac"] == true {
        platforms.push("Mac");
    }
    if a["platforms"]["linux"] == true {
        platforms.push("Linux");
    }
    d.set("platforms", json!(platforms));
    if a["is_free"] == true {
        d.set("price", json!({ "cents": 0, "initial": 0, "currency": "USD", "text": "Free" }));
        d.set("discount", 0);
    } else if let Some(p) = a.get("price_overview").filter(|p| p.is_object()) {
        d.set("price", json!({ "cents": p["final"], "initial": p["initial"], "currency": p["currency"], "text": p["final_formatted"] }));
        d.set("discount", i(p, "discount_percent").unwrap_or(0));
    }
    let gallery: Vec<String> = arr(a, "screenshots").iter().take(10).filter_map(|x| s(x, "path_full")).collect();
    d.set("gallery", gallery);
    if let Some(m) = arr(a, "movies").first() {
        let src = s(&m["mp4"], "max").or_else(|| s(&m["webm"], "max")).or_else(|| s(m, "hls_h264"));
        d.set("trailer", json!({ "site": "steam", "url": src, "name": m["name"], "thumb": m["thumbnail"] }));
    }
    d.set("store_url", format!("https://store.steampowered.com/app/{appid}"));
    d.ttl_secs = 6 * 3600;
    Ok(Some(d))
}

/// "verified" | "playable" | "unsupported" | "unknown".
pub async fn deck_status(ctx: &Ctx, appid: &str) -> Result<&'static str> {
    let v: Value = ctx.http.get("Steam store", "https://store.steampowered.com/saleaction/ajaxgetdeckappcompatibilityreport").query(&[("nAppID", appid), ("l", "english")]).ttl(7 * 86_400).json().await?;
    Ok(match i(&v["results"], "resolved_category") {
        Some(3) => "verified",
        Some(2) => "playable",
        Some(1) => "unsupported",
        _ => "unknown",
    })
}

/// Review summary: ("Very Positive", percent positive, total reviews).
pub async fn reviews(ctx: &Ctx, appid: &str) -> Result<Option<Value>> {
    let v: Value = ctx.http.get("Steam store", format!("https://store.steampowered.com/appreviews/{appid}")).query(&[("json", "1"), ("num_per_page", "0"), ("language", "all"), ("purchase_type", "all")]).ttl(86_400).json().await?;
    let q = &v["query_summary"];
    let total = i(q, "total_reviews").unwrap_or(0);
    if total == 0 {
        return Ok(None);
    }
    let pct = (i(q, "total_positive").unwrap_or(0) as f64 / total as f64 * 100.0).round() as i64;
    Ok(Some(json!({ "label": q["review_score_desc"], "percent": pct, "total": total })))
}

/// All-time low from IsThereAnyDeal (optional key). Steam itself doesn't publish price history.
pub async fn itad_low(ctx: &Ctx, appid: &str) -> Result<Option<Value>> {
    let Some(key) = ctx.key("itad") else { return Ok(None) };
    let found: Value = ctx.http.get("IsThereAnyDeal", "https://api.isthereanydeal.com/games/lookup/v1").query(&[("key", &key), ("appid", appid)]).ttl(30 * 86_400).json().await?;
    let Some(id) = s(&found["game"], "id") else { return Ok(None) };
    let lows: Value = ctx.http.post("IsThereAnyDeal", "https://api.isthereanydeal.com/games/historylow/v1").query(&[("key", &key), ("country", &ctx.region())]).json_body(&json!([id])).ttl(86_400).json().await?;
    let low = &lows[0]["low"];
    if low.is_null() {
        return Ok(None);
    }
    Ok(Some(json!({
        "cents": i(&low["price"], "amountInt"),
        "currency": low["price"]["currency"],
        "cut": low["cut"],
        "shop": low["shop"]["name"],
        "at": s(low, "timestamp").and_then(|t| date_only(&t)),
        "source": "IsThereAnyDeal",
    })))
}

/// Records today's price and returns the lowest price Marquee has seen for this game.
pub fn track_price(conn: &rusqlite::Connection, item_id: i64, cents: i64, discount: i64, currency: &str) -> Result<Value> {
    conn.execute("INSERT OR REPLACE INTO price_history (item_id, at, cents, discount, currency) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![item_id, today(), cents, discount, currency])?;
    let (low, at): (i64, String) = conn.query_row("SELECT cents, at FROM price_history WHERE item_id = ?1 ORDER BY cents, at LIMIT 1", [item_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(json!({ "cents": low, "currency": currency, "at": at, "source": "Marquee" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    const APP: &str = include_str!("../../tests/fixtures/steam_appdetails.json");

    #[test]
    fn store_dates_in_every_shape() {
        assert_eq!(parse_store_date("Nov 5, 2026").as_deref(), Some("2026-11-05"));
        assert_eq!(parse_store_date("5 Nov, 2026").as_deref(), Some("2026-11-05"));
        assert_eq!(parse_store_date("November 2026").as_deref(), Some("2026-11-28"));
        assert_eq!(parse_store_date("Q1 2027").as_deref(), Some("2027-03-31"));
        assert_eq!(parse_store_date("Q2 2027").as_deref(), Some("2027-06-30"));
        assert_eq!(parse_store_date("2027").as_deref(), Some("2027-12-31"));
        assert_eq!(parse_store_date("Coming soon"), None);
        assert_eq!(parse_store_date("To be announced"), None);
    }

    #[tokio::test]
    async fn store_items_give_names_and_the_real_hashed_artwork() {
        let t = ctx();
        t.net.on(
            "IStoreBrowseService/GetItems",
            200,
            r#"{"response":{"store_items":[
                {"item_type":0,"id":4928450,"success":1,"name":"Real Players","appid":4928450,
                 "basic_info":{"short_description":"A game.","developers":[{"name":"REAL PLAYERS"}],"publishers":[{"name":"Pub"}]},
                 "release":{"steam_release_date":1763596800,"is_coming_soon":true},
                 "assets":{"asset_url_format":"steam/apps/4928450/9f3c1a/${FILENAME}?t=1","header":"header.jpg","library_capsule":"library_600x900.jpg","library_capsule_2x":"library_600x900_2x.jpg","library_hero":"library_hero.jpg"}},
                {"item_type":0,"id":1,"success":2}
            ]}}"#,
        );
        let found = items(&t.ctx, &["4928450".into(), "1".into()]).await.unwrap();
        let it = &found["4928450"];
        assert_eq!(it.name.as_deref(), Some("Real Players"));
        assert_eq!(it.poster.as_deref(), Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/4928450/9f3c1a/library_600x900_2x.jpg?t=1"));
        assert_eq!(it.release_date.as_deref(), Some("2025-11-20"));
        assert!(!found.contains_key("1"), "a failed lookup is left out");
        let mut d = ItemDraft::new(Media::Game, "Steam app 4928450");
        apply(&mut d, it);
        assert_eq!(d.title, "Real Players");
        assert_eq!(d.data["developer"], "REAL PLAYERS");
    }

    #[tokio::test]
    async fn app_details_read_price_and_discount() {
        let t = ctx();
        t.net.on("store.steampowered.com/api/appdetails", 200, APP);
        let d = app_details(&t.ctx, "753640").await.unwrap().unwrap();
        assert_eq!(d.title, "Outer Wilds");
        assert_eq!(d.data["price"]["cents"], 1499);
        assert_eq!(d.data["price"]["initial"], 2499);
        assert_eq!(d.data["discount"], 40);
        assert_eq!(d.release_date.as_deref(), Some("2020-06-18"));
        assert!(d.creators.contains(&Creator::new("Mobius Digital", "developer")));
        assert!(t.net.last("appdetails").unwrap().url.contains("cc=us"));
    }

    #[tokio::test]
    async fn a_delisted_app_is_none_not_an_error() {
        let t = ctx();
        t.net.on("store.steampowered.com/api/appdetails", 200, r#"{"12345":{"success":false}}"#);
        assert!(app_details(&t.ctx, "12345").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn deck_and_reviews() {
        let t = ctx();
        t.net.on("ajaxgetdeckappcompatibilityreport", 200, r#"{"success":1,"results":{"appid":753640,"resolved_category":3,"resolved_items":[]}}"#);
        t.net.on("appreviews/753640", 200, r#"{"success":1,"query_summary":{"review_score":9,"review_score_desc":"Overwhelmingly Positive","total_positive":95000,"total_negative":5000,"total_reviews":100000}}"#);
        assert_eq!(deck_status(&t.ctx, "753640").await.unwrap(), "verified");
        let r = reviews(&t.ctx, "753640").await.unwrap().unwrap();
        assert_eq!(r["percent"], 95);
    }

    #[test]
    fn the_local_low_is_the_cheapest_price_seen() {
        let db = crate::db::Db::memory().unwrap();
        let conn = db.lock();
        let id = crate::store::upsert_item(&conn, &ItemDraft::new(Media::Game, "X").id("steam", 1)).unwrap();
        conn.execute("INSERT INTO price_history (item_id, at, cents, discount) VALUES (?1, '2026-01-01', 999, 60)", [id]).unwrap();
        let low = track_price(&conn, id, 2499, 0, "USD").unwrap();
        assert_eq!(low["cents"], 999);
        assert_eq!(low["at"], "2026-01-01");
    }
}
