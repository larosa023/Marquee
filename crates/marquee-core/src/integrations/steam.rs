//! Steam: owned games, playtime, recently played and the wishlist.
//!
//! Sign-in is Steam OpenID (it only proves which SteamID is yours; Marquee never sees a
//! password). Reads use the Steam Web API with a key. Steam has no supported API for adding to
//! a wishlist, so "Add to wishlist" opens the game's store page in the Steam client and the
//! item stays "pending" until a later sync sees it on the wishlist.

use super::{cfg, item_for_write, oauth, set_cfg, Source, Synced, WriteOp, WriteOutcome};
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store;
use crate::util::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

const API: &str = "https://api.steampowered.com";
const OPENID: &str = "https://steamcommunity.com/openid/login";

pub const PRIVATE_HELP: &str = "Steam isn't sharing your games. In Steam, open your profile → Edit Profile → Privacy Settings and set \"Game details\" to Public (and untick \"Always keep my total playtime private\"), then sync again.";

pub struct Steam;

pub fn openid_url(port: u16) -> String {
    let realm = format!("http://127.0.0.1:{port}");
    let params = [
        ("openid.ns", "http://specs.openid.net/auth/2.0"),
        ("openid.mode", "checkid_setup"),
        ("openid.return_to", &format!("{realm}/steam")),
        ("openid.realm", &realm),
        ("openid.identity", "http://specs.openid.net/auth/2.0/identifier_select"),
        ("openid.claimed_id", "http://specs.openid.net/auth/2.0/identifier_select"),
    ];
    format!("{OPENID}?{}", params.iter().map(|(k, v)| format!("{k}={}", urlencoding::encode(v))).collect::<Vec<_>>().join("&"))
}

pub fn steamid_from_claimed(claimed: &str) -> Option<String> {
    let id = claimed.strip_prefix("https://steamcommunity.com/openid/id/")?;
    (id.len() == 17 && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_string())
}

/// Opens the Steam sign-in page, waits for the redirect, verifies it with Steam and stores the SteamID.
pub async fn connect(ctx: &Ctx) -> Result<String> {
    ctx.host.open_url(&openid_url(oauth::PORT));
    let params = oauth::wait_for_redirect(oauth::PORT, "/steam", Duration::from_secs(300)).await?;
    if params.get("openid.mode").map(String::as_str) != Some("id_res") {
        return Err(Error::msg("Steam sign-in was cancelled."));
    }
    // Ask Steam whether this assertion is genuine before trusting the id in it.
    let mut check: Vec<(String, String)> = params.iter().filter(|(k, _)| k.starts_with("openid.")).map(|(k, v)| (k.clone(), v.clone())).collect();
    for p in check.iter_mut() {
        if p.0 == "openid.mode" {
            p.1 = "check_authentication".into();
        }
    }
    let form: Vec<(&str, &str)> = check.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let verdict = ctx.http.post("Steam", OPENID).form(&form).text().await?;
    if !verdict.lines().any(|l| l.trim() == "is_valid:true") {
        return Err(Error::msg("Steam couldn't verify that sign-in. Try connecting again."));
    }
    let steamid = params.get("openid.claimed_id").and_then(|c| steamid_from_claimed(c)).ok_or_else(|| Error::msg("Steam didn't return a SteamID."))?;
    finish_connect(ctx, &steamid).await
}

/// Connects with a SteamID the user typed in (for people who'd rather not use the browser sign-in).
pub async fn connect_with_id(ctx: &Ctx, steamid: &str) -> Result<String> {
    let id = steamid.trim();
    if !(id.len() == 17 && id.chars().all(|c| c.is_ascii_digit())) {
        return Err(Error::msg("A SteamID is 17 digits, like 76561198000000000. You can find yours at steamid.io."));
    }
    finish_connect(ctx, id).await
}

async fn finish_connect(ctx: &Ctx, steamid: &str) -> Result<String> {
    let name = match ctx.key("steam") {
        Some(key) => {
            let v: Value = ctx.http.get("Steam", format!("{API}/ISteamUser/GetPlayerSummaries/v2/")).query(&[("key", &key), ("steamids", steamid)]).json().await?;
            s(&v["response"]["players"][0], "personaname").unwrap_or_else(|| steamid.to_string())
        }
        None => steamid.to_string(),
    };
    let conn = ctx.db.lock();
    store::set_source_config(&conn, "steam", json!({ "steamid": steamid }))?;
    store::set_source(&conn, "steam", "connected", Some(&name), None)?;
    // Play Later mirrors the Steam wishlist unless the user already pointed it elsewhere.
    conn.execute("UPDATE lists SET mirror = 'steam:wishlist' WHERE slug = 'play-later' AND mirror IS NULL", [])?;
    Ok(name)
}

fn game_draft(appid: &str, name: Option<String>) -> ItemDraft {
    let mut d = ItemDraft::new(Media::Game, name.unwrap_or_else(|| format!("Steam app {appid}"))).id("steam", appid);
    d.poster = Some(format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/library_600x900.jpg"));
    d.set("header", format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg"));
    d.set("platforms", json!(["PC"]));
    d.set("store_url", format!("https://store.steampowered.com/app/{appid}"));
    d.tag_source = TagSource::Steam;
    d
}

#[async_trait]
impl Source for Steam {
    fn id(&self) -> &'static str {
        "steam"
    }
    fn name(&self) -> &'static str {
        "Steam"
    }
    fn reads(&self) -> Vec<&'static str> {
        vec!["Owned games", "Playtime", "Recently played", "Wishlist"]
    }
    fn writes(&self) -> Vec<&'static str> {
        vec!["Wishlist (opens the store page)"]
    }
    fn note(&self) -> &'static str {
        "Steam has no way for apps to add to your wishlist, so Marquee opens the store page and confirms on the next sync."
    }
    fn configured(&self, ctx: &Ctx) -> bool {
        ctx.has_key("steam")
    }

    async fn sync(&self, ctx: &Ctx) -> Result<Synced> {
        let key = ctx.require_key("steam")?;
        let steamid = s(&cfg(ctx, "steam"), "steamid").ok_or_else(|| Error::Reauth("Steam".into()))?;
        let mut out = Synced::default();

        let owned: Value =
            ctx.http.get("Steam", format!("{API}/IPlayerService/GetOwnedGames/v1/")).query(&[("key", &key), ("steamid", &steamid), ("include_appinfo", "1"), ("include_played_free_games", "1")]).json().await.map_err(|e| match e {
                Error::Api { status: 401 | 403, .. } => Error::NotConfigured("Steam rejected the Web API key. Check it in Settings → Connections → API keys.".into()),
                other => other,
            })?;
        let games = arr(&owned["response"], "games");
        if owned["response"].get("games").is_none() {
            // A private profile answers with an empty object, not an error.
            out.warnings.push(PRIVATE_HELP.to_string());
        }
        // Names and real artwork for every game in one batch per hundred.
        let wl: Value = ctx.http.get("Steam", format!("{API}/IWishlistService/GetWishlist/v1/")).query(&[("key", &key), ("steamid", &steamid)]).json().await.unwrap_or(Value::Null);
        let mut appids: Vec<String> = games.iter().filter_map(|g| s(g, "appid")).collect();
        appids.extend(arr(&wl["response"], "items").iter().filter_map(|it| s(it, "appid")));
        let store = crate::catalog::steam_store::items(ctx, &appids).await.unwrap_or_else(|e| {
            tracing::debug!("couldn't read Steam store art: {e}");
            Default::default()
        });
        let draft_for = |appid: &str, name: Option<String>| {
            let mut d = game_draft(appid, name);
            if let Some(it) = store.get(appid) {
                crate::catalog::steam_store::apply(&mut d, it);
            }
            d
        };
        for g in games {
            let Some(appid) = s(g, "appid") else { continue };
            let draft = draft_for(&appid, s(g, "name"));
            let minutes = f(g, "playtime_forever").unwrap_or(0.0);
            let last = i(g, "rtime_last_played").filter(|t| *t > 0).and_then(unix_to_rfc3339);
            out.events.push(RawEvent::new(draft.clone(), EventKind::Owned, 1.0).at(last.clone()));
            if minutes > 0.0 {
                let mut ev = RawEvent::new(draft, EventKind::Play, minutes).at(last);
                ev.meta = json!({ "minutes": minutes, "recent_minutes": f(g, "playtime_2weeks").unwrap_or(0.0) });
                out.events.push(ev);
            }
        }

        // The wishlist moved to IWishlistService in late 2024; the old store JSON endpoint is gone.
        let mut wishlist = vec![];
        for item in arr(&wl["response"], "items") {
            let Some(appid) = s(item, "appid") else { continue };
            let draft = draft_for(&appid, None);
            let added = i(item, "date_added").and_then(unix_to_rfc3339);
            out.events.push(RawEvent::new(draft.clone(), EventKind::Wishlist, 1.0).at(added));
            wishlist.push(draft);
        }
        if wl.is_null() {
            out.warnings.push("Steam didn't return your wishlist this time. It will be retried on the next sync.".into());
        } else {
            out.lists.push(("steam:wishlist".into(), wishlist));
        }
        Ok(out)
    }

    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome> {
        let item = item_for_write(ctx, op.item_id())?;
        let appid = item.ext("steam").ok_or_else(|| Error::msg(format!("{} isn't on Steam, so it can't go on your Steam wishlist.", item.title)))?.to_string();
        match op {
            WriteOp::AddToList { .. } | WriteOp::RemoveFromList { .. } => {
                let url = format!("steam://store/{appid}");
                ctx.host.open_url(&url);
                let adding = matches!(op, WriteOp::AddToList { .. });
                Ok(WriteOutcome::Opened {
                    url,
                    hint: if adding {
                        format!("Steam opened {}. Click \"Add to your wishlist\" there; Marquee confirms it on the next sync.", item.title)
                    } else {
                        format!("Steam opened {}. Remove it from your wishlist there; Marquee confirms it on the next sync.", item.title)
                    },
                })
            }
            _ => Err(Error::msg("Steam only supports wishlist changes.")),
        }
    }

    fn forget(&self, ctx: &Ctx) {
        let _ = set_cfg(ctx, "steam", json!({ "steamid": null }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    fn connected() -> crate::ctx::testing::TestCtx {
        let t = ctx();
        store::set_source_config(&t.ctx.db.lock(), "steam", json!({ "steamid": "76561198000000001" })).unwrap();
        t
    }

    #[test]
    fn openid_urls_and_claimed_ids() {
        let url = openid_url(43117);
        assert!(url.starts_with("https://steamcommunity.com/openid/login?openid.ns="));
        assert!(url.contains("openid.return_to=http%3A%2F%2F127.0.0.1%3A43117%2Fsteam"));
        assert_eq!(steamid_from_claimed("https://steamcommunity.com/openid/id/76561198000000001").as_deref(), Some("76561198000000001"));
        assert_eq!(steamid_from_claimed("https://evil.example/openid/id/76561198000000001"), None);
        assert_eq!(steamid_from_claimed("https://steamcommunity.com/openid/id/123"), None);
    }

    #[tokio::test]
    async fn sync_reads_library_playtime_and_wishlist() {
        let t = connected();
        t.net.on("IPlayerService/GetOwnedGames", 200, include_str!("../../tests/fixtures/steam_owned.json"));
        t.net.on("IWishlistService/GetWishlist", 200, r#"{"response":{"items":[{"appid":1145360,"priority":1,"date_added":1700000000}]}}"#);
        let synced = Steam.sync(&t.ctx).await.unwrap();
        let plays: Vec<_> = synced.events.iter().filter(|e| e.kind == EventKind::Play).collect();
        assert_eq!(plays.len(), 1, "an unplayed game is owned but has no play event");
        assert_eq!(plays[0].value, 1320.0);
        assert_eq!(plays[0].meta["recent_minutes"], 95.0);
        assert_eq!(synced.events.iter().filter(|e| e.kind == EventKind::Owned).count(), 2);
        assert_eq!(synced.events.iter().filter(|e| e.kind == EventKind::Wishlist).count(), 1);
        assert_eq!(synced.lists[0].0, "steam:wishlist");
        assert!(synced.warnings.is_empty());
        assert!(t.net.last("GetOwnedGames").unwrap().url.contains("steamid=76561198000000001"));
    }

    #[tokio::test]
    async fn a_private_profile_gets_instructions_not_an_error() {
        let t = connected();
        t.net.on("IPlayerService/GetOwnedGames", 200, r#"{"response":{}}"#);
        t.net.on("IWishlistService/GetWishlist", 200, r#"{"response":{}}"#);
        let synced = Steam.sync(&t.ctx).await.unwrap();
        assert!(synced.events.is_empty());
        assert!(synced.warnings[0].contains("Game details"));
    }

    #[tokio::test]
    async fn adding_to_the_wishlist_opens_the_steam_client() {
        let t = connected();
        let id = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Game, "Hades").id("steam", 1145360)).unwrap();
        let outcome = Steam.write(&t.ctx, &WriteOp::AddToList { item_id: id, target: "wishlist".into() }).await.unwrap();
        assert!(matches!(outcome, WriteOutcome::Opened { .. }));
        assert_eq!(t.host.opened.lock().unwrap().as_slice(), ["steam://store/1145360"]);
    }

    #[tokio::test]
    async fn a_game_that_is_not_on_steam_says_so() {
        let t = connected();
        let id = store::upsert_item(&t.ctx.db.lock(), &ItemDraft::new(Media::Game, "Astro Bot").id("igdb", 1)).unwrap();
        let err = Steam.write(&t.ctx, &WriteOp::AddToList { item_id: id, target: "wishlist".into() }).await.unwrap_err();
        assert!(err.to_string().contains("isn't on Steam"));
    }

    #[tokio::test]
    async fn typed_steam_ids_are_checked() {
        let t = ctx();
        assert!(connect_with_id(&t.ctx, "anthony").await.is_err());
        t.net.on("GetPlayerSummaries", 200, r#"{"response":{"players":[{"steamid":"76561198000000001","personaname":"larosa"}]}}"#);
        assert_eq!(connect_with_id(&t.ctx, "76561198000000001").await.unwrap(), "larosa");
        let mirror: Option<String> = t.ctx.db.lock().query_row("SELECT mirror FROM lists WHERE slug = 'play-later'", [], |r| r.get(0)).unwrap();
        assert_eq!(mirror.as_deref(), Some("steam:wishlist"));
    }
}
