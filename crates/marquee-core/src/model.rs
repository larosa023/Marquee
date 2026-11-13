//! Shared data types.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Media {
    Movie,
    Tv,
    Game,
    Album,
    /// Artists are stored so scrobbles and follows have something to attach to. They feed the
    /// taste profile but never appear as cards in a feed.
    Artist,
}

impl Media {
    pub const FEED: [Media; 4] = [Media::Movie, Media::Tv, Media::Game, Media::Album];

    pub fn as_str(self) -> &'static str {
        match self {
            Media::Movie => "movie",
            Media::Tv => "tv",
            Media::Game => "game",
            Media::Album => "album",
            Media::Artist => "artist",
        }
    }

    pub fn parse(s: &str) -> Option<Media> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "movie" | "movies" | "film" | "films" => Media::Movie,
            "tv" | "show" | "shows" | "series" => Media::Tv,
            "game" | "games" => Media::Game,
            "album" | "albums" | "music" => Media::Album,
            "artist" | "artists" => Media::Artist,
            _ => return None,
        })
    }

    /// The Marquee list an item of this kind goes to by default.
    pub fn default_list(self) -> &'static str {
        match self {
            Media::Movie | Media::Tv => "watchlist",
            Media::Game => "play-later",
            Media::Album | Media::Artist => "listen-later",
        }
    }

    pub fn done_verb(self) -> &'static str {
        match self {
            Media::Movie | Media::Tv => "watched",
            Media::Game => "played",
            Media::Album | Media::Artist => "listened to",
        }
    }
}

/// Which vocabulary a draft's raw tags come from (they're mapped into the shared one).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TagSource {
    #[default]
    Tmdb,
    Igdb,
    Steam,
    Lastfm,
    MusicBrainz,
    Import,
    Mal,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Creator {
    pub name: String,
    /// director | writer | creator | cast | studio | network | developer | publisher | artist | label
    pub role: String,
}

impl Creator {
    pub fn new(name: impl Into<String>, role: &str) -> Self {
        Creator { name: name.into(), role: role.into() }
    }
}

/// What a catalog or integration knows about a title, before it's stored.
#[derive(Clone, Debug, Default)]
pub struct ItemDraft {
    pub media: Option<Media>,
    pub title: String,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<i64>,
    pub popularity: f64,
    pub rating: Option<f64>,
    /// (source, external id) pairs. The first is the canonical one.
    pub ids: Vec<(String, String)>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub tag_source: TagSource,
    pub creators: Vec<Creator>,
    pub language: Option<String>,
    /// Extra fields kept as JSON (providers, trailers, platforms, price, seasons...).
    pub data: Map<String, Value>,
    /// (source, external id, title) of titles the source calls similar.
    pub similar: Vec<ItemDraft>,
    /// 0 = stub from a list endpoint, 1 = full detail.
    pub detail_level: i64,
    /// Seconds until the stored copy should be refreshed.
    pub ttl_secs: i64,
}

impl ItemDraft {
    pub fn new(media: Media, title: impl Into<String>) -> Self {
        ItemDraft { media: Some(media), title: title.into(), ttl_secs: 7 * 86_400, ..Default::default() }
    }

    pub fn id(mut self, source: &str, ext: impl ToString) -> Self {
        let ext = ext.to_string();
        if !ext.is_empty() {
            self.ids.push((source.to_string(), ext));
        }
        self
    }

    pub fn media(&self) -> Media {
        self.media.unwrap_or(Media::Movie)
    }

    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        let v = value.into();
        if !v.is_null() {
            self.data.insert(key.to_string(), v);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub media: Media,
    pub title: String,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<i64>,
    pub popularity: f64,
    pub rating: Option<f64>,
    pub data: Value,
    pub detail_level: i64,
    pub ids: Map<String, Value>,
}

impl Item {
    pub fn ext(&self, source: &str) -> Option<&str> {
        self.ids.get(source).and_then(|v| v.as_str())
    }

    pub fn genres(&self) -> Vec<String> {
        self.data["genres"].as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect()).unwrap_or_default()
    }
}

/// The compact shape every feed row and grid uses.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Card {
    pub id: i64,
    pub media: String,
    pub title: String,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<i64>,
    pub rating: Option<f64>,
    pub popularity: f64,
    pub match_pct: Option<i64>,
    pub why: Vec<String>,
    pub genres: Vec<String>,
    /// Small per-media extras: artist, price, discount, platforms, providers, next_episode.
    pub meta: Value,
    pub user_rating: Option<f64>,
    pub done: bool,
    pub saved: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Rating,
    Like,
    Play,
    Watched,
    Owned,
    Wishlist,
    Dismiss,
    MoreLike,
    Pin,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Rating => "rating",
            EventKind::Like => "like",
            EventKind::Play => "play",
            EventKind::Watched => "watched",
            EventKind::Owned => "owned",
            EventKind::Wishlist => "wishlist",
            EventKind::Dismiss => "dismiss",
            EventKind::MoreLike => "more_like",
            EventKind::Pin => "pin",
        }
    }

    pub fn parse(s: &str) -> Option<EventKind> {
        Some(match s {
            "rating" => EventKind::Rating,
            "like" | "love" => EventKind::Like,
            "play" => EventKind::Play,
            "watched" | "played" | "listened" => EventKind::Watched,
            "owned" => EventKind::Owned,
            "wishlist" => EventKind::Wishlist,
            "dismiss" => EventKind::Dismiss,
            "more_like" => EventKind::MoreLike,
            "pin" => EventKind::Pin,
            _ => return None,
        })
    }
}

/// One thing the user did, as reported by a service, before the title is resolved to an item.
#[derive(Clone, Debug)]
pub struct RawEvent {
    pub draft: ItemDraft,
    pub kind: EventKind,
    pub value: f64,
    pub at: Option<String>,
    pub first_at: Option<String>,
    pub meta: Value,
}

impl RawEvent {
    pub fn new(draft: ItemDraft, kind: EventKind, value: f64) -> Self {
        RawEvent { draft, kind, value, at: None, first_at: None, meta: Value::Null }
    }

    pub fn at(mut self, at: impl Into<Option<String>>) -> Self {
        self.at = at.into();
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceStatus {
    pub id: String,
    pub name: String,
    pub status: String,
    pub account: Option<String>,
    pub last_sync: Option<String>,
    pub error: Option<String>,
    pub config: Value,
    /// False when the app-level key this service needs is missing from `.env`/Settings.
    pub configured: bool,
    pub reads: Vec<&'static str>,
    pub writes: Vec<&'static str>,
    pub note: &'static str,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListInfo {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub mirror: Option<String>,
    pub count: i64,
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Notification {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub item_id: Option<i64>,
    pub created_at: String,
    pub read: bool,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn days_from_today(days: i64) -> String {
    (chrono::Local::now() + chrono::Duration::days(days)).format("%Y-%m-%d").to_string()
}

/// "The Matrix" sorts under M.
pub fn sort_title(title: &str) -> String {
    let t = title.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = t.strip_prefix(article) {
            return rest.to_string();
        }
    }
    t
}

pub fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut dash = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn year_of(date: &str) -> Option<i32> {
    date.get(0..4)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_sort_titles() {
        assert_eq!(slug("Denis Villeneuve"), "denis-villeneuve");
        assert_eq!(slug("  A24 / Plan B!  "), "a24-plan-b");
        assert_eq!(sort_title("The Matrix"), "matrix");
        assert_eq!(sort_title("Arrival"), "arrival");
        assert_eq!(year_of("2016-11-11"), Some(2016));
        assert_eq!(year_of(""), None);
    }

    #[test]
    fn media_parses_loose_names() {
        assert_eq!(Media::parse("Films"), Some(Media::Movie));
        assert_eq!(Media::parse("series"), Some(Media::Tv));
        assert_eq!(Media::parse("???"), None);
    }
}
