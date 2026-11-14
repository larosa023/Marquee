//! The catalog: what exists and when it comes out. One module per metadata source, plus the
//! glue here that keeps the local catalog fresh and fills in details on demand.

pub mod igdb;
pub mod imdb;
pub mod lastfm;
pub mod musicbrainz;
pub mod steam_store;
pub mod tmdb;
pub mod tvmaze;

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use serde_json::{json, Value};

/// Combines two drafts of the same title. `a` wins where both have a value; genres, tags,
/// creators and ids are unioned so one source's detail fetch never erases another's features.
pub fn merge(mut a: ItemDraft, b: ItemDraft) -> ItemDraft {
    for id in b.ids {
        if !a.ids.contains(&id) {
            a.ids.push(id);
        }
    }
    for g in b.genres {
        if !a.genres.iter().any(|x| x.eq_ignore_ascii_case(&g)) {
            a.genres.push(g);
        }
    }
    for t in b.tags {
        if !a.tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
            a.tags.push(t);
        }
    }
    for c in b.creators {
        if !a.creators.contains(&c) {
            a.creators.push(c);
        }
    }
    a.release_date = a.release_date.or(b.release_date);
    a.poster = a.poster.or(b.poster);
    a.backdrop = a.backdrop.or(b.backdrop);
    a.overview = a.overview.filter(|o| !o.is_empty()).or(b.overview);
    a.runtime = a.runtime.or(b.runtime);
    a.rating = a.rating.or(b.rating);
    a.year = a.year.or(b.year);
    a.language = a.language.or(b.language);
    a.popularity = a.popularity.max(b.popularity);
    for (k, v) in b.data {
        a.data.entry(k).or_insert(v);
    }
    if a.similar.is_empty() {
        a.similar = b.similar;
    }
    a.detail_level = a.detail_level.max(b.detail_level);
    a.ttl_secs = a.ttl_secs.min(b.ttl_secs);
    a
}

pub fn save(ctx: &Ctx, d: &ItemDraft) -> Result<i64> {
    store::upsert_item(&ctx.db.lock(), d)
}

pub fn save_all(ctx: &Ctx, drafts: &[ItemDraft]) -> Result<Vec<i64>> {
    let conn = ctx.db.lock();
    let tx = conn.unchecked_transaction()?;
    let mut ids = Vec::with_capacity(drafts.len());
    for d in drafts {
        if let Ok(id) = store::upsert_item(&tx, d) {
            ids.push(id);
        }
    }
    tx.commit()?;
    Ok(ids)
}

/// Fetches full details for an item if the stored copy is a stub or has gone stale.
/// Works offline: a failed fetch leaves whatever is cached.
pub async fn ensure_detail(ctx: &Ctx, item_id: i64) -> Result<()> {
    let (item, stale) = {
        let conn = ctx.db.lock();
        (store::get_item(&conn, item_id)?, store::item_is_stale(&conn, item_id))
    };
    if !stale {
        return Ok(());
    }
    let draft = match item.media {
        Media::Movie | Media::Tv => movie_or_show(ctx, &item).await?,
        Media::Game => game(ctx, &item).await?,
        Media::Album => album(ctx, &item).await?,
        Media::Artist => artist(ctx, &item).await?,
    };
    if let Some(mut d) = draft {
        // Make sure the draft lands on this item even if the source knows it by another id.
        for (source, ext) in item.ids.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))) {
            if !d.ids.iter().any(|(s, _)| *s == source) {
                d.ids.insert(0, (source, ext));
            }
        }
        let conn = ctx.db.lock();
        let saved = store::upsert_item_at(&conn, &d, Some(item_id))?;
        if item.media == Media::Game {
            if let (Some(cents), Some(currency)) = (d.data.get("price").and_then(|p| p["cents"].as_i64()), d.data.get("price").and_then(|p| p["currency"].as_str())) {
                if cents > 0 {
                    let discount = d.data.get("discount").and_then(|x| x.as_i64()).unwrap_or(0);
                    let local = steam_store::track_price(&conn, saved, cents, discount, currency)?;
                    // Keep an IsThereAnyDeal low if it is lower than anything seen locally.
                    let best = match d.data.get("low") {
                        Some(itad) if itad["cents"].as_i64().map(|c| c <= local["cents"].as_i64().unwrap_or(i64::MAX)).unwrap_or(false) => itad.clone(),
                        _ => local,
                    };
                    conn.execute("UPDATE items SET data = json_set(data, '$.low', json(?1)) WHERE id = ?2", rusqlite::params![best.to_string(), saved])?;
                }
            }
        }
    }
    Ok(())
}

async fn movie_or_show(ctx: &Ctx, item: &Item) -> Result<Option<ItemDraft>> {
    // Without a TMDB key, shows still get details, posters and episodes from TVmaze (no key).
    if item.media == Media::Tv && !ctx.has_key("tmdb") {
        if item.data["mal_sequel"] == true {
            return Ok(None);
        }
        let id = match item.ext("tvmaze") {
            Some(id) => Some(id.to_string()),
            None => tvmaze::find(ctx, item.ext("imdb"), item.ext("tvdb"), &item.title, item.year).await?,
        };
        return match id {
            Some(id) => Ok(Some(tvmaze::details(ctx, &id).await?)),
            None => Ok(None),
        };
    }
    // Not "no match": the caller must know to try again once the key is added.
    ctx.require_key("tmdb")?;
    // A MyAnimeList sequel season ("... Season 2") is news as itself. Matching it to its show on
    // TMDB would swap in the show's title and first-air date, so it keeps MAL's details.
    if item.data["mal_sequel"] == true && item.ext("tmdb").is_none() {
        return Ok(None);
    }
    let tmdb_id = match item.ext("tmdb") {
        Some(id) => Some(id.to_string()),
        None => match item.ext("imdb") {
            Some(imdb) => tmdb::find_by_imdb(ctx, imdb).await?.filter(|d| d.media() == item.media).and_then(|d| d.ids.first().map(|i| i.1.clone())),
            None => {
                let hits = tmdb::search(ctx, &item.title, Some(item.media), item.year).await?;
                let hits = if hits.is_empty() && item.year.is_some() { tmdb::search(ctx, &item.title, Some(item.media), None).await? } else { hits };
                hits.into_iter().next().and_then(|d| d.ids.first().map(|i| i.1.clone()))
            }
        },
    };
    match tmdb_id {
        Some(id) => Ok(Some(tmdb::details(ctx, item.media, &id).await?)),
        None => Ok(None),
    }
}

async fn game(ctx: &Ctx, item: &Item) -> Result<Option<ItemDraft>> {
    let mut draft: Option<ItemDraft> = None;
    if igdb::configured(ctx) {
        let found = match (item.ext("igdb"), item.ext("steam")) {
            (Some(id), _) => igdb::details(ctx, id).await.ok(),
            (None, Some(app)) => match igdb::by_steam_ids(ctx, &[app.to_string()]).await.ok().and_then(|v| v.into_iter().next()) {
                Some(stub) => igdb::details(ctx, &stub.ids[0].1).await.ok(),
                None => None,
            },
            (None, None) => match igdb::search(ctx, &item.title, 3).await.ok().and_then(|v| v.into_iter().next()) {
                Some(stub) => igdb::details(ctx, &stub.ids[0].1).await.ok(),
                None => None,
            },
        };
        draft = found;
    }
    let steam_id = item.ext("steam").map(String::from).or_else(|| draft.as_ref().and_then(|d| d.ids.iter().find(|i| i.0 == "steam").map(|i| i.1.clone())));
    if let Some(app) = steam_id {
        if let Ok(Some(mut store_draft)) = steam_store::app_details(ctx, &app).await {
            store_draft.set("deck", steam_store::deck_status(ctx, &app).await.unwrap_or("unknown"));
            if let Ok(Some(r)) = steam_store::reviews(ctx, &app).await {
                store_draft.set("reviews", r);
            }
            if let Ok(Some(low)) = steam_store::itad_low(ctx, &app).await {
                store_draft.set("low", low);
            }
            draft = Some(match draft {
                // IGDB has the richer themes; Steam has the price. Price keys must come from Steam.
                Some(mut d) => {
                    for key in ["price", "discount", "deck", "reviews", "low", "store_url", "header", "coming_soon", "release_text"] {
                        if let Some(v) = store_draft.data.remove(key) {
                            d.data.insert(key.into(), v);
                        }
                    }
                    merge(d, store_draft)
                }
                None => store_draft,
            });
        }
    }
    Ok(draft)
}

async fn album(ctx: &Ctx, item: &Item) -> Result<Option<ItemDraft>> {
    let artist = item.data["artist"].as_str().unwrap_or("").to_string();
    let mbid = match item.ext("mbid") {
        Some(m) => Some(m.to_string()),
        None if !artist.is_empty() => musicbrainz::find_album(ctx, &artist, &item.title).await.ok().flatten().and_then(|d| d.ids.first().map(|i| i.1.clone())),
        None => None,
    };
    let mut draft = match mbid {
        Some(m) => musicbrainz::details(ctx, &m).await.ok(),
        None => None,
    };
    if draft.is_none() {
        let mut d = ItemDraft::new(Media::Album, item.title.clone());
        d.detail_level = 1;
        if !artist.is_empty() {
            d.creators.push(Creator::new(artist.clone(), "artist"));
            d.set("artist", artist.clone());
        }
        draft = Some(d);
    }
    let mut d = draft.unwrap_or_default();
    // Last.fm's tags describe how music sounds far better than MusicBrainz genres do.
    if ctx.has_key("lastfm_key") && !artist.is_empty() {
        let mut tags = lastfm::album_tags(ctx, &artist, &item.title).await.unwrap_or_default();
        if tags.len() < 3 {
            tags.extend(lastfm::artist_tags(ctx, &artist).await.unwrap_or_default());
        }
        let mut merged = tags;
        for t in d.tags.drain(..) {
            if !merged.contains(&t) {
                merged.push(t);
            }
        }
        d.tags = merged;
        d.tag_source = TagSource::Lastfm;
        if let Ok(sim) = lastfm::similar_artists(ctx, &artist, 8).await {
            d.set("similar_artists", json!(sim.iter().map(|s| json!({ "name": s.0, "match": s.2 })).collect::<Vec<_>>()));
        }
    }
    Ok(Some(d))
}

async fn artist(ctx: &Ctx, item: &Item) -> Result<Option<ItemDraft>> {
    if !ctx.has_key("lastfm_key") {
        return Ok(None);
    }
    let mut d = ItemDraft::new(Media::Artist, item.title.clone());
    d.detail_level = 1;
    d.ttl_secs = 30 * 86_400;
    d.tags = lastfm::artist_tags(ctx, &item.title).await?;
    d.tag_source = TagSource::Lastfm;
    d.creators.push(Creator::new(item.title.clone(), "artist"));
    Ok(Some(d))
}

/// Searches every catalog and returns the ids of the stored results, best first.
pub async fn search_everything(ctx: &Ctx, query: &str, media: Option<Media>, limit: usize) -> Result<Vec<i64>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let want = |m: Media| media.is_none() || media == Some(m);
    let screen = async {
        if ctx.has_key("tmdb") && (want(Media::Movie) || want(Media::Tv)) {
            tmdb::search(ctx, q, media.filter(|m| matches!(m, Media::Movie | Media::Tv)), None).await
        } else {
            Ok(vec![])
        }
    };
    let games = async {
        if igdb::configured(ctx) && want(Media::Game) {
            igdb::search(ctx, q, 10).await
        } else {
            Ok(vec![])
        }
    };
    let music = async {
        if want(Media::Album) {
            musicbrainz::search_albums(ctx, q, 10).await
        } else {
            Ok(vec![])
        }
    };
    let (screen, games, music) = futures::join!(screen, games, music);
    let mut offline = false;
    let mut drafts: Vec<ItemDraft> = vec![];
    for r in [screen, games, music] {
        match r {
            Ok(list) => drafts.extend(list.into_iter().take(limit)),
            Err(e) => offline |= e.is_offline(),
        }
    }
    let remote = save_all(ctx, &drafts)?;

    // Local matches (titles already in the library, including imports no catalog knows).
    let conn = ctx.db.lock();
    let like = format!("%{}%", q.to_lowercase().replace('%', ""));
    let mut sql = "SELECT id FROM items WHERE media != 'artist' AND (sort_title LIKE ?1 OR lower(title) LIKE ?1 OR lower(json_extract(data, '$.artist')) LIKE ?1)".to_string();
    if let Some(m) = media {
        sql.push_str(&format!(" AND media = '{}'", m.as_str()));
    }
    sql.push_str(" ORDER BY (sort_title = ?2) DESC, (sort_title LIKE ?3) DESC, popularity DESC LIMIT 40");
    let mut stmt = conn.prepare(&sql)?;
    let local: Vec<i64> = stmt.query_map(rusqlite::params![like, sort_title(q), format!("{}%", sort_title(q))], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut ids = vec![];
    // Exact and prefix local matches first, then remote in source order, then the rest.
    for id in local.iter().take(3).chain(remote.iter()).chain(local.iter()) {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    if ids.is_empty() && offline {
        return Err(Error::Offline("the catalogs".into()));
    }
    ids.truncate(limit.max(1) * 3);
    Ok(ids)
}

/// Pulls well-known albums for a tag into the catalog (used by vibe search and hidden gems).
pub async fn albums_for_tag(ctx: &Ctx, tag: &str, limit: u32) -> Result<Vec<i64>> {
    let albums = lastfm::tag_albums(ctx, tag, limit).await?;
    let drafts: Vec<ItemDraft> = albums
        .into_iter()
        .filter_map(|a| {
            let mut d = ItemDraft::new(Media::Album, a.name?);
            if let Some(m) = a.mbid.filter(|m| !m.is_empty()) {
                d.ids.push(("lastfm".into(), m));
            }
            d.poster = a.image;
            d.creators.push(Creator::new(a.artist.clone(), "artist"));
            d.set("artist", a.artist);
            d.set("album_type", "album");
            d.tags = vec![tag.to_string()];
            d.tag_source = TagSource::Lastfm;
            Some(d)
        })
        .collect();
    save_all(ctx, &drafts)
}

#[derive(Default, Debug, serde::Serialize)]
pub struct RefreshReport {
    pub movies: usize,
    pub shows: usize,
    pub games: usize,
    pub albums: usize,
    pub skipped: Vec<String>,
}

/// Brings in what's new and upcoming for every medium. Each medium is independent: one failing
/// (or missing its key) never blocks the others.
pub async fn refresh(ctx: &Ctx, progress: &(dyn Fn(&str, f64) + Send + Sync)) -> RefreshReport {
    let mut report = RefreshReport::default();
    let d = days_from_today;
    let mut offline = false;

    progress("Checking new movies", 0.05);
    if ctx.has_key("tmdb") {
        let (m1, m2, m3, m4) = (d(-45), d(0), d(1), d(150));
        let (a, b, c) = futures::join!(tmdb::movies_between(ctx, &m1, &m2, 3), tmdb::movies_between(ctx, &m3, &m4, 4), tmdb::trending(ctx, Media::Movie));
        for r in [a, b, c] {
            match r {
                Ok(list) => report.movies += save_all(ctx, &list).map(|v| v.len()).unwrap_or(0),
                Err(e) => offline |= note(&mut report, "Movies", e),
            }
        }
        progress("Checking new shows", 0.2);
        let (s1, s2, s3, s4) = (d(-30), d(120), d(-3), d(10));
        let (a, b, c) = futures::join!(tmdb::shows_premiering(ctx, &s1, &s2, 3), tmdb::shows_airing(ctx, &s3, &s4, 2), tmdb::trending(ctx, Media::Tv));
        for r in [a, b, c] {
            match r {
                Ok(list) => report.shows += save_all(ctx, &list).map(|v| v.len()).unwrap_or(0),
                Err(e) => offline |= note(&mut report, "TV", e),
            }
        }
    } else {
        report.skipped.push("Movies need a TMDB key (Settings → Connections → API keys). TV comes from TVmaze until then.".into());
        progress("Checking new shows", 0.2);
        match tvmaze::premieres(ctx, -14, 30).await {
            Ok(list) => report.shows += save_all(ctx, &list).map(|v| v.len()).unwrap_or(0),
            Err(e) => offline |= note(&mut report, "TV", e),
        }
    }

    if ctx.has_key("mal_client_id") {
        progress("Checking this season's anime", 0.3);
        match crate::integrations::mal::current_and_next(ctx).await {
            Ok(list) => report.shows += save_all(ctx, &list).map(|v| v.len()).unwrap_or(0),
            Err(e) => offline |= note(&mut report, "Anime", e),
        }
    }

    progress("Checking new games", 0.35);
    if igdb::configured(ctx) {
        let now = chrono::Utc::now().timestamp();
        let (a, b) = futures::join!(igdb::games_between(ctx, now, now + 200 * 86_400, 150), igdb::recent_releases(ctx, now - 60 * 86_400, now, 100));
        for r in [a, b] {
            match r {
                Ok(list) => report.games += save_all(ctx, &list).map(|v| v.len()).unwrap_or(0),
                Err(e) => offline |= note(&mut report, "Games", e),
            }
        }
    } else {
        report.skipped.push("The games catalog needs an IGDB (Twitch) client ID and secret.".into());
    }

    progress("Checking new music", 0.5);
    if !offline {
        match refresh_music(ctx, progress).await {
            Ok(n) => report.albums = n,
            Err(e) => {
                note(&mut report, "Music", e);
            }
        }
    }
    report
}

fn note(report: &mut RefreshReport, what: &str, e: Error) -> bool {
    let offline = e.is_offline();
    let line = format!("{what}: {e}");
    if !report.skipped.contains(&line) {
        report.skipped.push(line);
    }
    offline
}

/// Artists whose releases Marquee watches: followed, most-played, and (scaled by the taste
/// dial) artists similar to those.
pub async fn music_artists(ctx: &Ctx) -> Result<Vec<(String, Option<String>)>> {
    let dial = crate::taste::TasteSettings::load(&ctx.db).dial;
    let mut artists: Vec<(String, Option<String>)> = {
        let conn = ctx.db.lock();
        let mut stmt = conn.prepare(
            "SELECT i.title, (SELECT ext_id FROM item_ids x WHERE x.item_id = i.id AND x.source = 'mbid' LIMIT 1), max(CASE e.kind WHEN 'like' THEN 400 WHEN 'pin' THEN 600 ELSE e.value END) AS w
             FROM items i JOIN user_events e ON e.item_id = i.id
             WHERE i.media = 'artist' AND e.kind IN ('play', 'like', 'pin', 'more_like')
             GROUP BY i.id ORDER BY w DESC LIMIT 60",
        )?;
        let mut rows: Vec<(String, Option<String>)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut follows = conn.prepare("SELECT name FROM follows WHERE kind = 'artist'")?;
        for name in follows.query_map([], |r| r.get::<_, String>(0))? {
            let name = name?;
            if !rows.iter().any(|r| r.0.eq_ignore_ascii_case(&name)) {
                rows.insert(0, (name, None));
            }
        }
        rows
    };
    if !ctx.has_key("lastfm_key") {
        return Ok(artists);
    }
    if artists.is_empty() {
        // No music history at all: start from what the world is listening to.
        return Ok(lastfm::chart_artists(ctx, 30).await.unwrap_or_default());
    }
    // Similar artists: 2 seeds when comfortable, up to 14 when adventurous.
    let seeds = (2.0 + dial * 12.0).round() as usize;
    let per_seed = (2.0 + dial * 4.0).round() as u32;
    let known: Vec<String> = artists.iter().map(|a| a.0.to_lowercase()).collect();
    let mut extra: Vec<(String, Option<String>)> = vec![];
    for (name, _) in artists.iter().take(seeds) {
        for (sim, mbid, _) in lastfm::similar_artists(ctx, name, per_seed + 2).await.unwrap_or_default() {
            if !known.contains(&sim.to_lowercase()) && !extra.iter().any(|e| e.0.eq_ignore_ascii_case(&sim)) && extra.len() < 30 {
                extra.push((sim, mbid.filter(|m| !m.is_empty())));
            }
        }
    }
    artists.extend(extra);
    Ok(artists)
}

async fn refresh_music(ctx: &Ctx, progress: &(dyn Fn(&str, f64) + Send + Sync)) -> Result<usize> {
    let artists = music_artists(ctx).await?;
    let total = artists.len().max(1) as f64;
    let window_start = days_from_today(-75);
    let mut saved = 0;
    for (n, (name, mbid)) in artists.iter().enumerate() {
        if n % 5 == 0 {
            progress("Checking new music", 0.5 + 0.3 * (n as f64 / total));
        }
        let mbid = match mbid.clone().filter(|m| !m.is_empty()) {
            Some(m) => m,
            None => match musicbrainz::find_artist(ctx, name).await {
                Ok(Some((m, _))) => m,
                Ok(None) => continue,
                Err(e) if e.is_offline() => return Err(e),
                Err(_) => continue,
            },
        };
        let groups = match musicbrainz::release_groups(ctx, &mbid, name).await {
            Ok(g) => g,
            Err(e) if e.is_offline() => return Err(e),
            Err(_) => continue,
        };
        // New and upcoming only; back catalogues arrive through listening history.
        let mut fresh: Vec<ItemDraft> = groups.into_iter().filter(|g| g.release_date.as_deref().map(|d| d >= window_start.as_str()).unwrap_or(false)).collect();
        if fresh.is_empty() {
            continue;
        }
        let tags = if ctx.has_key("lastfm_key") { lastfm::artist_tags(ctx, name).await.unwrap_or_default() } else { vec![] };
        for d in fresh.iter_mut() {
            for t in &tags {
                if !d.tags.contains(t) {
                    d.tags.push(t.clone());
                }
            }
            d.tag_source = TagSource::Lastfm;
        }
        saved += save_all(ctx, &fresh)?.len();
    }
    Ok(saved)
}

/// The catalog fields a "where to get it" panel needs, in one place.
pub fn where_to_get(item: &Item) -> Value {
    match item.media {
        Media::Movie | Media::Tv => json!({ "providers": item.data["providers"], "tmdb": item.ext("tmdb").map(|id| format!("https://www.themoviedb.org/{}/{id}", if item.media == Media::Tv { "tv" } else { "movie" })) }),
        Media::Game => {
            json!({ "price": item.data["price"], "discount": item.data["discount"], "low": item.data["low"], "deck": item.data["deck"], "reviews": item.data["reviews"], "store_url": item.data["store_url"], "steam_uri": item.ext("steam").map(|a| format!("steam://store/{a}")), "platforms": item.data["platforms"], "release_dates": item.data["release_dates"] })
        }
        Media::Album | Media::Artist => {
            let artist = item.data["artist"].as_str().unwrap_or("");
            let search = format!("https://open.spotify.com/search/{}", urlencoding::encode(&format!("{artist} {}", item.title)));
            json!({
                "spotify": item.data["spotify_url"].as_str().map(String::from).or_else(|| item.ext("spotify").map(|id| format!("https://open.spotify.com/album/{id}"))).unwrap_or(search),
                "spotify_id": item.ext("spotify"),
                "lastfm": format!("https://www.last.fm/music/{}/{}", urlencoding::encode(artist), urlencoding::encode(&item.title)),
                "musicbrainz": item.ext("mbid").map(|m| format!("https://musicbrainz.org/release-group/{m}")),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn merging_unions_features_and_keeps_the_primary_text() {
        let mut a = ItemDraft::new(Media::Game, "Outer Wilds").id("igdb", 11737);
        a.genres = vec!["Adventure".into()];
        a.tags = vec!["space".into()];
        a.overview = Some("IGDB text".into());
        let mut b = ItemDraft::new(Media::Game, "Outer Wilds").id("steam", 753640);
        b.genres = vec!["adventure".into(), "Action".into()];
        b.overview = Some("Steam text".into());
        b.set("price", json!({ "cents": 1499 }));
        let m = merge(a, b);
        assert_eq!(m.genres, vec!["Adventure", "Action"]);
        assert_eq!(m.overview.as_deref(), Some("IGDB text"));
        assert_eq!(m.ids.len(), 2);
        assert_eq!(m.data["price"]["cents"], 1499);
    }

    #[tokio::test]
    async fn a_game_detail_combines_igdb_themes_with_steam_price() {
        let t = ctx();
        let id = save(&t.ctx, &ItemDraft::new(Media::Game, "Outer Wilds").id("steam", 753640)).unwrap();
        t.net.on("id.twitch.tv/oauth2/token", 200, r#"{"access_token":"tw","expires_in":5000000}"#);
        t.net.on("api.igdb.com/v4/external_games", 200, r#"[{"uid":"753640","url":"https://store.steampowered.com/app/753640","game":11737}]"#);
        t.net.on("api.igdb.com/v4/games", 200, include_str!("../../tests/fixtures/igdb_games.json"));
        t.net.on("store.steampowered.com/api/appdetails", 200, include_str!("../../tests/fixtures/steam_appdetails.json"));
        t.net.on("ajaxgetdeckappcompatibilityreport", 200, r#"{"results":{"resolved_category":3}}"#);
        t.net.on("appreviews/753640", 200, r#"{"query_summary":{"review_score_desc":"Overwhelmingly Positive","total_positive":95,"total_reviews":100}}"#);
        t.net.on("isthereanydeal.com/games/lookup", 200, r#"{"found":false}"#);
        ensure_detail(&t.ctx, id).await.unwrap();
        let conn = t.ctx.db.lock();
        let item = store::get_item(&conn, id).unwrap();
        assert_eq!(item.detail_level, 1);
        assert_eq!(item.ext("igdb"), Some("11737"));
        assert_eq!(item.data["price"]["cents"], 1499);
        assert_eq!(item.data["deck"], "verified");
        assert_eq!(item.data["low"]["cents"], 1499, "the first price seen is the local low");
        let feats: Vec<String> = store::item_features(&conn, id).unwrap().into_iter().map(|f| f.0).collect();
        assert!(feats.contains(&"t:space".to_string()), "IGDB keywords survive the Steam merge: {feats:?}");
        assert!(feats.contains(&"c:mobius-digital".to_string()));
        assert!(!store::item_is_stale(&conn, id));
    }

    #[tokio::test]
    async fn an_imported_film_with_only_a_title_is_matched_on_tmdb() {
        let t = ctx();
        let mut d = ItemDraft::new(Media::Movie, "Arrival");
        d.year = Some(2016);
        let id = save(&t.ctx, &d).unwrap();
        t.net.on("/3/search/movie", 200, r#"{"results":[{"id":329865,"title":"Arrival","release_date":"2016-11-11","genre_ids":[878]}]}"#);
        t.net.on("/3/movie/329865", 200, include_str!("../../tests/fixtures/tmdb_movie.json"));
        ensure_detail(&t.ctx, id).await.unwrap();
        let item = store::get_item(&t.ctx.db.lock(), id).unwrap();
        assert_eq!(item.ext("tmdb"), Some("329865"));
        assert_eq!(item.data["director"], "Denis Villeneuve");
        assert!(t.net.last("/3/search/movie").unwrap().url.contains("year=2016"));
    }

    #[tokio::test]
    async fn offline_detail_requests_fail_softly_and_keep_the_stub() {
        let t = ctx();
        let id = save(&t.ctx, &ItemDraft::new(Media::Movie, "Arrival").id("tmdb", 329865)).unwrap();
        t.net.set_offline(true);
        assert!(ensure_detail(&t.ctx, id).await.unwrap_err().is_offline());
        assert_eq!(store::get_item(&t.ctx.db.lock(), id).unwrap().title, "Arrival");
    }

    #[tokio::test]
    async fn search_mixes_catalog_results_with_the_local_library() {
        let t = ctx();
        let local = save(&t.ctx, &ItemDraft::new(Media::Game, "Arrival: The Game (import)")).unwrap();
        t.net.on("/3/search/multi", 200, r#"{"results":[{"id":329865,"media_type":"movie","title":"Arrival","release_date":"2016-11-11","genre_ids":[878]},{"id":1,"media_type":"person","name":"Arrival Person"}]}"#);
        t.net.on("id.twitch.tv/oauth2/token", 200, r#"{"access_token":"tw","expires_in":5000000}"#);
        t.net.on("api.igdb.com/v4/games", 200, "[]");
        t.net.on("musicbrainz.org/ws/2/release-group", 200, r#"{"release-groups":[]}"#);
        let ids = search_everything(&t.ctx, "arrival", None, 10).await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&local));
        // Offline with nothing local is an offline error, not "no results".
        let t2 = ctx();
        t2.net.set_offline(true);
        assert!(search_everything(&t2.ctx, "arrival", None, 10).await.unwrap_err().is_offline());
    }

    #[tokio::test]
    async fn refresh_reports_missing_keys_instead_of_failing() {
        let t = ctx();
        let bare = Ctx::new(t.ctx.db.clone(), t.ctx.http.clone(), t.ctx.secrets.clone(), t.host.clone(), t.dir.clone(), "0.1.0", Default::default());
        let report = refresh(&bare, &|_, _| {}).await;
        assert_eq!(report.movies + report.shows + report.games, 0);
        assert!(report.skipped.iter().any(|s| s.contains("TMDB")));
        assert!(report.skipped.iter().any(|s| s.contains("IGDB")));
    }

    #[tokio::test]
    async fn music_refresh_keeps_only_recent_and_upcoming_releases() {
        let t = ctx();
        {
            let conn = t.ctx.db.lock();
            let artist = store::upsert_item(&conn, &ItemDraft::new(Media::Artist, "Tycho").id("artist", "tycho").id("mbid", "mbid-artist")).unwrap();
            store::record_event(&conn, artist, "lastfm", EventKind::Play, 1800.0, None, None, &Value::Null).unwrap();
        }
        store::set_setting(&t.ctx.db, "taste", &json!({ "dial": 0.0 })).unwrap();
        t.net.on("method=artist.getSimilar", 200, r#"{"similarartists":{"artist":[]}}"#);
        t.net.on("method=artist.getTopTags", 200, include_str!("../../tests/fixtures/lastfm_artist_tags.json"));
        // The fixture's newest album is dated by year only; keep it in the future whenever this runs.
        let next_year = (chrono::Datelike::year(&chrono::Local::now()) + 1).to_string();
        let groups = include_str!("../../tests/fixtures/mb_release_groups.json").replace("\"2026\"", &format!("\"{next_year}\""));
        t.net.on("musicbrainz.org/ws/2/release-group", 200, &groups);
        let n = refresh_music(&t.ctx, &|_, _| {}).await.unwrap();
        assert_eq!(n, 1, "only the upcoming album is new; Dive (2011) and Weather (2019) are back catalogue");
        let conn = t.ctx.db.lock();
        let (title, data): (String, String) = conn.query_row("SELECT title, data FROM items WHERE media = 'album'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(title, "Infinite Health");
        assert!(data.contains("ambient"), "artist tags are attached so the album can be scored");
    }
}
