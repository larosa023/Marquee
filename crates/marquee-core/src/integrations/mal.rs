//! MyAnimeList: your anime list (scores, statuses, plan to watch) and seasonal anime as new
//! releases; plus updating your list (plan to watch, completed, score).
//! https://myanimelist.net/apiconfig/references/api/v2
//!
//! API notes this client follows:
//! * public data (any public list, seasonal charts, search) needs only the app's client ID in the
//!   `X-MAL-CLIENT-ID` header, so connecting with just a username reads your list
//! * changing your list needs OAuth 2 with PKCE, and MAL only supports the `plain` challenge
//!   method (the challenge equals the verifier, 43-128 characters)
//! * apps registered as type "other" have no client secret; a "web" app's secret is sent if set
//! * the rate limit is unpublished; one request a second is the usual safe pace
//!
//! MAL lists every season (and part, and cour) as its own entry, while TMDB and Trakt treat a
//! show as one title with seasons. Sync folds a franchise's TV entries into one show
//! ("Attack on Titan Season 3 Part 2" counts toward "Attack on Titan") so it matches the rest of
//! the catalog; movies stay separate.

use super::{cfg, item_for_write, oauth, set_cfg, Source, Synced, WriteOp, WriteOutcome};
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use crate::util::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

const API: &str = "https://api.myanimelist.net/v2";
const AUTH: &str = "https://myanimelist.net/v1/oauth2";
pub const LIST_FIELDS: &str = "list_status,media_type,start_date,num_episodes,genres,studios,mean,alternative_titles,main_picture,synopsis,num_list_users";
const ANIME_FIELDS: &str = "media_type,start_date,num_episodes,genres,studios,mean,alternative_titles,main_picture,synopsis,num_list_users,average_episode_duration,status";

/// MAL "genres" that describe the audience or content rating rather than taste.
const NOT_TASTE: &[&str] = &["award winning", "adult cast", "ecchi", "erotica", "hentai", "medical", "showbiz", "educational", "magical sex shift", "crossdressing", "kids"];

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{}/mal", oauth::PORT)
}

pub fn anime_url(id: &str) -> String {
    format!("https://myanimelist.net/anime/{id}")
}

pub struct Mal;

fn call<'a>(ctx: &'a Ctx, method: &str, url: String, client_id: &str) -> crate::net::Call<'a> {
    ctx.http.request("MyAnimeList", method, url).header("X-MAL-CLIENT-ID", client_id).header("User-Agent", ctx.user_agent())
}

fn explain(e: Error) -> Error {
    match e {
        Error::Api { status: 403, .. } => {
            Error::msg("MyAnimeList refused the request. Check that the client ID in Settings → API keys is right, and that your anime list is public (MyAnimeList → Settings → Privacy) if you connected with a username.")
        }
        Error::Api { status: 404, .. } => Error::msg("MyAnimeList doesn't know that username."),
        other => other,
    }
}

// ------------------------------------------------------------------ sign-in

/// Connects with just a username: reads a public list, can't change it.
pub async fn connect(ctx: &Ctx, username: &str) -> Result<String> {
    let username = username.trim().trim_start_matches('@');
    if username.is_empty() {
        return Err(Error::msg("Type your MyAnimeList username."));
    }
    let id = ctx.require_key("mal_client_id")?;
    call(ctx, "GET", format!("{API}/users/{}/animelist", urlencoding::encode(username)), &id).query(&[("limit", "1")]).send().await.map_err(explain)?;
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "mal", json!({ "username": username }))?;
    store::set_source(&conn, "mal", "connected", Some(username), None)?;
    Ok(username.to_string())
}

pub fn authorize_url(client_id: &str, verifier: &str, state: &str) -> String {
    // MAL supports only the plain method: the challenge is the verifier itself.
    let params = [("response_type", "code"), ("client_id", client_id), ("state", state), ("redirect_uri", &redirect_uri()), ("code_challenge", verifier), ("code_challenge_method", "plain")];
    format!("{AUTH}/authorize?{}", params.iter().map(|(k, v)| format!("{k}={}", urlencoding::encode(v))).collect::<Vec<_>>().join("&"))
}

/// Signs in on MyAnimeList so Marquee can update your list.
pub async fn sign_in(ctx: &Ctx) -> Result<String> {
    let client_id = ctx.require_key("mal_client_id")?;
    let verifier = oauth::random_string(96);
    let state = oauth::random_string(24);
    ctx.host.open_url(&authorize_url(&client_id, &verifier, &state));
    let params = oauth::wait_for_redirect(oauth::PORT, "/mal", Duration::from_secs(300)).await?;
    finish_sign_in(ctx, &params, &verifier, &state).await
}

pub async fn finish_sign_in(ctx: &Ctx, params: &HashMap<String, String>, verifier: &str, state: &str) -> Result<String> {
    if let Some(err) = params.get("error") {
        return Err(Error::msg(if err == "access_denied" { "You declined the request on MyAnimeList, so nothing was changed.".to_string() } else { format!("MyAnimeList sign-in failed: {err}") }));
    }
    if params.get("state").map(String::as_str) != Some(state) {
        return Err(Error::msg("The MyAnimeList sign-in response didn't match this request. Try again."));
    }
    let code = params.get("code").ok_or_else(|| Error::msg("MyAnimeList didn't return a sign-in code."))?;
    let v = token_request(ctx, &[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", &redirect_uri()), ("code_verifier", verifier)]).await.map_err(|e| match e {
        Error::Api { status: 400 | 401, detail, .. } if detail.contains("redirect") || detail.contains("invalid_client") => Error::NotConfigured(format!(
            "MyAnimeList rejected the sign-in. In your MAL app (myanimelist.net/apiconfig), set the App Redirect URL to exactly {} and, for an app of type \"web\", add its client secret in Settings → API keys.",
            redirect_uri()
        )),
        other => other,
    })?;
    save_tokens(ctx, &v)?;
    let id = ctx.require_key("mal_client_id")?;
    let token = ctx.secrets.get("mal.access").unwrap_or_default();
    let me: Value = call(ctx, "GET", format!("{API}/users/@me"), &id).bearer(&token).json().await?;
    let name = s(&me, "name").unwrap_or_else(|| "MyAnimeList".into());
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "mal", json!({ "username": name, "can_write": true }))?;
    store::set_source(&conn, "mal", "connected", Some(&name), None)?;
    Ok(name)
}

async fn token_request(ctx: &Ctx, extra: &[(&str, &str)]) -> Result<Value> {
    let client_id = ctx.require_key("mal_client_id")?;
    let secret = ctx.key("mal_client_secret");
    let mut form: Vec<(&str, &str)> = vec![("client_id", &client_id)];
    if let Some(sec) = secret.as_deref() {
        form.push(("client_secret", sec));
    }
    form.extend_from_slice(extra);
    ctx.http.post("MyAnimeList", format!("{AUTH}/token")).form(&form).json().await
}

fn save_tokens(ctx: &Ctx, v: &Value) -> Result<()> {
    let access = s(v, "access_token").ok_or_else(|| Error::msg("MyAnimeList didn't return an access token"))?;
    ctx.secrets.set("mal.access", &access)?;
    if let Some(r) = s(v, "refresh_token") {
        ctx.secrets.set("mal.refresh", &r)?;
    }
    set_cfg(ctx, "mal", json!({ "expires_at": chrono::Utc::now().timestamp() + i(v, "expires_in").unwrap_or(3600) }))
}

/// A signed-in access token, refreshed when close to expiry. `None` when only a username is set.
async fn token(ctx: &Ctx) -> Result<Option<String>> {
    let Some(access) = ctx.secrets.get("mal.access") else { return Ok(None) };
    if i(&cfg(ctx, "mal"), "expires_at").unwrap_or(0) - chrono::Utc::now().timestamp() > 300 {
        return Ok(Some(access));
    }
    let refresh = ctx.secrets.get("mal.refresh").ok_or_else(|| Error::Reauth("MyAnimeList".into()))?;
    let v = token_request(ctx, &[("grant_type", "refresh_token"), ("refresh_token", &refresh)]).await.map_err(|e| match e {
        Error::Api { status: 400 | 401, .. } => Error::Reauth("MyAnimeList".into()),
        other => other,
    })?;
    save_tokens(ctx, &v)?;
    Ok(s(&v, "access_token"))
}

pub fn can_write(ctx: &Ctx) -> bool {
    ctx.secrets.get("mal.access").is_some() && store::is_connected(&ctx.db.lock(), "mal")
}

// ------------------------------------------------------------------- titles

/// "Attack on Titan Season 3 Part 2" -> "Attack on Titan"; "Mob Psycho 100 II" -> "Mob Psycho 100".
pub fn base_title(title: &str) -> String {
    let mut t = title.trim().to_string();
    // Drop a trailing "(TV)" / "(2019)" style qualifier first.
    if let Some(open) = t.rfind(" (") {
        if t.ends_with(')') && t.len() - open < 12 {
            t.truncate(open);
        }
    }
    let ordinals = ["2nd", "3rd", "4th", "5th", "6th", "7th", "8th", "9th", "second", "third", "fourth", "fifth", "sixth", "final", "the final", "1st", "first"];
    loop {
        let before = t.clone();
        let lower = t.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        let n = words.len();
        let mut cut = 0usize; // number of trailing words to remove
        if n >= 3 && matches!(words[n - 2], "season" | "part" | "cour") && is_number(words[n - 1]) {
            cut = 2;
        } else if n >= 3 && words[n - 1] == "season" && ordinals.contains(&words[n - 2]) {
            cut = 2;
            if n >= 4 && words[n - 3] == "the" {
                cut = 3;
            }
        } else if n >= 2 && matches!(words[n - 1], "ii" | "iii" | "iv" | "v" | "vi") {
            cut = 1;
        } else if n >= 3 && words[n - 1] == "season" && words[n - 2] == "final" {
            cut = 2;
        } else if n >= 2 && matches!(words[n - 1], "season" | "part") {
            cut = 1;
        }
        if cut > 0 {
            t = t.split_whitespace().take(n - cut).collect::<Vec<_>>().join(" ");
        }
        // "Title: Part 2" leaves a dangling colon or dash.
        t = t.trim_end_matches([':', '-', '–', ' ', ',']).to_string();
        if t == before || t.is_empty() {
            break;
        }
    }
    if t.is_empty() {
        title.trim().to_string()
    } else {
        t
    }
}

fn is_number(w: &str) -> bool {
    w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty()
}

fn english_title(node: &Value) -> String {
    s(&node["alternative_titles"], "en").filter(|t| !t.trim().is_empty()).or_else(|| s(node, "title")).unwrap_or_default()
}

fn media_of(node: &Value) -> Option<Media> {
    match s(node, "media_type").as_deref() {
        Some("movie") => Some(Media::Movie),
        Some("tv" | "ona" | "ova" | "special" | "tv_special") | None => Some(Media::Tv),
        _ => None, // music videos, commercials, PVs
    }
}

/// One MAL entry as a catalog draft (used for seasonal releases, where each entry is news).
pub fn draft(node: &Value) -> Option<ItemDraft> {
    let media = media_of(node)?;
    let id = i(node, "id")?;
    let title = english_title(node);
    if title.is_empty() {
        return None;
    }
    let mut d = ItemDraft::new(media, title.clone()).id("mal", id);
    d.release_date = s(node, "start_date").filter(|d| d.len() == 10);
    d.year = s(node, "start_date").and_then(|d| d.get(..4).and_then(|y| y.parse().ok()));
    d.poster = s(&node["main_picture"], "large").or_else(|| s(&node["main_picture"], "medium"));
    d.overview = s(node, "synopsis").map(|o| o.replace("[Written by MAL Rewrite]", "").trim().to_string()).filter(|o| !o.is_empty());
    d.rating = f(node, "mean");
    d.popularity = i(node, "num_list_users").map(|n| (n as f64).sqrt() / 10.0).unwrap_or(0.0);
    if let Some(mins) = i(node, "average_episode_duration") {
        d.runtime = Some((mins / 60).max(1));
    }
    d.genres = arr(node, "genres").iter().filter_map(|g| s(g, "name")).filter(|g| !NOT_TASTE.contains(&g.to_lowercase().as_str())).collect();
    d.tags = vec!["anime".into()];
    d.creators = arr(node, "studios").iter().filter_map(|st| s(st, "name")).map(|name| Creator { name, role: "studio".into() }).collect();
    d.language = Some("ja".into());
    d.set("mal_score", f(node, "mean"));
    d.set("episodes", i(node, "num_episodes"));
    if s(node, "title").as_deref() != Some(title.as_str()) {
        d.set("original_title", s(node, "title"));
    }
    // A sequel season is news as itself, but shouldn't be merged into its show's catalog entry.
    if media == Media::Tv && base_title(&title) != title {
        d.set("mal_sequel", true);
    }
    d.tag_source = TagSource::Mal;
    Some(d)
}

struct Entry {
    node: Value,
    status: String,
    score: f64,
    updated: Option<String>,
    rewatching: bool,
}

/// Folds list entries into one draft per show (TV) or film, with the events they imply.
/// (kind, value, when, meta) for one event a list entry implies.
pub type FoldedEvent = (EventKind, f64, Option<String>, Value);

pub fn fold(entries: Vec<(Value, Value)>) -> Vec<(ItemDraft, Vec<FoldedEvent>)> {
    let mut groups: BTreeMap<(String, String), Vec<Entry>> = BTreeMap::new();
    for (node, status) in entries {
        let Some(media) = media_of(&node) else { continue };
        let title = english_title(&node);
        if title.is_empty() {
            continue;
        }
        let key = if media == Media::Tv { base_title(&title).to_lowercase() } else { format!("{}#{}", title.to_lowercase(), i(&node, "id").unwrap_or(0)) };
        groups.entry((media.as_str().to_string(), key)).or_default().push(Entry {
            status: s(&status, "status").unwrap_or_default(),
            score: f(&status, "score").unwrap_or(0.0),
            updated: s(&status, "updated_at"),
            rewatching: status["is_rewatching"] == true,
            node,
        });
    }
    let mut out = vec![];
    for (_, mut group) in groups {
        group.sort_by(|a, b| s(&a.node, "start_date").unwrap_or_else(|| "9999".into()).cmp(&s(&b.node, "start_date").unwrap_or_else(|| "9999".into())));
        let first = &group[0].node;
        let Some(mut d) = draft(first) else { continue };
        if d.media() == Media::Tv {
            d.title = base_title(&d.title);
            d.data.remove("mal_sequel");
            d.set("mal_sequel", false);
            d.release_date = None; // the franchise's date isn't the first entry's: let the catalog fill it
        }
        for e in group.iter().skip(1) {
            if let Some(id) = i(&e.node, "id") {
                d.ids.push(("mal".into(), id.to_string()));
            }
            for g in arr(&e.node, "genres").iter().filter_map(|g| s(g, "name")) {
                if !d.genres.contains(&g) && !NOT_TASTE.contains(&g.to_lowercase().as_str()) {
                    d.genres.push(g);
                }
            }
        }
        let latest = group.iter().filter_map(|e| e.updated.clone()).max();
        let mut events = vec![];
        let scores: Vec<f64> = group.iter().map(|e| e.score).filter(|s| *s > 0.0).collect();
        if !scores.is_empty() {
            // MAL scores out of 10; Marquee uses five stars with halves.
            let stars = ((scores.iter().sum::<f64>() / scores.len() as f64) / 2.0 * 2.0).round() / 2.0;
            events.push((EventKind::Rating, stars.clamp(0.5, 5.0), latest.clone(), json!({ "mal_scores": scores })));
        }
        let seen: Vec<&Entry> = group.iter().filter(|e| matches!(e.status.as_str(), "completed" | "watching" | "on_hold")).collect();
        if !seen.is_empty() {
            let plays = seen.iter().filter(|e| e.status == "completed").count().max(1) as f64;
            events.push((EventKind::Watched, plays, latest.clone(), json!({ "plays": plays, "rewatch": group.iter().any(|e| e.rewatching) })));
        }
        if seen.is_empty() && scores.is_empty() {
            if group.iter().any(|e| e.status == "dropped") {
                // Dropped without a score: a mild "not for me".
                events.push((EventKind::Dismiss, 1.0, latest.clone(), json!({ "reason": "other", "from": "mal:dropped" })));
            } else if group.iter().any(|e| e.status == "plan_to_watch") {
                events.push((EventKind::Wishlist, 1.0, latest.clone(), Value::Null));
            }
        }
        if !events.is_empty() {
            out.push((d, events));
        }
    }
    out
}

/// Every entry on a list, following MAL's `paging.next` links.
async fn list_entries(ctx: &Ctx) -> Result<Vec<(Value, Value)>> {
    let id = ctx.require_key("mal_client_id")?;
    let token = token(ctx).await?;
    let user = match &token {
        Some(_) => "@me".to_string(),
        None => s(&cfg(ctx, "mal"), "username").ok_or_else(|| Error::Reauth("MyAnimeList".into()))?,
    };
    let mut url = format!("{API}/users/{}/animelist?fields={LIST_FIELDS}&limit=1000&nsfw=true", urlencoding::encode(&user));
    let mut out = vec![];
    for _ in 0..40 {
        let mut c = call(ctx, "GET", url.clone(), &id).ttl(900).vary(user.clone());
        if let Some(t) = &token {
            c = c.bearer(t);
        }
        let v: Value = c.json().await.map_err(explain)?;
        for row in arr(&v, "data") {
            out.push((row["node"].clone(), row["list_status"].clone()));
        }
        match s(&v["paging"], "next") {
            Some(next) if next.starts_with(API) => url = next,
            _ => break,
        }
    }
    Ok(out)
}

/// The most popular anime of a season ("winter", "spring", "summer", "fall").
pub async fn seasonal(ctx: &Ctx, year: i32, season: &str, limit: usize) -> Result<Vec<ItemDraft>> {
    let id = ctx.require_key("mal_client_id")?;
    let v: Value =
        call(ctx, "GET", format!("{API}/anime/season/{year}/{season}"), &id).query(&[("sort", "anime_num_list_users"), ("limit", &limit.min(500).to_string()), ("fields", ANIME_FIELDS)]).ttl(12 * 3600).json().await.map_err(explain)?;
    Ok(arr(&v, "data").iter().filter_map(|row| draft(&row["node"])).collect())
}

/// The anime season a date falls in, as MAL names them.
pub fn season_of(date: chrono::NaiveDate) -> (i32, &'static str) {
    use chrono::Datelike;
    let s = match date.month() {
        1..=3 => "winter",
        4..=6 => "spring",
        7..=9 => "summer",
        _ => "fall",
    };
    (date.year(), s)
}

/// This season and the next, for the new-release feeds.
pub async fn current_and_next(ctx: &Ctx) -> Result<Vec<ItemDraft>> {
    let today = chrono::Utc::now().date_naive();
    let next = today + chrono::Duration::days(92);
    let (y1, s1) = season_of(today);
    let (y2, s2) = season_of(next);
    let (a, b) = futures::join!(seasonal(ctx, y1, s1, 80), seasonal(ctx, y2, s2, 60));
    let mut out = a?;
    out.extend(b.unwrap_or_default());
    Ok(out)
}

/// The MAL id for an item, searching by title when the item came from elsewhere (TMDB, Trakt).
async fn mal_id(ctx: &Ctx, item: &Item) -> Result<String> {
    if let Some(id) = item.ext("mal") {
        return Ok(id.to_string());
    }
    let client = ctx.require_key("mal_client_id")?;
    let q: String = item.title.chars().take(64).collect();
    let v: Value = call(ctx, "GET", format!("{API}/anime"), &client).query(&[("q", q.as_str()), ("limit", "8"), ("fields", "start_date,media_type,alternative_titles")]).ttl(7 * 86_400).json().await.map_err(explain)?;
    let rows = arr(&v, "data");
    let year = |n: &Value| s(n, "start_date").and_then(|d| d.get(..4).and_then(|y| y.parse::<i32>().ok()));
    let wanted = if item.media == Media::Movie { "movie" } else { "tv" };
    let best = rows
        .iter()
        .map(|r| &r["node"])
        .max_by_key(|n| {
            let same_type = s(n, "media_type").as_deref() == Some(wanted);
            let same_year = item.year.is_some() && year(n) == item.year;
            let title_match = english_title(n).eq_ignore_ascii_case(&item.title) || s(n, "title").map(|t| t.eq_ignore_ascii_case(&item.title)).unwrap_or(false);
            (title_match as u8) * 4 + (same_year as u8) * 2 + same_type as u8
        })
        .and_then(|n| i(n, "id"))
        .ok_or_else(|| Error::msg(format!("MyAnimeList has no match for {}.", item.title)))?;
    let id = best.to_string();
    let _ = ctx.db.lock().execute("INSERT OR IGNORE INTO item_ids (source, ext_id, media, item_id) VALUES ('mal', ?1, ?2, ?3)", rusqlite::params![id, item.media.as_str(), item.id]);
    Ok(id)
}

/// True for titles MyAnimeList tracks: anything that came from MAL, or Japanese animation.
pub fn is_anime(item: &Item) -> bool {
    if !matches!(item.media, Media::Movie | Media::Tv) {
        return false;
    }
    if item.ext("mal").is_some() {
        return true;
    }
    let genres: Vec<String> = item.data["genres"].as_array().map(|g| g.iter().filter_map(|x| x.as_str().map(|s| s.to_lowercase())).collect()).unwrap_or_default();
    let animated = genres.iter().any(|g| g == "animation" || g == "anime");
    let japanese = item.data["language"].as_str() == Some("ja") || item.data["origin_country"].as_array().map(|c| c.iter().any(|x| x == "JP")).unwrap_or(false);
    animated && japanese
}

#[async_trait]
impl Source for Mal {
    fn id(&self) -> &'static str {
        "mal"
    }
    fn name(&self) -> &'static str {
        "MyAnimeList"
    }
    fn reads(&self) -> Vec<&'static str> {
        vec!["Anime list and statuses", "Scores", "Plan to watch"]
    }
    fn writes(&self) -> Vec<&'static str> {
        vec!["Plan to watch", "Completed", "Scores"]
    }
    fn note(&self) -> &'static str {
        "Your anime list. A username reads a public list; sign in as well to let Marquee update it."
    }
    fn configured(&self, ctx: &Ctx) -> bool {
        ctx.has_key("mal_client_id")
    }

    async fn sync(&self, ctx: &Ctx) -> Result<Synced> {
        let entries = list_entries(ctx).await?;
        let mut out = Synced::default();
        let mut plan = vec![];
        for (draft, events) in fold(entries) {
            for (kind, value, at, meta) in events {
                if kind == EventKind::Wishlist {
                    plan.push(draft.clone());
                }
                let mut ev = RawEvent::new(draft.clone(), kind, value).at(at);
                ev.meta = meta;
                out.events.push(ev);
            }
        }
        out.lists.push(("mal:list".into(), plan));
        if out.events.is_empty() {
            out.warnings.push("Your MyAnimeList list is empty, or private. To read a private list, sign in to MyAnimeList in Settings → Connections.".into());
        }
        Ok(out)
    }

    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome> {
        let item = item_for_write(ctx, op.item_id())?;
        if !matches!(item.media, Media::Movie | Media::Tv) {
            return Err(Error::msg("MyAnimeList only tracks anime."));
        }
        let Some(token) = token(ctx).await? else {
            return Err(Error::msg("Sign in to MyAnimeList (Settings → Connections) to let Marquee update your list. Connecting with a username only reads it."));
        };
        let client = ctx.require_key("mal_client_id")?;
        let id = mal_id(ctx, &item).await?;
        let url = format!("{API}/anime/{id}/my_list_status");
        let score;
        let form: Vec<(&str, &str)> = match op {
            WriteOp::AddToList { .. } => vec![("status", "plan_to_watch")],
            WriteOp::RemoveFromList { .. } => {
                call(ctx, "DELETE", url, &client).bearer(&token).send_raw().await?;
                return Ok(WriteOutcome::Synced);
            }
            WriteOp::MarkDone { .. } => vec![("status", "completed")],
            WriteOp::Rate { stars, .. } => {
                score = ((stars * 2.0).round() as i64).clamp(0, 10).to_string();
                vec![("score", score.as_str())]
            }
            _ => return Err(Error::msg("MyAnimeList doesn't support that.")),
        };
        call(ctx, "PATCH", url, &client).bearer(&token).form(&form).send().await.map_err(explain)?;
        Ok(WriteOutcome::Synced)
    }

    fn forget(&self, ctx: &Ctx) {
        let _ = ctx.secrets.delete("mal.access");
        let _ = ctx.secrets.delete("mal.refresh");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    const LIST: &str = r#"{"data":[
      {"node":{"id":16498,"title":"Shingeki no Kyojin","alternative_titles":{"en":"Attack on Titan"},"media_type":"tv","start_date":"2013-04-07","genres":[{"id":1,"name":"Action"},{"id":8,"name":"Drama"},{"id":46,"name":"Award Winning"}],"studios":[{"id":858,"name":"Wit Studio"}],"mean":8.55,"main_picture":{"large":"https://cdn.myanimelist.net/images/anime/10/47347l.jpg"}},
       "list_status":{"status":"completed","score":8,"updated_at":"2024-01-02T00:00:00+00:00"}},
      {"node":{"id":38524,"title":"Shingeki no Kyojin Season 3 Part 2","alternative_titles":{"en":"Attack on Titan Season 3 Part 2"},"media_type":"tv","start_date":"2019-04-29","genres":[{"id":1,"name":"Action"},{"id":37,"name":"Suspense"}]},
       "list_status":{"status":"completed","score":10,"updated_at":"2024-02-02T00:00:00+00:00"}},
      {"node":{"id":5114,"title":"Fullmetal Alchemist: Brotherhood","alternative_titles":{"en":"Fullmetal Alchemist: Brotherhood"},"media_type":"tv","start_date":"2009-04-05"},
       "list_status":{"status":"plan_to_watch","score":0}},
      {"node":{"id":32281,"title":"Kimi no Na wa.","alternative_titles":{"en":"Your Name."},"media_type":"movie","start_date":"2016-08-26","genres":[{"id":22,"name":"Romance"}]},
       "list_status":{"status":"completed","score":8}},
      {"node":{"id":1,"title":"Dropped Show","media_type":"tv"},"list_status":{"status":"dropped","score":0}},
      {"node":{"id":2,"title":"Some PV","media_type":"pv"},"list_status":{"status":"completed","score":7}}
    ],"paging":{"next":"https://api.myanimelist.net/v2/users/larosa/animelist?offset=1000&fields=list_status&limit=1000"}}"#;

    #[test]
    fn season_suffixes_come_off() {
        for (raw, base) in [
            ("Attack on Titan Season 3 Part 2", "Attack on Titan"),
            ("Attack on Titan: The Final Season", "Attack on Titan"),
            ("Mob Psycho 100 II", "Mob Psycho 100"),
            ("Kaguya-sama: Love is War Season 2", "Kaguya-sama: Love is War"),
            ("Spy x Family Part 2", "Spy x Family"),
            ("Vinland Saga Season 2", "Vinland Saga"),
            ("Sword Art Online II", "Sword Art Online"),
            ("Overlord IV", "Overlord"),
            ("Hunter x Hunter (2011)", "Hunter x Hunter"),
            ("Mushoku Tensei: Jobless Reincarnation Cour 2", "Mushoku Tensei: Jobless Reincarnation"),
            ("Frieren: Beyond Journey's End", "Frieren: Beyond Journey's End"),
            ("Steins;Gate 0", "Steins;Gate 0"),
            ("Season", "Season"),
        ] {
            assert_eq!(base_title(raw), base, "{raw}");
        }
    }

    #[test]
    fn a_franchise_folds_into_one_show_with_its_seasons_ids() {
        let v: Value = serde_json::from_str(LIST).unwrap();
        let entries = arr(&v, "data").iter().map(|r| (r["node"].clone(), r["list_status"].clone())).collect();
        let folded = fold(entries);
        let aot = folded.iter().find(|(d, _)| d.title == "Attack on Titan").unwrap();
        assert_eq!(aot.0.ids, vec![("mal".to_string(), "16498".to_string()), ("mal".to_string(), "38524".to_string())]);
        assert!(aot.0.genres.contains(&"Suspense".to_string()), "genres from every season");
        assert!(!aot.0.genres.contains(&"Award Winning".to_string()), "not a taste signal");
        assert_eq!(aot.0.creators[0].name, "Wit Studio");
        assert!(aot.0.tags.contains(&"anime".to_string()));
        let rating = aot.1.iter().find(|e| e.0 == EventKind::Rating).unwrap();
        assert_eq!(rating.1, 4.5, "scores 8 and 10 average to 9/10, which is 4.5 stars");
        let watched = aot.1.iter().find(|e| e.0 == EventKind::Watched).unwrap();
        assert_eq!(watched.1, 2.0, "two completed seasons");

        let fma = folded.iter().find(|(d, _)| d.title.starts_with("Fullmetal")).unwrap();
        assert_eq!(fma.1.len(), 1);
        assert_eq!(fma.1[0].0, EventKind::Wishlist);

        let film = folded.iter().find(|(d, _)| d.title == "Your Name.").unwrap();
        assert_eq!(film.0.media(), Media::Movie);
        assert_eq!(film.0.data["original_title"], "Kimi no Na wa.");

        let dropped = folded.iter().find(|(d, _)| d.title == "Dropped Show").unwrap();
        assert_eq!(dropped.1[0].0, EventKind::Dismiss);
        assert!(!folded.iter().any(|(d, _)| d.title == "Some PV"), "promo videos are skipped");
    }

    #[tokio::test]
    async fn a_username_reads_the_public_list_and_follows_pages() {
        let t = ctx();
        t.net.on("animelist?limit=1", 200, r#"{"data":[]}"#);
        assert_eq!(connect(&t.ctx, "@larosa").await.unwrap(), "larosa");
        assert!(t.net.last("animelist").unwrap().headers.iter().any(|(k, v)| k == "X-MAL-CLIENT-ID" && v == "test-mal_client_id"));

        t.net.on("offset=1000", 200, r#"{"data":[{"node":{"id":9253,"title":"Steins;Gate","media_type":"tv","start_date":"2011-04-06"},"list_status":{"status":"completed","score":10}}],"paging":{}}"#);
        t.net.on("users/larosa/animelist?fields=", 200, LIST);
        let synced = Mal.sync(&t.ctx).await.unwrap();
        assert!(synced.events.iter().any(|e| e.draft.title == "Steins;Gate"), "the second page was read");
        assert!(synced.events.iter().any(|e| e.draft.title == "Attack on Titan"));
        assert_eq!(synced.lists[0].0, "mal:list");
        assert_eq!(synced.lists[0].1.len(), 1);
        assert!(t.net.last("users/larosa/animelist?fields=").unwrap().url.contains("nsfw=true"));

        let item = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Tv, "Frieren").id("mal", 52991)).unwrap();
        let err = Mal.write(&t.ctx, &WriteOp::AddToList { item_id: item, target: "list".into() }).await.unwrap_err();
        assert!(err.to_string().contains("Sign in"), "a username alone can't write");
    }

    #[test]
    fn the_sign_in_uses_the_plain_challenge() {
        let url = authorize_url("cid", "the-verifier", "st");
        assert!(url.contains("code_challenge=the-verifier") && url.contains("code_challenge_method=plain"));
        assert!(url.contains(&urlencoding::encode(&redirect_uri()).to_string()));
    }

    #[tokio::test]
    async fn signing_in_then_writing_updates_the_list() {
        let t = ctx();
        t.net.on("/v1/oauth2/token", 200, r#"{"token_type":"Bearer","expires_in":2678400,"access_token":"acc","refresh_token":"ref"}"#);
        t.net.on("/users/@me", 200, r#"{"id":1,"name":"larosa"}"#);
        let params: HashMap<String, String> = [("code".to_string(), "c0de".to_string()), ("state".to_string(), "st".to_string())].into();
        assert_eq!(finish_sign_in(&t.ctx, &params, "verif", "st").await.unwrap(), "larosa");
        let body = String::from_utf8(t.net.last("/v1/oauth2/token").unwrap().body.unwrap()).unwrap();
        assert!(body.contains("code_verifier=verif") && body.contains("grant_type=authorization_code"));
        assert!(can_write(&t.ctx));

        // An item from TMDB: its MAL id is found by searching, then remembered.
        let mut d = ItemDraft::new(Media::Tv, "Frieren: Beyond Journey's End").id("tmdb", 209867);
        d.year = Some(2023);
        let item = store::upsert_item(&t.ctx.db.lock(), &d).unwrap();
        t.net.on("/v2/anime?q=", 200, r#"{"data":[{"node":{"id":1,"title":"Other","media_type":"tv","start_date":"2010-01-01"}},{"node":{"id":52991,"title":"Sousou no Frieren","alternative_titles":{"en":"Frieren: Beyond Journey's End"},"media_type":"tv","start_date":"2023-09-29"}}]}"#);
        t.net.on("PATCH https://api.myanimelist.net/v2/anime/52991/my_list_status", 200, r#"{"status":"plan_to_watch"}"#);
        assert_eq!(Mal.write(&t.ctx, &WriteOp::AddToList { item_id: item, target: "list".into() }).await.unwrap(), WriteOutcome::Synced);
        let req = t.net.last("my_list_status").unwrap();
        assert_eq!(req.method, "PATCH");
        assert_eq!(String::from_utf8(req.body.unwrap()).unwrap(), "status=plan_to_watch");
        assert!(req.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer acc"));

        Mal.write(&t.ctx, &WriteOp::Rate { item_id: item, stars: 4.5 }).await.unwrap();
        assert_eq!(String::from_utf8(t.net.last("my_list_status").unwrap().body.unwrap()).unwrap(), "score=9");
        assert_eq!(t.net.calls("/v2/anime?q="), 1, "the MAL id was remembered");
    }

    #[tokio::test]
    async fn seasonal_anime_arrive_as_new_releases() {
        let t = ctx();
        t.net.on("/anime/season/", 200, r#"{"data":[
            {"node":{"id":59978,"title":"Sousou no Frieren 2nd Season","alternative_titles":{"en":"Frieren: Beyond Journey's End Season 2"},"media_type":"tv","start_date":"2027-01-16","mean":9.1,"num_list_users":400000,"genres":[{"name":"Adventure"},{"name":"Fantasy"}],"studios":[{"name":"Madhouse"}]}},
            {"node":{"id":3,"title":"A Music Video","media_type":"music"}}]}"#);
        let drafts = seasonal(&t.ctx, 2027, "winter", 50).await.unwrap();
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert_eq!(d.title, "Frieren: Beyond Journey's End Season 2");
        assert_eq!(d.release_date.as_deref(), Some("2027-01-16"));
        assert_eq!(d.data["mal_sequel"], true);
        assert!(crate::taste::vocab::features_for(d).contains_key("t:anime"));
        assert_eq!(season_of(chrono::NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()), (2026, "fall"));
    }
}
