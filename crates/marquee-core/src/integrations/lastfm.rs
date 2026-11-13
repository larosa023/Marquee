//! Last.fm: scrobble history, top artists and albums by period, loved tracks. The best
//! long-term music signal. Reading needs only a username; loving a track needs a session.

use super::{cfg, item_for_write, set_cfg, Source, Synced, WriteOp, WriteOutcome};
use crate::catalog::lastfm as api;
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use crate::util::*;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

pub struct Lastfm;

/// Connects by username (read-only).
pub async fn connect(ctx: &Ctx, username: &str) -> Result<String> {
    let username = username.trim();
    if username.is_empty() {
        return Err(Error::msg("Type your Last.fm username."));
    }
    let info = api::user_info(ctx, username).await?;
    let name = s(&info, "name").unwrap_or_else(|| username.to_string());
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "lastfm", json!({ "username": name, "scrobbles": i(&info, "playcount") }))?;
    store::set_source(&conn, "lastfm", "connected", Some(&name), None)?;
    Ok(name)
}

/// Step 1 of write access: get a token and open Last.fm's approval page.
pub async fn auth_start(ctx: &Ctx) -> Result<String> {
    let key = ctx.require_key("lastfm_key")?;
    ctx.require_key("lastfm_secret")?;
    let v = api::signed_post(ctx, "auth.getToken", &[], None).await?;
    let token = s(&v, "token").ok_or_else(|| Error::msg("Last.fm didn't return a token."))?;
    ctx.host.open_url(&format!("https://www.last.fm/api/auth/?api_key={key}&token={token}"));
    Ok(token)
}

/// Step 2: after the user clicks "Allow", trade the token for a session key.
pub async fn auth_finish(ctx: &Ctx, token: &str) -> Result<String> {
    let v = api::signed_post(ctx, "auth.getSession", &[("token", token)], None).await.map_err(|e| match e {
        Error::Reauth(_) | Error::Api { .. } => Error::msg("Last.fm hasn't been approved yet. Click \"Yes, allow access\" on the Last.fm page, then try again."),
        other => other,
    })?;
    let sk = s(&v["session"], "key").ok_or_else(|| Error::msg("Last.fm didn't return a session."))?;
    let name = s(&v["session"], "name").unwrap_or_default();
    ctx.secrets.set("lastfm.session", &sk)?;
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "lastfm", json!({ "username": name, "can_write": true }))?;
    store::set_source(&conn, "lastfm", "connected", Some(&name), None)?;
    Ok(name)
}

fn artist_draft(name: &str, mbid: Option<&str>) -> ItemDraft {
    let mut d = ItemDraft::new(Media::Artist, name).id("artist", slug(name));
    if let Some(m) = mbid.filter(|m| !m.is_empty()) {
        d.ids.push(("mbid".into(), m.to_string()));
    }
    d.creators.push(Creator::new(name, "artist"));
    d.tag_source = TagSource::Lastfm;
    d
}

#[async_trait]
impl Source for Lastfm {
    fn id(&self) -> &'static str {
        "lastfm"
    }
    fn name(&self) -> &'static str {
        "Last.fm"
    }
    fn reads(&self) -> Vec<&'static str> {
        vec!["Scrobbles", "Top artists and albums", "Loved tracks", "Tags"]
    }
    fn writes(&self) -> Vec<&'static str> {
        vec!["Love track"]
    }
    fn note(&self) -> &'static str {
        "Your username is enough to read. Sign in as well if you want Marquee to love tracks for you."
    }
    fn configured(&self, ctx: &Ctx) -> bool {
        ctx.has_key("lastfm_key")
    }

    async fn sync(&self, ctx: &Ctx) -> Result<Synced> {
        let user = s(&cfg(ctx, "lastfm"), "username").ok_or_else(|| Error::Reauth("Last.fm".into()))?;
        let mut out = Synced::default();
        let now = chrono::Utc::now();
        let ago = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        // All-time counts say how much; the recent windows say how lately.
        let overall = api::top_artists(ctx, &user, "overall", 200).await?;
        let mut last_heard: HashMap<String, i64> = HashMap::new();
        for (period, days) in [("12month", 200), ("3month", 45), ("1month", 15), ("7day", 3)] {
            for a in api::top_artists(ctx, &user, period, 100).await? {
                last_heard.insert(a.artist.to_lowercase(), days);
            }
        }
        for a in &overall {
            let recent = last_heard.get(&a.artist.to_lowercase()).copied();
            let mut ev = RawEvent::new(artist_draft(&a.artist, a.mbid.as_deref()), EventKind::Play, a.plays as f64).at(Some(ago(recent.unwrap_or(540))));
            // Still in rotation after a long time means sustained taste, not a phase.
            ev.first_at = Some(ago(if recent.is_some() { 720 } else { 560 }));
            ev.meta = json!({ "scrobbles": a.plays });
            out.events.push(ev);
        }

        for al in api::top_albums(ctx, &user, "overall", 200).await? {
            let Some(title) = al.name else { continue };
            let mut d = ItemDraft::new(Media::Album, title);
            if let Some(m) = al.mbid.filter(|m| !m.is_empty()) {
                d.ids.push(("lastfm".into(), m));
            }
            d.poster = al.image;
            d.creators.push(Creator::new(al.artist.clone(), "artist"));
            d.set("artist", al.artist.clone());
            d.tag_source = TagSource::Lastfm;
            let recent = last_heard.get(&al.artist.to_lowercase()).copied();
            let mut ev = RawEvent::new(d, EventKind::Play, al.plays as f64).at(Some(ago(recent.unwrap_or(540))));
            ev.meta = json!({ "scrobbles": al.plays });
            out.events.push(ev);
        }

        // A loved track is a like for its artist.
        let mut loved: HashMap<String, (i64, Option<i64>)> = HashMap::new();
        for (artist, _track, at) in api::loved_tracks(ctx, &user, 500).await? {
            let e = loved.entry(artist).or_insert((0, at));
            e.0 += 1;
            e.1 = e.1.max(at);
        }
        for (artist, (count, at)) in loved {
            let mut ev = RawEvent::new(artist_draft(&artist, None), EventKind::Like, count as f64).at(at.and_then(unix_to_rfc3339));
            ev.meta = json!({ "loved_tracks": count });
            out.events.push(ev);
        }

        // Scrobbles per month for the last year, for the Taste screen's stats.
        let mut months = vec![];
        let today = chrono::Local::now().date_naive();
        let mut first = today.with_day(1).unwrap_or(today);
        for _ in 0..12 {
            let next = if first.month() == 12 { chrono::NaiveDate::from_ymd_opt(first.year() + 1, 1, 1) } else { chrono::NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1) }.unwrap_or(first);
            let from = first.and_hms_opt(0, 0, 0).map(|d| d.and_utc().timestamp()).unwrap_or(0);
            let to = next.and_hms_opt(0, 0, 0).map(|d| d.and_utc().timestamp()).unwrap_or(0);
            match api::scrobbles_between(ctx, &user, from, to).await {
                Ok(n) => months.push(json!({ "month": first.format("%Y-%m").to_string(), "scrobbles": n })),
                Err(_) => break,
            }
            first = (first - chrono::Duration::days(1)).with_day(1).unwrap_or(first);
        }
        months.reverse();
        set_cfg(ctx, "lastfm", json!({ "months": months }))?;
        out.account = Some(user);
        Ok(out)
    }

    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome> {
        let item = item_for_write(ctx, op.item_id())?;
        let sk = ctx.secrets.get("lastfm.session").ok_or_else(|| Error::msg("Sign in to Last.fm (Settings → Connections) to love tracks from Marquee."))?;
        match op {
            WriteOp::Love { .. } | WriteOp::Rate { .. } if item.media == Media::Album => {
                if let WriteOp::Rate { stars, .. } = op {
                    if *stars < 4.0 {
                        return Ok(WriteOutcome::Synced); // only high ratings become a "love"
                    }
                }
                let artist = item.data["artist"].as_str().unwrap_or("").to_string();
                // Last.fm loves tracks, not albums: love the album's first track.
                let info = api::album_info(ctx, &artist, &item.title).await?;
                let track = list(&info["tracks"]["track"]).first().and_then(|t| s(t, "name")).or_else(|| item.data["tracks"][0]["title"].as_str().map(String::from)).ok_or_else(|| Error::msg(format!("Last.fm has no track list for {}.", item.title)))?;
                api::signed_post(ctx, "track.love", &[("artist", &artist), ("track", &track)], Some(&sk)).await?;
                Ok(WriteOutcome::Synced)
            }
            _ => Err(Error::msg("Last.fm can only love music.")),
        }
    }

    fn forget(&self, ctx: &Ctx) {
        let _ = ctx.secrets.delete("lastfm.session");
    }
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[tokio::test]
    async fn connecting_by_username_checks_the_profile_exists() {
        let t = ctx();
        t.net.on("method=user.getInfo", 200, r#"{"user":{"name":"anthony","playcount":"52000"}}"#);
        assert_eq!(connect(&t.ctx, " anthony ").await.unwrap(), "anthony");
        t.net.on("method=user.getInfo", 404, r#"{"error":6,"message":"User not found"}"#);
        assert!(connect(&t.ctx, "nobody-here").await.unwrap_err().to_string().contains("couldn't find"));
        assert!(connect(&t.ctx, "  ").await.is_err());
    }

    #[tokio::test]
    async fn sync_turns_scrobbles_into_dated_play_events() {
        let t = ctx();
        store::set_source_config(&t.ctx.db.lock(), "lastfm", json!({ "username": "anthony" })).unwrap();
        let top = include_str!("../../tests/fixtures/lastfm_top_artists.json");
        t.net.on("method=user.getTopArtists", 200, r#"{"topartists":{"artist":[]}}"#);
        t.net.on("period=overall", 200, top);
        t.net.on("period=1month", 200, r#"{"topartists":{"artist":{"name":"Tycho","mbid":"","playcount":"40"}}}"#);
        t.net.on("method=user.getTopAlbums", 200, r##"{"topalbums":{"album":[{"name":"Dive","playcount":"310","mbid":"","artist":{"name":"Tycho"},"image":[{"size":"small","#text":""},{"size":"extralarge","#text":"https://l/dive.jpg"}]}]}}"##);
        t.net.on("method=user.getLovedTracks", 200, r#"{"lovedtracks":{"track":[{"name":"Awake","date":{"uts":"1700000000"},"artist":{"name":"Tycho"}},{"name":"Dive","date":{"uts":"1600000000"},"artist":{"name":"Tycho"}}]}}"#);
        t.net.on("method=user.getRecentTracks", 200, r#"{"recenttracks":{"track":[],"@attr":{"total":"420"}}}"#);
        let synced = Lastfm.sync(&t.ctx).await.unwrap();
        let tycho = synced.events.iter().find(|e| e.kind == EventKind::Play && e.draft.title == "Tycho").unwrap();
        let boc = synced.events.iter().find(|e| e.kind == EventKind::Play && e.draft.title == "Boards of Canada").unwrap();
        assert_eq!(tycho.value, 1842.0);
        assert!(tycho.at > boc.at, "an artist still in rotation is dated more recently");
        assert!(tycho.draft.ids.contains(&("artist".into(), "tycho".into())));
        let album = synced.events.iter().find(|e| e.draft.media() == Media::Album).unwrap();
        assert_eq!(album.draft.poster.as_deref(), Some("https://l/dive.jpg"));
        let love = synced.events.iter().find(|e| e.kind == EventKind::Like).unwrap();
        assert_eq!(love.value, 2.0);
        let months = store::source_config(&t.ctx.db.lock(), "lastfm")["months"].as_array().unwrap().len();
        assert_eq!(months, 12);
    }

    #[tokio::test]
    async fn loving_needs_a_session_and_loves_the_first_track() {
        let t = ctx();
        let id = {
            let mut d = ItemDraft::new(Media::Album, "Dive").id("mbid", "rg-3");
            d.set("artist", "Tycho");
            store::upsert_item(&t.ctx.db.lock(), &d).unwrap()
        };
        assert!(Lastfm.write(&t.ctx, &WriteOp::Love { item_id: id }).await.unwrap_err().to_string().contains("Sign in"));
        t.ctx.secrets.set("lastfm.session", "sess").unwrap();
        t.net.on("method=album.getInfo", 200, r#"{"album":{"name":"Dive","tracks":{"track":[{"name":"A Walk"},{"name":"Hours"}]}}}"#);
        t.net.on("POST https://ws.audioscrobbler.com", 200, "{}");
        Lastfm.write(&t.ctx, &WriteOp::Love { item_id: id }).await.unwrap();
        let body = String::from_utf8(t.net.last("POST").unwrap().body.unwrap()).unwrap();
        assert!(body.contains("track=A%20Walk") && body.contains("artist=Tycho") && body.contains("sk=sess"));
        // A middling rating doesn't love anything.
        Lastfm.write(&t.ctx, &WriteOp::Rate { item_id: id, stars: 3.0 }).await.unwrap();
        assert_eq!(t.net.calls("POST"), 1);
    }
}
