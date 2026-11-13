//! MusicBrainz: release groups (albums, EPs, singles), new releases per artist, artist
//! relationships, plus Cover Art Archive for artwork. https://musicbrainz.org/doc/MusicBrainz_API
//!
//! MusicBrainz asks for one request per second and a descriptive User-Agent; the scheduler
//! enforces the first and the transport sends the second.

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::util::*;
use serde_json::{json, Value};

const BASE: &str = "https://musicbrainz.org/ws/2";

pub fn cover(release_group_mbid: &str) -> String {
    format!("https://coverartarchive.org/release-group/{release_group_mbid}/front-500")
}

async fn get(ctx: &Ctx, path: &str, query: &[(&str, &str)], ttl: i64) -> Result<Value> {
    ctx.http.get("MusicBrainz", format!("{BASE}{path}")).query(query).query(&[("fmt", "json")]).header("User-Agent", ctx.user_agent()).header("Accept", "application/json").ttl(ttl).json().await
}

/// Finds an artist's MBID by name. Only a confident match is returned.
pub async fn find_artist(ctx: &Ctx, name: &str) -> Result<Option<(String, String)>> {
    let q = format!("artist:\"{}\"", name.replace('"', " "));
    let v = get(ctx, "/artist", &[("query", &q), ("limit", "3")], 30 * 86_400).await?;
    Ok(arr(&v, "artists")
        .iter()
        .find(|a| i(a, "score").unwrap_or(0) >= 90 && s(a, "name").map(|n| n.to_lowercase() == name.to_lowercase()).unwrap_or(false))
        .or_else(|| arr(&v, "artists").iter().find(|a| i(a, "score").unwrap_or(0) >= 98))
        .and_then(|a| Some((s(a, "id")?, s(a, "name")?))))
}

fn album_type(rg: &Value) -> Option<&'static str> {
    // Skip compilations, live albums, remixes, soundtracks-of-others and so on.
    if !arr(rg, "secondary-types").is_empty() {
        return None;
    }
    match rg["primary-type"].as_str()? {
        "Album" => Some("album"),
        "EP" => Some("ep"),
        "Single" => Some("single"),
        _ => None,
    }
}

fn rg_draft(rg: &Value, artist: &str, artist_mbid: Option<&str>) -> Option<ItemDraft> {
    let kind = album_type(rg)?;
    let mbid = s(rg, "id")?;
    let mut d = ItemDraft::new(Media::Album, s(rg, "title")?).id("mbid", &mbid);
    d.release_date = s(rg, "first-release-date").and_then(|x| partial_date(&x));
    d.set("date_precision", s(rg, "first-release-date").map(|x| x.len()));
    d.poster = Some(cover(&mbid));
    d.creators.push(Creator::new(artist, "artist"));
    d.set("artist", artist);
    d.set("artist_mbid", artist_mbid);
    d.set("album_type", kind);
    d.tag_source = TagSource::MusicBrainz;
    d.tags = tag_names(rg);
    d.ttl_secs = 7 * 86_400;
    Some(d)
}

fn tag_names(v: &Value) -> Vec<String> {
    let mut tags: Vec<(i64, String)> = arr(v, "genres").iter().chain(arr(v, "tags")).filter_map(|t| Some((i(t, "count").unwrap_or(0), s(t, "name")?))).collect();
    tags.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out: Vec<String> = vec![];
    for (_, t) in tags {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out.truncate(12);
    out
}

/// An artist's albums, EPs and singles (newest first).
pub async fn release_groups(ctx: &Ctx, artist_mbid: &str, artist: &str) -> Result<Vec<ItemDraft>> {
    let v = get(ctx, "/release-group", &[("artist", artist_mbid), ("type", "album|ep|single"), ("limit", "100")], 12 * 3600).await?;
    let mut out: Vec<ItemDraft> = arr(&v, "release-groups").iter().filter_map(|rg| rg_draft(rg, artist, Some(artist_mbid))).collect();
    out.sort_by(|a, b| b.release_date.cmp(&a.release_date));
    Ok(out)
}

/// Full details for one release group: tags, track list, label, length.
pub async fn details(ctx: &Ctx, mbid: &str) -> Result<ItemDraft> {
    let rg = get(ctx, &format!("/release-group/{mbid}"), &[("inc", "artist-credits+tags+genres+url-rels")], 7 * 86_400).await?;
    let credit = arr(&rg, "artist-credit").first().cloned().unwrap_or(Value::Null);
    let artist = s(&credit, "name").or_else(|| s(&credit["artist"], "name")).unwrap_or_else(|| "Unknown artist".into());
    let artist_mbid = s(&credit["artist"], "id");
    let mut d = match rg_draft(&rg, &artist, artist_mbid.as_deref()) {
        Some(d) => d,
        None => {
            // A compilation or live album the user asked for by name: still show it.
            let mut d = ItemDraft::new(Media::Album, s(&rg, "title").unwrap_or_default()).id("mbid", mbid);
            d.release_date = s(&rg, "first-release-date").and_then(|x| partial_date(&x));
            d.poster = Some(cover(mbid));
            d.creators.push(Creator::new(artist.clone(), "artist"));
            d.set("artist", artist.clone());
            d.set("album_type", "album");
            d.tags = tag_names(&rg);
            d
        }
    };
    d.detail_level = 1;
    // One official release gives the track list, the label and the running time.
    let rel = get(ctx, "/release", &[("release-group", mbid), ("status", "official"), ("inc", "labels+recordings"), ("limit", "1")], 7 * 86_400).await?;
    if let Some(r) = arr(&rel, "releases").first() {
        if let Some(label) = arr(r, "label-info").first().and_then(|l| s(&l["label"], "name")) {
            d.creators.push(Creator::new(label.clone(), "label"));
            d.set("label", label);
        }
        let mut total_ms = 0;
        let mut tracks = vec![];
        for medium in arr(r, "media") {
            for t in arr(medium, "tracks") {
                let ms = i(t, "length").or_else(|| i(&t["recording"], "length")).unwrap_or(0);
                total_ms += ms;
                tracks.push(json!({ "n": t["position"], "title": t["title"], "ms": ms }));
            }
        }
        if total_ms > 0 {
            d.runtime = Some(total_ms / 60_000);
        }
        d.set("tracks", tracks);
    }
    Ok(d)
}

/// Bands the artist is in, members, and collaborators.
pub async fn related_artists(ctx: &Ctx, artist_mbid: &str) -> Result<Vec<Value>> {
    let v = get(ctx, &format!("/artist/{artist_mbid}"), &[("inc", "artist-rels")], 30 * 86_400).await?;
    Ok(arr(&v, "relations")
        .iter()
        .filter(|r| matches!(r["type"].as_str(), Some("member of band" | "collaboration" | "supporting musician" | "is person" | "subgroup")))
        .filter_map(|r| Some(json!({ "name": s(&r["artist"], "name")?, "mbid": s(&r["artist"], "id")?, "relation": r["type"] })))
        .take(12)
        .collect())
}

pub async fn search_albums(ctx: &Ctx, text: &str, limit: u32) -> Result<Vec<ItemDraft>> {
    let q = format!("releasegroup:\"{0}\" OR artist:\"{0}\"", text.replace('"', " "));
    let v = get(ctx, "/release-group", &[("query", &q), ("limit", &limit.to_string())], 86_400).await?;
    Ok(arr(&v, "release-groups")
        .iter()
        .filter_map(|rg| {
            let credit = arr(rg, "artist-credit").first()?;
            let artist = s(credit, "name").or_else(|| s(&credit["artist"], "name"))?;
            let mut d = rg_draft(rg, &artist, s(&credit["artist"], "id").as_deref())?;
            d.popularity = f(rg, "score").unwrap_or(0.0) / 10.0;
            Some(d)
        })
        .collect())
}

/// Finds the release group for "artist – album" (used to give imports and Spotify albums an MBID).
pub async fn find_album(ctx: &Ctx, artist: &str, title: &str) -> Result<Option<ItemDraft>> {
    let q = format!("releasegroup:\"{}\" AND artist:\"{}\"", title.replace('"', " "), artist.replace('"', " "));
    let v = get(ctx, "/release-group", &[("query", &q), ("limit", "5")], 30 * 86_400).await?;
    Ok(arr(&v, "release-groups").iter().filter(|rg| i(rg, "score").unwrap_or(0) >= 85).find_map(|rg| {
        let credit = arr(rg, "artist-credit").first()?;
        let name = s(credit, "name").or_else(|| s(&credit["artist"], "name"))?;
        rg_draft(rg, &name, s(&credit["artist"], "id").as_deref())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    const RGS: &str = include_str!("../../tests/fixtures/mb_release_groups.json");
    const RG: &str = include_str!("../../tests/fixtures/mb_release_group.json");
    const RELEASE: &str = include_str!("../../tests/fixtures/mb_release.json");

    #[tokio::test]
    async fn release_groups_skip_compilations_and_sort_newest_first() {
        let t = ctx();
        t.net.on("musicbrainz.org/ws/2/release-group", 200, RGS);
        let albums = release_groups(&t.ctx, "mbid-artist", "Tycho").await.unwrap();
        assert_eq!(albums.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), vec!["Infinite Health", "Weather", "Dive"]);
        assert_eq!(albums[0].data["album_type"], "album");
        assert_eq!(albums[0].data["artist"], "Tycho");
        assert_eq!(albums[0].poster.as_deref(), Some("https://coverartarchive.org/release-group/rg-1/front-500"));
        let sent = t.net.last("musicbrainz.org").unwrap();
        assert!(sent.headers.iter().any(|(k, v)| k == "User-Agent" && v.starts_with("Marquee/")), "MusicBrainz requires a real User-Agent");
        assert!(sent.url.contains("fmt=json"));
    }

    #[tokio::test]
    async fn a_year_only_date_is_not_treated_as_already_out() {
        let t = ctx();
        t.net.on("musicbrainz.org/ws/2/release-group", 200, RGS);
        let albums = release_groups(&t.ctx, "mbid-artist", "Tycho").await.unwrap();
        assert_eq!(albums[0].release_date.as_deref(), Some("2026-12-31"));
        assert_eq!(albums[0].data["date_precision"], 4);
    }

    #[tokio::test]
    async fn details_add_tracks_label_and_length() {
        let t = ctx();
        t.net.on("musicbrainz.org/ws/2/release-group/rg-3", 200, RG);
        t.net.on("musicbrainz.org/ws/2/release?", 200, RELEASE);
        let d = details(&t.ctx, "rg-3").await.unwrap();
        assert_eq!(d.title, "Dive");
        assert_eq!(d.runtime, Some(9));
        assert_eq!(d.data["label"], "Ghostly International");
        assert_eq!(d.data["tracks"].as_array().unwrap().len(), 2);
        assert_eq!(d.tags[0], "ambient", "most-voted tag first");
    }
}
