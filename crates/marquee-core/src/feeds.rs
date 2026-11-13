//! Everything the screens show: Home rows, For You, the media hubs, the calendar, detail pages,
//! Quick Rate and the Taste screen. All of it is read from the local database, so it renders
//! instantly and works offline.

use crate::catalog;
use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::store::{self, cards, CARD_COLS, CARD_FROM};
use crate::taste::{self, score, vocab};
use rusqlite::types::Value as Sql;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Titles that may appear in a feed: scored (blocked and dismissed titles have no score row).
const VISIBLE: &str = "i.media != 'artist' AND s.item_id IS NOT NULL";
const NOT_DONE: &str = "NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'play', 'rating', 'owned'))";

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Filters {
    pub media: Vec<String>,
    /// Shared genre slugs ("sci-fi", "rpg", "ambient").
    pub genres: Vec<String>,
    /// "all" | "released" | "upcoming"
    pub status: String,
    pub min_runtime: Option<i64>,
    pub max_runtime: Option<i64>,
    /// Only titles streaming on a service the user subscribes to.
    pub my_services: bool,
    pub on_sale: bool,
    /// Cents.
    pub max_price: Option<i64>,
    pub platform: Option<String>,
    pub wishlisted: bool,
    pub hide_done: bool,
    /// "album" | "ep" | "single"
    pub album_types: Vec<String>,
    /// Only shows the user is tracking.
    pub tracking: bool,
    /// Only artists the user follows or plays.
    pub my_artists: bool,
    /// "match" | "date" | "date_desc" | "popularity" | "rating"
    pub sort: String,
}

fn in_list(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// Builds the WHERE clause and its parameters for a set of filters.
fn where_clause(ctx: &Ctx, f: &Filters) -> (String, Vec<Sql>) {
    let mut sql = vec![VISIBLE.to_string()];
    let mut args: Vec<Sql> = vec![];
    let media: Vec<&String> = f.media.iter().filter(|m| Media::parse(m).map(|m| m != Media::Artist).unwrap_or(false)).collect();
    if !media.is_empty() {
        sql.push(format!("i.media IN ({})", in_list(media.len())));
        args.extend(media.iter().map(|m| Sql::Text(Media::parse(m).map(|m| m.as_str()).unwrap_or("movie").to_string())));
    }
    if !f.genres.is_empty() {
        sql.push(format!("EXISTS (SELECT 1 FROM item_features x WHERE x.item_id = i.id AND x.feature IN ({}))", in_list(f.genres.len())));
        args.extend(f.genres.iter().map(|g| Sql::Text(if g.contains(':') { g.clone() } else { format!("g:{g}") })));
    }
    match f.status.as_str() {
        "released" => {
            sql.push("i.release_date <= ?".into());
            args.push(Sql::Text(today()));
        }
        "upcoming" => {
            sql.push("i.release_date > ?".into());
            args.push(Sql::Text(today()));
        }
        _ => {}
    }
    if let Some(min) = f.min_runtime {
        sql.push("i.runtime >= ?".into());
        args.push(Sql::Integer(min));
    }
    if let Some(max) = f.max_runtime {
        sql.push("i.runtime IS NOT NULL AND i.runtime <= ?".into());
        args.push(Sql::Integer(max));
    }
    if f.my_services {
        let mine: Vec<i64> = store::setting_or(&ctx.db, "my_providers", vec![]);
        if mine.is_empty() {
            sql.push("0".into());
        } else {
            sql.push(format!(
                "EXISTS (SELECT 1 FROM json_each(i.data, '$.providers.stream') p WHERE json_extract(p.value, '$.id') IN ({}))",
                mine.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(",")
            ));
        }
    }
    if f.on_sale {
        sql.push("COALESCE(json_extract(i.data, '$.discount'), 0) > 0".into());
    }
    if let Some(max) = f.max_price {
        sql.push("json_extract(i.data, '$.price.cents') IS NOT NULL AND json_extract(i.data, '$.price.cents') <= ?".into());
        args.push(Sql::Integer(max));
    }
    if let Some(p) = f.platform.as_ref().filter(|p| !p.is_empty()) {
        sql.push("EXISTS (SELECT 1 FROM json_each(i.data, '$.platforms') p WHERE lower(p.value) = lower(?))".into());
        args.push(Sql::Text(p.clone()));
    }
    if f.wishlisted {
        sql.push("(EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'wishlist') OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id))".into());
    }
    if f.hide_done {
        sql.push(NOT_DONE.into());
    }
    if !f.album_types.is_empty() {
        sql.push(format!("(i.media != 'album' OR json_extract(i.data, '$.album_type') IN ({}))", in_list(f.album_types.len())));
        args.extend(f.album_types.iter().map(|t| Sql::Text(t.clone())));
    }
    if f.tracking {
        sql.push(
            "(EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'rating', 'like', 'play')) OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id) OR EXISTS (SELECT 1 FROM follows fo WHERE fo.item_id = i.id))"
                .into(),
        );
    }
    if f.my_artists {
        sql.push(MY_ARTISTS.into());
    }
    (sql.join(" AND "), args)
}

/// An album whose artist the user follows, loves or plays.
const MY_ARTISTS: &str = "(EXISTS (SELECT 1 FROM follows fo WHERE fo.kind = 'artist' AND lower(fo.name) = lower(json_extract(i.data, '$.artist')))
    OR EXISTS (SELECT 1 FROM items a JOIN user_events e ON e.item_id = a.id WHERE a.media = 'artist' AND lower(a.title) = lower(json_extract(i.data, '$.artist'))))";

fn order_by(sort: &str) -> &'static str {
    match sort {
        "date" => "i.release_date IS NULL, i.release_date ASC, s.score DESC",
        "date_desc" => "i.release_date IS NULL, i.release_date DESC, s.score DESC",
        "popularity" => "i.popularity DESC",
        "rating" => "i.rating IS NULL, i.rating DESC, i.popularity DESC",
        _ => "s.score DESC, i.popularity DESC",
    }
}

fn query_cards(ctx: &Ctx, where_sql: &str, args: &[Sql], order: &str, limit: i64, offset: i64) -> Result<Vec<Card>> {
    let conn = ctx.db.lock();
    let sql = format!("SELECT {CARD_COLS} FROM {CARD_FROM} WHERE {where_sql} ORDER BY {order} LIMIT {limit} OFFSET {offset}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), store::card_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Re-ranks cards so a row isn't all one franchise, creator or medium.
fn spread(ctx: &Ctx, list: Vec<Card>, take: usize, lambda: f64) -> Result<Vec<Card>> {
    if list.len() <= 2 {
        return Ok(list);
    }
    let conn = ctx.db.lock();
    let mut scores: HashMap<i64, f64> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached("SELECT score FROM scores WHERE item_id = ?1")?;
        for c in &list {
            scores.insert(c.id, stmt.query_row([c.id], |r| r.get(0)).unwrap_or(0.0));
        }
    }
    let rows: Vec<(i64, f64, String)> = list.iter().map(|c| (c.id, scores[&c.id], c.media.clone())).collect();
    let order = score::diversify(score::candidates(&conn, &rows)?, lambda, take);
    let mut by_id: HashMap<i64, Card> = list.into_iter().map(|c| (c.id, c)).collect();
    Ok(order.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

fn diversity(ctx: &Ctx) -> f64 {
    0.25 + 0.45 * taste::TasteSettings::load(&ctx.db).dial
}

fn week_bounds() -> (String, String) {
    let today = chrono::Local::now().date_naive();
    let monday = today - chrono::Duration::days(chrono::Datelike::weekday(&today).num_days_from_monday() as i64);
    (monday.format("%Y-%m-%d").to_string(), (monday + chrono::Duration::days(6)).format("%Y-%m-%d").to_string())
}

pub fn has_taste(ctx: &Ctx) -> bool {
    ctx.db.lock().query_row("SELECT EXISTS (SELECT 1 FROM taste_profile)", [], |r| r.get(0)).unwrap_or(false)
}

fn row(id: &str, title: &str, subtitle: &str, cards: Vec<Card>) -> Value {
    json!({ "id": id, "title": title, "subtitle": subtitle, "cards": cards })
}

/// Home, "Now Showing": the hero carousel and every row. Empty rows are left out.
pub fn home(ctx: &Ctx) -> Result<Value> {
    let (monday, sunday) = week_bounds();
    let today = today();
    let lambda = diversity(ctx);
    let t = |s: &str| Sql::Text(s.to_string());
    let personal = has_taste(ctx);
    // Before there's a taste profile, "best" means "most anticipated".
    let best = if personal { "s.score DESC, i.popularity DESC" } else { "i.popularity DESC" };

    // Hero: the week's biggest releases that match, and that have artwork worth showing big.
    let hero_pool = query_cards(
        ctx,
        &format!("{VISIBLE} AND i.backdrop IS NOT NULL AND i.release_date BETWEEN ? AND ? AND {NOT_DONE}"),
        &[t(&days_from_today(-7)), t(&days_from_today(10))],
        if personal { "(s.score * 2 + i.popularity / 200.0) DESC" } else { "i.popularity DESC" },
        24,
        0,
    )?;
    let hero = spread(ctx, hero_pool, 7, lambda.max(0.5))?;

    let mut rows = vec![];
    let mut push = |v: Value| {
        if v["cards"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            rows.push(v);
        }
    };

    let out_now = query_cards(ctx, &format!("{VISIBLE} AND i.release_date BETWEEN ? AND ? AND {NOT_DONE}"), &[t(&monday), t(&sunday)], best, 60, 0)?;
    push(row("this-week", "Out This Week", "New in theaters, on streaming, in stores and on your speakers", spread(ctx, out_now, 20, lambda)?));

    let soon = query_cards(ctx, &format!("{VISIBLE} AND i.release_date > ? AND i.release_date <= ?"), &[t(&sunday), t(&days_from_today(90))], best, 80, 0)?;
    push(row("coming-soon", "Coming Soon", "The next three months, best matches first", spread(ctx, soon, 20, lambda)?));

    // Shows the user actually watches with an episode on the way; season premieres first.
    let seasons = query_cards(
        ctx,
        &format!(
            "{VISIBLE} AND i.media = 'tv' AND json_extract(i.data, '$.next_episode.air_date') BETWEEN ? AND ?
             AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'rating', 'like', 'play')) OR EXISTS (SELECT 1 FROM follows fo WHERE fo.item_id = i.id))"
        ),
        &[t(&days_from_today(-7)), t(&days_from_today(75))],
        "(json_extract(i.data, '$.next_episode.episode') = 1) DESC, json_extract(i.data, '$.next_episode.air_date') ASC",
        20,
        0,
    )?;
    push(row("new-seasons", "New Seasons of Shows You Watch", "Premieres and new episodes on the way", seasons));

    let artists = query_cards(
        ctx,
        &format!("{VISIBLE} AND i.media = 'album' AND i.release_date BETWEEN ? AND ? AND {MY_ARTISTS}"),
        &[t(&days_from_today(-45)), t(&days_from_today(90))],
        "abs(julianday(i.release_date) - julianday('now')) ASC",
        20,
        0,
    )?;
    push(row("artists", "New From Artists You Love", "Fresh albums, EPs and singles", artists));

    let wish = query_cards(
        ctx,
        &format!(
            "{VISIBLE} AND i.media = 'game'
             AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'wishlist') OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id))
             AND NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'owned')
             AND (COALESCE(json_extract(i.data, '$.discount'), 0) > 0 OR i.release_date BETWEEN ? AND ?)"
        ),
        &[t(&days_from_today(-30)), t(&today)],
        "COALESCE(json_extract(i.data, '$.discount'), 0) DESC, i.release_date DESC",
        20,
        0,
    )?;
    push(row("wishlist-games", "Games You Wishlisted Are Out / On Sale", "From your Steam wishlist and Play Later", wish));

    let trending = query_cards(ctx, &format!("{VISIBLE} AND json_extract(i.data, '$.trending') = 1 AND {NOT_DONE}"), &[], "i.popularity DESC", 40, 0)?;
    push(row("trending", "Trending", "What everyone's watching this week", spread(ctx, trending, 20, 0.3)?));

    if personal {
        // Great matches that aren't the obvious hits: the less popular 60% of each medium.
        let mut cutoffs = vec![];
        {
            let conn = ctx.db.lock();
            for m in Media::FEED {
                let cut: f64 = conn
                    .query_row(
                        "SELECT popularity FROM items WHERE media = ?1 ORDER BY popularity DESC LIMIT 1 OFFSET (SELECT count(*) * 2 / 5 FROM items WHERE media = ?1)",
                        [m.as_str()],
                        |r| r.get(0),
                    )
                    .unwrap_or(f64::MAX);
                cutoffs.push(format!("WHEN '{}' THEN {}", m.as_str(), if cut.is_finite() && cut < 1e12 { cut } else { 1e12 }));
            }
        }
        let gems = query_cards(
            ctx,
            &format!("{VISIBLE} AND s.match_pct >= 72 AND i.release_date <= ? AND {NOT_DONE} AND i.popularity <= CASE i.media {} ELSE 1e12 END", cutoffs.join(" ")),
            &[t(&today)],
            "s.score DESC",
            80,
            0,
        )?;
        push(row("gems", "Hidden Gems For You", "Strong matches that flew under the radar", spread(ctx, gems, 20, lambda.max(0.45))?));
    }

    let counts: Value = {
        let conn = ctx.db.lock();
        json!({
            "items": conn.query_row("SELECT count(*) FROM items WHERE media != 'artist'", [], |r| r.get::<_, i64>(0))?,
            "events": conn.query_row("SELECT count(*) FROM user_events", [], |r| r.get::<_, i64>(0))?,
        })
    };
    Ok(json!({ "hero": hero, "rows": rows, "personal": personal, "counts": counts, "week": [monday, sunday] }))
}

/// For You: one mixed-media feed, best match first, diversified, paged.
pub fn for_you(ctx: &Ctx, f: &Filters, offset: usize, limit: usize) -> Result<Value> {
    let mut f = f.clone();
    f.hide_done = true;
    let (w, args) = where_clause(ctx, &f);
    // Rank a generous pool, diversify it once, then page through that order.
    let pool = query_cards(ctx, &w, &args, order_by("match"), 600, 0)?;
    let total = pool.len();
    let ordered = if f.sort.is_empty() || f.sort == "match" { spread(ctx, pool, 600, diversity(ctx))? } else { query_cards(ctx, &w, &args, order_by(&f.sort), 600, 0)? };
    let page: Vec<Card> = ordered.into_iter().skip(offset).take(limit).collect();
    let next = if offset + page.len() < total { Some(offset + page.len()) } else { None };
    Ok(json!({ "cards": page, "next": next, "total": total }))
}

/// A media hub: new and upcoming for one medium, filtered and sorted.
pub fn hub(ctx: &Ctx, media: Media, f: &Filters, offset: usize, limit: usize) -> Result<Value> {
    let mut f = f.clone();
    f.media = vec![media.as_str().to_string()];
    let (mut w, mut args) = where_clause(ctx, &f);
    if f.status.is_empty() || f.status == "all" {
        // "New + upcoming": nothing older than four months unless the user asks for released titles.
        w.push_str(" AND (i.release_date >= ? OR i.release_date IS NULL)");
        args.push(Sql::Text(days_from_today(-120)));
    }
    let sort = if f.sort.is_empty() { "match" } else { f.sort.as_str() };
    let cards = query_cards(ctx, &w, &args, order_by(sort), limit as i64 + 1, offset as i64)?;
    let more = cards.len() > limit;
    let page: Vec<Card> = cards.into_iter().take(limit).collect();

    // Facets for the filter bar.
    let conn = ctx.db.lock();
    let mut stmt = conn.prepare(
        "SELECT substr(x.feature, 3), count(*) FROM item_features x JOIN items i ON i.id = x.item_id
         WHERE i.media = ?1 AND x.feature LIKE 'g:%' GROUP BY x.feature ORDER BY count(*) DESC LIMIT 18",
    )?;
    let genres: Vec<Value> = stmt.query_map([media.as_str()], |r| Ok(json!({ "id": r.get::<_, String>(0)?, "count": r.get::<_, i64>(1)? })))?.collect::<rusqlite::Result<_>>()?;
    let platforms: Vec<String> = if media == Media::Game {
        let mut stmt = conn.prepare("SELECT p.value, count(*) FROM items i, json_each(i.data, '$.platforms') p WHERE i.media = 'game' GROUP BY p.value ORDER BY count(*) DESC LIMIT 12")?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        rows
    } else {
        vec![]
    };
    Ok(json!({ "cards": page, "next": if more { Some(offset + limit) } else { None }, "genres": genres, "platforms": platforms }))
}

/// TV: the shows being tracked, with a countdown to the next episode.
pub fn tracked_shows(ctx: &Ctx) -> Result<Vec<Value>> {
    let conn = ctx.db.lock();
    let list = cards(
        &conn,
        "WHERE i.media = 'tv' AND (EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'rating', 'like', 'play'))
            OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id) OR EXISTS (SELECT 1 FROM follows fo WHERE fo.item_id = i.id))
         ORDER BY json_extract(i.data, '$.next_episode.air_date') IS NULL, json_extract(i.data, '$.next_episode.air_date') ASC, i.popularity DESC LIMIT 80",
        &[],
    )?;
    let today = chrono::Local::now().date_naive();
    Ok(list
        .into_iter()
        .map(|c| {
            let ep = c.meta["next_episode"].clone();
            let days = ep["air_date"].as_str().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).map(|d| (d - today).num_days());
            json!({ "card": c, "next_episode": ep, "days": days })
        })
        .collect())
}

#[derive(Debug, Serialize, Clone)]
pub struct CalendarEntry {
    pub date: String,
    /// "release" | "episode" | "premiere" | "finale"
    pub kind: &'static str,
    pub label: String,
    pub card: Card,
    pub pinned: bool,
}

/// Releases between two dates. `scope`: "taste" (matches and anything you follow or saved),
/// "lists" (only what you follow or saved) or "all".
pub fn calendar(ctx: &Ctx, from: &str, to: &str, scope: &str) -> Result<Vec<CalendarEntry>> {
    const MINE: &str = "(EXISTS (SELECT 1 FROM follows fo WHERE fo.item_id = i.id)
        OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id)
        OR EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind = 'wishlist')
        OR json_extract(i.data, '$.followed_franchise') IN (SELECT ref FROM follows WHERE kind = 'franchise'))";
    let scope_sql = match scope {
        "all" => "1".to_string(),
        "lists" => format!("({MINE} OR (i.media = 'album' AND {MY_ARTISTS}))"),
        _ => format!("(s.match_pct >= 68 OR {MINE} OR (i.media = 'album' AND {MY_ARTISTS}))"),
    };
    let conn = ctx.db.lock();
    let mut out = vec![];
    let pinned_sql = format!("SELECT {CARD_COLS}, {MINE} FROM {CARD_FROM} WHERE {VISIBLE} AND i.release_date BETWEEN ?1 AND ?2 AND {scope_sql} ORDER BY i.release_date, s.score DESC LIMIT 600");
    let mut stmt = conn.prepare(&pinned_sql)?;
    for r in stmt.query_map([from, to], |r| Ok((store::card_from_row(r)?, r.get::<_, bool>(17)?)))? {
        let (card, pinned) = r?;
        let Some(date) = card.release_date.clone() else { continue };
        let label = match card.media.as_str() {
            "tv" => "Series premiere".to_string(),
            "album" => card.meta["artist"].as_str().map(|a| format!("New from {a}")).unwrap_or_else(|| "New music".into()),
            "game" => "Game release".to_string(),
            _ => "Movie release".to_string(),
        };
        out.push(CalendarEntry { date, kind: if card.media == "tv" { "premiere" } else { "release" }, label, card, pinned });
    }
    // Episodes of shows being tracked (or everything airing, in the "all" scope).
    let ep_scope = if scope == "all" { "1" } else { "(EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'rating', 'like', 'play')) OR EXISTS (SELECT 1 FROM list_items l WHERE l.item_id = i.id) OR EXISTS (SELECT 1 FROM follows fo WHERE fo.item_id = i.id))" };
    let ep_sql = format!("SELECT {CARD_COLS} FROM {CARD_FROM} WHERE i.media = 'tv' AND json_extract(i.data, '$.next_episode.air_date') BETWEEN ?1 AND ?2 AND {ep_scope} LIMIT 300");
    let mut stmt = conn.prepare(&ep_sql)?;
    for card in stmt.query_map([from, to], store::card_from_row)? {
        let card = card?;
        let ep = &card.meta["next_episode"];
        let (Some(date), s, e) = (ep["air_date"].as_str().map(String::from), ep["season"].as_i64().unwrap_or(0), ep["episode"].as_i64().unwrap_or(0)) else { continue };
        if out.iter().any(|x: &CalendarEntry| x.card.id == card.id && x.date == date) {
            continue; // the series premiere is already listed as a release
        }
        let (kind, label) = if e == 1 { ("premiere", format!("Season {s} premiere")) } else if ep["finale"] == true { ("finale", format!("Season {s} finale")) } else { ("episode", format!("S{s} E{e}")) };
        out.push(CalendarEntry { date, kind, label, card, pinned: scope != "all" });
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(b.pinned.cmp(&a.pinned)).then(b.card.match_pct.cmp(&a.card.match_pct)));
    Ok(out)
}

/// Titles to show in Quick Rate: things the user has probably seen, played or heard but not
/// rated, then well-known titles across every medium to seed a profile from nothing.
pub fn quick_rate(ctx: &Ctx, limit: usize) -> Result<Vec<Card>> {
    let conn = ctx.db.lock();
    let unrated = "NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('rating', 'dismiss'))";
    let mut list = cards(
        &conn,
        &format!(
            "WHERE i.media != 'artist' AND i.poster IS NOT NULL AND {unrated}
               AND EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id AND e.kind IN ('watched', 'play', 'owned'))
             ORDER BY (SELECT max(e.at) FROM user_events e WHERE e.item_id = i.id) DESC LIMIT {}",
            limit / 2
        ),
        &[],
    )?;
    let today = today();
    for media in Media::FEED {
        let per = (limit.saturating_sub(list.len())) / 4 + 2;
        list.extend(cards(
            &conn,
            &format!("WHERE i.media = ?1 AND i.poster IS NOT NULL AND {unrated} AND i.release_date <= ?2 AND NOT EXISTS (SELECT 1 FROM user_events e WHERE e.item_id = i.id) ORDER BY i.popularity DESC LIMIT {per}"),
            &[&media.as_str(), &today],
        )?);
    }
    // Interleave media so the deck isn't twenty films in a row.
    let mut by_media: HashMap<String, Vec<Card>> = HashMap::new();
    let mut order = vec![];
    for c in list {
        if !order.contains(&c.media) {
            order.push(c.media.clone());
        }
        by_media.entry(c.media.clone()).or_default().push(c);
    }
    let mut out = vec![];
    while out.len() < limit && by_media.values().any(|v| !v.is_empty()) {
        for m in &order {
            if let Some(c) = by_media.get_mut(m).and_then(|v| if v.is_empty() { None } else { Some(v.remove(0)) }) {
                out.push(c);
            }
        }
    }
    out.truncate(limit);
    Ok(out)
}

/// Today's releases worth knowing about, for the tray panel.
pub fn today_releases(ctx: &Ctx, limit: i64) -> Result<Vec<Card>> {
    let t = today();
    let order = if has_taste(ctx) { "s.score DESC" } else { "i.popularity DESC" };
    query_cards(ctx, &format!("{VISIBLE} AND i.release_date = ?"), &[Sql::Text(t)], order, limit, 0)
}

/// Similar titles from every medium: the source's own "similar" list, then the closest titles
/// by shared mood and theme tags in each of the other media.
fn similar_everywhere(ctx: &Ctx, item: &Item) -> Result<Value> {
    let conn = ctx.db.lock();
    let same = cards(&conn, "JOIN item_similar sim ON sim.similar_id = i.id WHERE sim.item_id = ?1 AND i.media != 'artist' LIMIT 14", &[&item.id])?;
    let mut stmt = conn.prepare(
        "SELECT f2.item_id, i.media, sum(f1.weight * f2.weight * CASE WHEN f1.feature LIKE 't:%' THEN 1.0 ELSE 0.5 END) AS overlap, count(*) AS shared
         FROM item_features f1 JOIN item_features f2 ON f2.feature = f1.feature JOIN items i ON i.id = f2.item_id
         WHERE f1.item_id = ?1 AND f2.item_id != ?1 AND (f1.feature LIKE 't:%' OR f1.feature LIKE 'g:%') AND i.media != 'artist'
         GROUP BY f2.item_id HAVING shared >= 2 OR overlap >= 0.9 ORDER BY overlap DESC LIMIT 400",
    )?;
    let rows: Vec<(i64, String, f64)> = stmt.query_map([item.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut groups = serde_json::Map::new();
    for media in Media::FEED {
        if media == item.media && same.len() >= 6 {
            continue;
        }
        let ids: Vec<i64> = rows.iter().filter(|r| r.1 == media.as_str()).filter(|r| !same.iter().any(|c| c.id == r.0)).take(10).map(|r| r.0).collect();
        let list = store::cards_by_ids(&conn, &ids)?;
        if !list.is_empty() {
            groups.insert(media.as_str().into(), json!(list));
        }
    }
    Ok(json!({ "same": same, "cross": groups }))
}

/// Everything a detail page needs. Fetches full details first when the stored copy is a stub.
pub async fn detail(ctx: &Ctx, item_id: i64) -> Result<Value> {
    let fetch = catalog::ensure_detail(ctx, item_id).await;
    let offline = fetch.as_ref().err().map(|e| e.is_offline()).unwrap_or(false);
    let note = fetch.err().filter(|e| !e.is_offline()).map(|e| e.to_string());
    let (pct, why) = score::score_item(&ctx.db, item_id).unwrap_or((-1, vec![]));
    let conn = ctx.db.lock();
    let item = store::get_item(&conn, item_id)?;
    let card = store::card(&conn, item_id)?;
    let in_lists = store::lists_containing(&conn, item_id)?;
    let artist_name = item.data["artist"].as_str().map(String::from);
    let following = json!({
        "item": store::is_following(&conn, "item", &item_id.to_string()),
        "artist": artist_name.as_ref().map(|a| store::is_following(&conn, "artist", &slug(a))),
        "franchise": item.data["collection"]["id"].as_i64().map(|c| store::is_following(&conn, "franchise", &c.to_string())),
    });
    let events: Vec<Value> = {
        let mut stmt = conn.prepare("SELECT source, kind, value, at FROM user_events WHERE item_id = ?1 ORDER BY at DESC")?;
        let rows = stmt.query_map([item_id], |r| Ok(json!({ "source": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "value": r.get::<_, f64>(2)?, "at": r.get::<_, String>(3)? })))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    let features: Vec<Value> = store::item_features(&conn, item_id)?
        .into_iter()
        .filter(|(f, _)| f.starts_with("t:"))
        .map(|(f, w)| json!({ "feature": f, "label": vocab::label(&f), "weight": w }))
        .collect();
    let connected: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM sources WHERE status != 'disconnected'")?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    drop(conn);

    // Where this item can be added, in the order the menu shows it.
    let mut targets = vec![];
    let has = |s: &str| connected.iter().any(|c| c == s);
    match item.media {
        Media::Movie | Media::Tv => {
            if item.media == Media::Movie {
                targets.push(json!({ "id": "letterboxd:watchlist", "label": "Letterboxd watchlist", "hint": "Opens the film's page", "ready": true }));
            }
            targets.push(json!({ "id": "trakt:watchlist", "label": "Trakt watchlist", "ready": has("trakt") }));
        }
        Media::Game => {
            if item.ext("steam").is_some() {
                targets.push(json!({ "id": "steam:wishlist", "label": "Steam wishlist", "hint": "Opens the store page", "ready": has("steam") }));
            }
        }
        Media::Album | Media::Artist => {
            targets.push(json!({ "id": "spotify:library", "label": "Save to Spotify library", "ready": has("spotify") }));
            targets.push(json!({ "id": "spotify:playlist:picks", "label": "Marquee Picks playlist", "ready": has("spotify") }));
        }
    }
    let seriesgraph = if item.media == Media::Tv { item.ext("tmdb").map(|id| format!("https://seriesgraph.com/show/{id}")) } else { None };
    Ok(json!({
        "item": item,
        "card": card,
        "match_pct": if pct >= 0 { Some(pct) } else { None },
        "why": why,
        "where": catalog::where_to_get(&item),
        "similar": similar_everywhere(ctx, &item)?,
        "in_lists": in_lists.iter().map(|(id, state)| json!({ "list_id": id, "sync_state": state })).collect::<Vec<_>>(),
        "following": following,
        "events": events,
        "moods": features,
        "targets": targets,
        "seriesgraph": seriesgraph,
        "letterboxd": if item.media == Media::Movie { crate::integrations::letterboxd::film_url(&item) } else { None },
        "offline": offline,
        "note": note,
    }))
}

/// Episode ratings for the heatmap, season by season.
pub async fn episodes(ctx: &Ctx, item_id: i64) -> Result<Value> {
    let item = store::get_item(&ctx.db.lock(), item_id)?;
    let Some(tmdb) = item.ext("tmdb").filter(|_| item.media == Media::Tv) else { return Ok(json!({ "seasons": [] })) };
    let numbers: Vec<i64> = item.data["seasons"].as_array().map(|a| a.iter().filter_map(|s| s["number"].as_i64()).collect()).unwrap_or_default();
    let seasons = catalog::tmdb::episode_ratings(ctx, tmdb, &numbers).await?;
    let rated: Vec<f64> = seasons.iter().flatten().filter_map(|e| e.rating).collect();
    let avg = if rated.is_empty() { None } else { Some((rated.iter().sum::<f64>() / rated.len() as f64 * 10.0).round() / 10.0) };
    Ok(json!({ "seasons": seasons, "average": avg, "source": "TMDB" }))
}

// --------------------------------------------------------------- Taste screen

fn top_features(conn: &rusqlite::Connection, prefix: &str, limit: i64) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare("SELECT feature, weight, support FROM taste_profile WHERE feature LIKE ?1 AND weight > 0 ORDER BY weight DESC LIMIT ?2")?;
    let rows = stmt
        .query_map(rusqlite::params![format!("{prefix}:%"), limit], |r| {
            let f: String = r.get(0)?;
            Ok(json!({ "feature": f, "label": vocab::label(&f), "weight": r.get::<_, f64>(1)?, "support": r.get::<_, i64>(2)? }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The visual taste profile: top genres and themes per medium, the cross-media "taste DNA",
/// favourite creators, decades, adventurousness, and the editable pins and blocks.
pub fn taste_overview(ctx: &Ctx) -> Result<Value> {
    let settings = taste::TasteSettings::load(&ctx.db);
    let affinity = taste::profile::affinities(&ctx.db, &settings)?;
    let conn = ctx.db.lock();

    // Per-medium breakdown: which features carry the affinity inside each medium.
    let mut per_media: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut decades: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut creator_names: HashMap<String, (String, String)> = HashMap::new();
    {
        let mut item_stmt = conn.prepare_cached("SELECT media, data FROM items WHERE id = ?1")?;
        let mut feat_stmt = conn.prepare_cached("SELECT feature, weight FROM item_features WHERE item_id = ?1")?;
        for (id, a) in affinity.iter().filter(|(_, a)| **a > 0.2) {
            let Ok((media, data)) = item_stmt.query_row([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else { continue };
            let media = if media == "artist" { "album".to_string() } else { media };
            let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            for c in data["creators"].as_array().into_iter().flatten() {
                if let (Some(n), Some(role)) = (c["name"].as_str(), c["role"].as_str()) {
                    creator_names.entry(format!("c:{}", slug(n))).or_insert_with(|| (n.to_string(), role.to_string()));
                }
            }
            for row in feat_stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
                let (f, w) = row?;
                match f.as_bytes().first() {
                    Some(b'g' | b't') => *per_media.entry(media.clone()).or_default().entry(f).or_insert(0.0) += a * w,
                    Some(b'e') => *decades.entry(f[2..].to_string()).or_default().entry(media.clone()).or_insert(0.0) += a,
                    _ => {}
                }
            }
        }
    }
    let top_of = |m: &HashMap<String, f64>, prefix: &str, n: usize| -> Vec<Value> {
        let mut v: Vec<(&String, &f64)> = m.iter().filter(|(f, _)| f.starts_with(prefix)).collect();
        v.sort_by(|a, b| b.1.total_cmp(a.1));
        let max = v.first().map(|x| *x.1).unwrap_or(1.0).max(0.0001);
        v.into_iter().take(n).map(|(f, w)| json!({ "feature": f, "label": vocab::label(f), "weight": w / max })).collect()
    };
    let media_json: Value = Media::FEED
        .iter()
        .map(|m| {
            let feats = per_media.get(m.as_str());
            (m.as_str().to_string(), json!({ "genres": feats.map(|f| top_of(f, "g:", 6)).unwrap_or_default(), "themes": feats.map(|f| top_of(f, "t:", 8)).unwrap_or_default() }))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();

    // Taste DNA: the strongest shared tags, with how much each medium contributes.
    let dna: Vec<Value> = top_features(&conn, "t", 14)?
        .into_iter()
        .map(|mut t| {
            let f = t["feature"].as_str().unwrap_or("").to_string();
            let parts: Vec<f64> = Media::FEED.iter().map(|m| per_media.get(m.as_str()).and_then(|x| x.get(&f)).copied().unwrap_or(0.0).max(0.0)).collect();
            let total: f64 = parts.iter().sum::<f64>().max(0.0001);
            t["media"] = json!({ "movie": parts[0] / total, "tv": parts[1] / total, "game": parts[2] / total, "album": parts[3] / total });
            t["spans"] = json!(parts.iter().filter(|p| **p / total > 0.12).count());
            t
        })
        .collect();

    let creators: Vec<Value> = top_features(&conn, "c", 40)?
        .into_iter()
        .filter_map(|mut c| {
            let (name, role) = creator_names.get(c["feature"].as_str()?)?.clone();
            c["label"] = json!(name);
            c["role"] = json!(role);
            Some(c)
        })
        .take(16)
        .collect();

    let mut decade_list: Vec<Value> = decades.iter().map(|(d, m)| json!({ "decade": d, "media": m, "total": m.values().sum::<f64>() })).collect();
    decade_list.sort_by(|a, b| a["decade"].as_str().cmp(&b["decade"].as_str()));

    // How adventurous: how evenly the user's liking is spread across genres (normalised entropy).
    let mut genre_mass: HashMap<&String, f64> = HashMap::new();
    for feats in per_media.values() {
        for (f, w) in feats.iter().filter(|(f, w)| f.starts_with("g:") && **w > 0.0) {
            *genre_mass.entry(f).or_insert(0.0) += w;
        }
    }
    let total: f64 = genre_mass.values().sum();
    let adventurous = if genre_mass.len() < 2 || total <= 0.0 {
        None
    } else {
        let entropy: f64 = genre_mass.values().map(|w| w / total).map(|p| -p * p.ln()).sum();
        Some((entropy / (genre_mass.len().min(20) as f64).ln() * 100.0).clamp(0.0, 100.0).round())
    };

    let overrides: Vec<Value> = {
        let mut stmt = conn.prepare("SELECT feature, mode FROM taste_overrides ORDER BY mode, feature")?;
        let rows = stmt
            .query_map([], |r| {
                let f: String = r.get(0)?;
                Ok((f, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|(f, mode)| json!({ "label": creator_names.get(&f).map(|c| c.0.clone()).unwrap_or_else(|| vocab::label(&f)), "feature": f, "mode": mode })).collect()
    };
    let disliked: Vec<Value> = {
        let mut stmt = conn.prepare("SELECT feature, weight FROM taste_profile WHERE weight < -0.15 AND (feature LIKE 'g:%' OR feature LIKE 't:%') ORDER BY weight ASC LIMIT 8")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|(f, w)| json!({ "label": vocab::label(&f), "feature": f, "weight": w })).collect()
    };
    let source_counts: Vec<Value> = {
        let mut stmt = conn.prepare("SELECT source, count(*) FROM user_events GROUP BY source ORDER BY count(*) DESC")?;
        let rows = stmt.query_map([], |r| Ok(json!({ "source": r.get::<_, String>(0)?, "events": r.get::<_, i64>(1)?, })))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    drop(conn);

    Ok(json!({
        "empty": affinity.is_empty(),
        "media": media_json,
        "dna": dna,
        "creators": creators,
        "decades": decade_list,
        "adventurous": adventurous,
        "dial": settings.dial,
        "source_weights": settings.source_weights,
        "sources": source_counts,
        "overrides": overrides,
        "disliked": disliked,
        "stats": stats(ctx)?,
        "vocabulary": vocab::TAGS.iter().map(|(t, l)| json!({ "feature": format!("t:{t}"), "label": l })).collect::<Vec<_>>(),
    }))
}

/// Hours watched / played / listened per month, top of the year, and streaks.
pub fn stats(ctx: &Ctx) -> Result<Value> {
    let conn = ctx.db.lock();
    let year = chrono::Datelike::year(&chrono::Local::now());
    // Watching: runtime of everything marked watched, by the month it was watched.
    let mut months: std::collections::BTreeMap<String, (f64, f64)> = std::collections::BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT substr(e.at, 1, 7), i.media, sum(COALESCE(i.runtime, CASE i.media WHEN 'movie' THEN 110 ELSE 45 END) * CASE i.media WHEN 'tv' THEN max(1, COALESCE(json_extract(e.meta, '$.plays'), 1)) ELSE 1 END)
             FROM user_events e JOIN items i ON i.id = e.item_id
             WHERE e.kind = 'watched' AND i.media IN ('movie', 'tv') AND e.at >= ?1 GROUP BY 1, 2",
        )?;
        let since = (chrono::Local::now() - chrono::Duration::days(365)).format("%Y-%m").to_string();
        for row in stmt.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(2)?)))? {
            let (month, minutes) = row?;
            months.entry(month).or_insert((0.0, 0.0)).0 += minutes / 60.0;
        }
    }
    // Listening: Last.fm scrobbles per month at an average track length of 3.5 minutes.
    let lastfm = store::source_config(&conn, "lastfm");
    for m in lastfm["months"].as_array().into_iter().flatten() {
        if let (Some(month), Some(n)) = (m["month"].as_str(), m["scrobbles"].as_f64()) {
            months.entry(month.to_string()).or_insert((0.0, 0.0)).1 += n * 3.5 / 60.0;
        }
    }
    let monthly: Vec<Value> = months.iter().map(|(m, (w, l))| json!({ "month": m, "watched": (w * 10.0).round() / 10.0, "listened": (l * 10.0).round() / 10.0 })).collect();

    // Playing: Steam reports lifetime and last-two-weeks totals, not a monthly history.
    let (played_total, played_recent): (f64, f64) = conn.query_row(
        "SELECT COALESCE(sum(e.value), 0) / 60.0, COALESCE(sum(json_extract(e.meta, '$.recent_minutes')), 0) / 60.0 FROM user_events e JOIN items i ON i.id = e.item_id WHERE e.kind = 'play' AND i.media = 'game'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let top = |media: &str| -> Result<Vec<Card>> {
        cards(
            &conn,
            "JOIN user_events e ON e.item_id = i.id WHERE i.media = ?1 AND e.kind = 'rating' AND e.at >= ?2 GROUP BY i.id ORDER BY max(e.value) DESC, max(e.at) DESC LIMIT 5",
            &[&media, &format!("{year}-01-01")],
        )
    };
    let top_year = json!({ "movie": top("movie")?, "tv": top("tv")?, "game": top("game")?, "album": top("album")? });

    // Streak: consecutive weeks (ending this week or last) with something watched, played or rated.
    let mut weeks: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT DISTINCT CAST(julianday(substr(at, 1, 10)) / 7 AS INTEGER) FROM user_events WHERE kind IN ('watched', 'rating', 'play') AND source != 'import' ORDER BY 1 DESC")?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    weeks.dedup();
    let this_week: i64 = conn.query_row("SELECT CAST(julianday('now') / 7 AS INTEGER)", [], |r| r.get(0))?;
    let mut current = 0;
    if weeks.first().map(|w| this_week - w <= 1).unwrap_or(false) {
        current = 1;
        for pair in weeks.windows(2) {
            if pair[0] - pair[1] == 1 { current += 1 } else { break }
        }
    }
    let mut longest = if weeks.is_empty() { 0 } else { 1 };
    let mut run = 1;
    for pair in weeks.windows(2) {
        run = if pair[0] - pair[1] == 1 { run + 1 } else { 1 };
        longest = longest.max(run);
    }
    let counts: Value = {
        let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0);
        json!({
            "rated": n("SELECT count(DISTINCT item_id) FROM user_events WHERE kind = 'rating'"),
            "watched": n("SELECT count(DISTINCT e.item_id) FROM user_events e JOIN items i ON i.id = e.item_id WHERE e.kind = 'watched' AND i.media IN ('movie', 'tv')"),
            "games": n("SELECT count(DISTINCT e.item_id) FROM user_events e JOIN items i ON i.id = e.item_id WHERE e.kind IN ('owned', 'play') AND i.media = 'game'"),
            "artists": n("SELECT count(DISTINCT e.item_id) FROM user_events e JOIN items i ON i.id = e.item_id WHERE i.media = 'artist'"),
        })
    };
    Ok(json!({
        "monthly": monthly,
        "played_hours": played_total.round(),
        "played_recent_hours": (played_recent * 10.0).round() / 10.0,
        "top_year": top_year,
        "year": year,
        "streak_weeks": current,
        "longest_streak_weeks": longest,
        "counts": counts,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::{ctx, TestCtx};
    use crate::store::{record_event, upsert_item};

    fn add(t: &TestCtx, media: Media, title: &str, days: i64, genres: &[&str], tags: &[&str], pop: f64, f: impl FnOnce(&mut ItemDraft)) -> i64 {
        let mut d = ItemDraft::new(media, title).id("test", title);
        d.release_date = Some(days_from_today(days));
        d.genres = genres.iter().map(|s| s.to_string()).collect();
        d.tags = tags.iter().map(|s| s.to_string()).collect();
        d.popularity = pop;
        d.poster = Some("https://img/p.jpg".into());
        d.backdrop = Some("https://img/b.jpg".into());
        f(&mut d);
        upsert_item(&t.ctx.db.lock(), &d).unwrap()
    }

    /// A small world: a sci-fi fan with a few upcoming titles in every medium.
    fn world() -> (TestCtx, HashMap<&'static str, i64>) {
        let t = ctx();
        let mut ids = HashMap::new();
        let loved = add(&t, Media::Movie, "Arrival", -3000, &["Science Fiction"], &["space", "slow burn"], 50.0, |_| {});
        record_event(&t.ctx.db.lock(), loved, "marquee", EventKind::Rating, 5.0, None, None, &Value::Null).unwrap();
        ids.insert("scifi_film", add(&t, Media::Movie, "Signal Lost", 1, &["Science Fiction"], &["space"], 80.0, |_| {}));
        ids.insert("romcom", add(&t, Media::Movie, "Meet Cute", 1, &["Romance", "Comedy"], &[], 90.0, |_| {}));
        ids.insert("game", add(&t, Media::Game, "Void Drift", 20, &["Adventure"], &["space", "exploration"], 30.0, |d| {
            d.set("platforms", json!(["PC", "PS5"]));
            d.set("price", json!({ "cents": 1999, "text": "$19.99" }));
            d.set("discount", 30);
        }));
        ids.insert("album", add(&t, Media::Album, "Neon Tides", 5, &[], &["synthwave", "space"], 5.0, |d| {
            d.set("artist", "Lumen Drift");
            d.set("album_type", "album");
        }));
        ids.insert("show", add(&t, Media::Tv, "Severance", -1400, &["Sci-Fi & Fantasy"], &["dystopia"], 70.0, |d| {
            d.set("next_episode", json!({ "air_date": days_from_today(3), "season": 3, "episode": 1 }));
        }));
        record_event(&t.ctx.db.lock(), ids["show"], "trakt", EventKind::Watched, 1.0, None, None, &Value::Null).unwrap();
        ids.insert("old", add(&t, Media::Movie, "Old Film", -400, &["Science Fiction"], &["space"], 10.0, |_| {}));
        for n in 0..12 {
            add(&t, Media::Movie, &format!("Filler {n}"), -200 - n, &["Drama"], &[], 20.0 + n as f64, |_| {});
        }
        crate::sync::rescore(&t.ctx).unwrap();
        (t, ids)
    }

    fn row_ids(home: &Value, row: &str) -> Vec<i64> {
        home["rows"].as_array().unwrap().iter().find(|r| r["id"] == row).map(|r| r["cards"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect()).unwrap_or_default()
    }

    #[test]
    fn home_rows_put_matches_first_and_leave_out_empty_rows() {
        let (t, ids) = world();
        let home = home(&t.ctx).unwrap();
        assert_eq!(home["personal"], true);
        // "Out This Week" depends on where today falls in the week, so check Coming Soon and the rest.
        let soon = row_ids(&home, "coming-soon");
        let week = row_ids(&home, "this-week");
        let all: Vec<i64> = soon.iter().chain(week.iter()).copied().collect();
        assert!(all.contains(&ids["scifi_film"]) && all.contains(&ids["game"]) && all.contains(&ids["album"]));
        // Wherever the sci-fi film and the romcom share a row, the match leads despite lower popularity.
        for r in [&soon, &week] {
            if let (Some(a), Some(b)) = (r.iter().position(|i| *i == ids["scifi_film"]), r.iter().position(|i| *i == ids["romcom"])) {
                assert!(a < b);
            }
        }
        assert_eq!(row_ids(&home, "new-seasons"), vec![ids["show"]]);
        assert_eq!(row_ids(&home, "artists"), Vec::<i64>::new(), "no followed artists, so the row is absent");
        assert!(home["rows"].as_array().unwrap().iter().all(|r| !r["cards"].as_array().unwrap().is_empty()));
        assert!(!home["hero"].as_array().unwrap().is_empty());
        let card = home["rows"][0]["cards"][0].clone();
        assert!(card["match_pct"].is_number() && card["title"].is_string() && card["media"].is_string());
    }

    #[test]
    fn with_no_history_home_falls_back_to_popularity_and_hides_match_rings() {
        let t = ctx();
        add(&t, Media::Movie, "Big", 30, &["Action"], &[], 99.0, |_| {});
        add(&t, Media::Movie, "Small", 30, &["Drama"], &[], 1.0, |_| {});
        crate::sync::rescore(&t.ctx).unwrap();
        let home = home(&t.ctx).unwrap();
        assert_eq!(home["personal"], false);
        let soon = &home["rows"].as_array().unwrap().iter().find(|r| r["id"] == "coming-soon").unwrap()["cards"];
        assert_eq!(soon[0]["title"], "Big");
        assert!(soon[0]["match_pct"].is_null());
    }

    #[test]
    fn for_you_filters_pages_and_hides_what_you_have_seen() {
        let (t, ids) = world();
        let all = for_you(&t.ctx, &Filters::default(), 0, 5).unwrap();
        assert_eq!(all["cards"].as_array().unwrap().len(), 5);
        assert_eq!(all["next"], 5);
        let every: Vec<i64> = for_you(&t.ctx, &Filters::default(), 0, 100).unwrap()["cards"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect();
        assert!(!every.contains(&ids["show"]), "already watching it");
        let games = for_you(&t.ctx, &Filters { media: vec!["game".into()], on_sale: true, max_price: Some(2500), platform: Some("ps5".into()), ..Default::default() }, 0, 10).unwrap();
        assert_eq!(games["cards"].as_array().unwrap().len(), 1);
        assert_eq!(games["cards"][0]["id"], ids["game"]);
        assert_eq!(games["next"], Value::Null);
        let none = for_you(&t.ctx, &Filters { media: vec!["game".into()], max_price: Some(500), ..Default::default() }, 0, 10).unwrap();
        assert!(none["cards"].as_array().unwrap().is_empty());
        let upcoming_scifi = for_you(&t.ctx, &Filters { genres: vec!["sci-fi".into()], status: "upcoming".into(), ..Default::default() }, 0, 10).unwrap();
        assert_eq!(upcoming_scifi["cards"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_streaming_filter_uses_the_services_the_user_has() {
        let (t, _) = world();
        let id = add(&t, Media::Movie, "On Paramount", -10, &["Drama"], &[], 5.0, |d| d.set("providers", json!({ "stream": [{ "id": 531, "name": "Paramount Plus" }] })));
        crate::sync::rescore(&t.ctx).unwrap();
        let f = Filters { my_services: true, ..Default::default() };
        assert!(for_you(&t.ctx, &f, 0, 10).unwrap()["cards"].as_array().unwrap().is_empty(), "no services chosen yet");
        store::set_setting(&t.ctx.db, "my_providers", &vec![531, 8]).unwrap();
        let got = for_you(&t.ctx, &f, 0, 10).unwrap();
        assert_eq!(got["cards"].as_array().unwrap().len(), 1);
        assert_eq!(got["cards"][0]["id"], id);
    }

    #[test]
    fn hubs_show_new_and_upcoming_with_facets() {
        let (t, ids) = world();
        let movies = hub(&t.ctx, Media::Movie, &Filters { sort: "date".into(), ..Default::default() }, 0, 50).unwrap();
        let listed: Vec<i64> = movies["cards"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect();
        assert!(listed.contains(&ids["scifi_film"]) && !listed.contains(&ids["old"]), "nothing older than four months by default");
        assert!(movies["genres"].as_array().unwrap().iter().any(|g| g["id"] == "sci-fi"));
        let games = hub(&t.ctx, Media::Game, &Filters::default(), 0, 50).unwrap();
        assert!(games["platforms"].as_array().unwrap().contains(&json!("PS5")));
        let old = hub(&t.ctx, Media::Movie, &Filters { status: "released".into(), ..Default::default() }, 0, 50).unwrap();
        assert!(old["cards"].as_array().unwrap().iter().any(|c| c["id"] == ids["old"]));
    }

    #[test]
    fn the_calendar_scopes_and_lists_episodes() {
        let (t, ids) = world();
        let (from, to) = (days_from_today(-1), days_from_today(30));
        let taste = calendar(&t.ctx, &from, &to, "taste").unwrap();
        assert!(taste.iter().any(|e| e.card.id == ids["scifi_film"]));
        assert!(!taste.iter().any(|e| e.card.id == ids["romcom"]), "a poor match isn't on the taste calendar");
        let ep = taste.iter().find(|e| e.card.id == ids["show"]).unwrap();
        assert_eq!((ep.kind, ep.label.as_str()), ("premiere", "Season 3 premiere"));
        assert!(taste.windows(2).all(|w| w[0].date <= w[1].date));
        let everything = calendar(&t.ctx, &from, &to, "all").unwrap();
        assert!(everything.iter().any(|e| e.card.id == ids["romcom"]));
        // Following pins it to the "lists" scope.
        assert!(!calendar(&t.ctx, &from, &to, "lists").unwrap().iter().any(|e| e.card.id == ids["romcom"]));
        store::follow(&t.ctx.db.lock(), "item", &ids["romcom"].to_string(), "Meet Cute", Some(ids["romcom"])).unwrap();
        let mine = calendar(&t.ctx, &from, &to, "lists").unwrap();
        assert!(mine.iter().any(|e| e.card.id == ids["romcom"] && e.pinned));
    }

    #[test]
    fn quick_rate_mixes_media_and_skips_what_is_rated() {
        let (t, ids) = world();
        let deck = quick_rate(&t.ctx, 8).unwrap();
        assert!(!deck.is_empty());
        assert_eq!(deck[0].id, ids["show"], "something watched but unrated comes first");
        assert!(!deck.iter().any(|c| c.title == "Arrival"), "already rated");
        assert!(deck.iter().all(|c| c.release_date.as_deref().unwrap() <= today().as_str() || c.id == ids["show"]), "you can't rate what isn't out");
    }

    #[tokio::test]
    async fn detail_pages_work_offline_from_what_is_stored() {
        let (t, ids) = world();
        t.net.set_offline(true);
        let d = detail(&t.ctx, ids["game"]).await.unwrap();
        assert_eq!(d["item"]["title"], "Void Drift");
        assert!(d["match_pct"].as_i64().unwrap() > 60);
        assert!(d["why"][0].as_str().unwrap().contains("Arrival"));
        // Cross-media: a space game suggests the space film and album.
        let cross = &d["similar"]["cross"];
        assert!(cross["movie"].as_array().unwrap().iter().any(|c| c["title"] == "Signal Lost" || c["title"] == "Arrival"));
        assert!(cross["album"].as_array().unwrap().iter().any(|c| c["title"] == "Neon Tides"));
        assert_eq!(d["where"]["price"]["cents"], 1999);
        let show = detail(&t.ctx, ids["show"]).await.unwrap();
        assert!(show["targets"].as_array().unwrap().iter().any(|x| x["id"] == "trakt:watchlist" && x["ready"] == false));
    }

    #[test]
    fn the_taste_overview_describes_the_profile() {
        let (t, _) = world();
        let o = taste_overview(&t.ctx).unwrap();
        assert_eq!(o["empty"], false);
        assert_eq!(o["media"]["movie"]["genres"][0]["label"], "sci-fi");
        assert!(o["dna"].as_array().unwrap().iter().any(|d| d["feature"] == "t:space"));
        assert!(o["decades"].as_array().unwrap().iter().any(|d| d["total"].as_f64().unwrap() > 0.0));
        assert_eq!(o["stats"]["counts"]["rated"], 1);
        assert!(o["stats"]["streak_weeks"].as_i64().unwrap() >= 1);
        assert!(o["vocabulary"].as_array().unwrap().len() > 50);
    }
}
