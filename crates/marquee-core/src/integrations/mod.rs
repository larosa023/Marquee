//! Account integrations. Each service lives in its own module behind the `Source` trait, so the
//! sync engine and the UI treat them all the same way.

pub mod import;
pub mod lastfm;
pub mod letterboxd;
pub mod oauth;
pub mod spotify;
pub mod steam;
pub mod trakt;

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::*;
use crate::store;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A change the user made in Marquee that should also happen on a service.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WriteOp {
    /// `target`: "watchlist" (Trakt, Letterboxd), "wishlist" (Steam), "library" (Spotify)
    /// or "playlist:<id>" (Spotify).
    AddToList { item_id: i64, target: String },
    RemoveFromList { item_id: i64, target: String },
    Rate { item_id: i64, stars: f64 },
    MarkDone { item_id: i64, at: String },
    Love { item_id: i64 },
    Follow { item_id: i64 },
}

impl WriteOp {
    pub fn item_id(&self) -> i64 {
        match self {
            WriteOp::AddToList { item_id, .. } | WriteOp::RemoveFromList { item_id, .. } | WriteOp::Rate { item_id, .. } | WriteOp::MarkDone { item_id, .. } | WriteOp::Love { item_id } | WriteOp::Follow { item_id } => *item_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WriteOutcome {
    /// The service confirmed the change.
    Synced,
    /// The service has no write API for this, so Marquee opened the right page for the user to
    /// finish it. The next sync confirms it.
    Opened { url: String, hint: String },
}

/// What one sync brought back.
#[derive(Default, Debug)]
pub struct Synced {
    pub events: Vec<RawEvent>,
    /// Remote lists as (target, drafts): what is on the Trakt watchlist, Steam wishlist...
    pub lists: Vec<(String, Vec<ItemDraft>)>,
    pub account: Option<String>,
    /// Non-fatal problems worth showing ("Your Steam game details are private").
    pub warnings: Vec<String>,
}

#[async_trait]
pub trait Source: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn reads(&self) -> Vec<&'static str>;
    fn writes(&self) -> Vec<&'static str>;
    /// One line the Connections screen shows under the service name.
    fn note(&self) -> &'static str;
    /// True when the app-level keys this service needs are present.
    fn configured(&self, ctx: &Ctx) -> bool;
    /// Pulls everything the user has done on the service.
    async fn sync(&self, ctx: &Ctx) -> Result<Synced>;
    /// Pushes one change to the service.
    async fn write(&self, ctx: &Ctx, op: &WriteOp) -> Result<WriteOutcome>;
    /// Forgets tokens. Stored events are removed by the caller.
    fn forget(&self, ctx: &Ctx);
}

pub fn all() -> Vec<Box<dyn Source>> {
    vec![Box::new(steam::Steam), Box::new(letterboxd::Letterboxd), Box::new(trakt::Trakt), Box::new(spotify::Spotify), Box::new(lastfm::Lastfm)]
}

pub fn get(id: &str) -> Option<Box<dyn Source>> {
    all().into_iter().find(|s| s.id() == id)
}

/// Status of every connection, for Settings → Connections and onboarding.
pub fn statuses(ctx: &Ctx) -> Result<Vec<SourceStatus>> {
    let conn = ctx.db.lock();
    let mut out = vec![];
    for src in all() {
        let (status, account, last_sync, error) = store::source_row(&conn, src.id())?;
        out.push(SourceStatus {
            id: src.id().into(),
            name: src.name().into(),
            status,
            account,
            last_sync,
            error,
            config: store::source_config(&conn, src.id()),
            configured: src.configured(ctx),
            reads: src.reads(),
            writes: src.writes(),
            note: src.note(),
        });
    }
    for (id, name, note) in [
        ("serializd", "Serializd", "Serializd has no public API or export. Import a JSON or CSV file of your diary instead."),
        ("import", "File import", "Bring in anything as CSV or JSON: IMDb ratings, Backloggd, RateYourMusic and more."),
    ] {
        let (status, account, last_sync, error) = store::source_row(&conn, id)?;
        out.push(SourceStatus { id: id.into(), name: name.into(), status, account, last_sync, error, config: store::source_config(&conn, id), configured: true, reads: vec!["ratings", "diary", "lists"], writes: vec![], note });
    }
    Ok(out)
}

/// Item fields a write needs, read without holding the database across an await.
pub(crate) fn item_for_write(ctx: &Ctx, item_id: i64) -> Result<Item> {
    store::get_item(&ctx.db.lock(), item_id)
}

pub(crate) fn cfg(ctx: &Ctx, source: &str) -> Value {
    store::source_config(&ctx.db.lock(), source)
}

pub(crate) fn set_cfg(ctx: &Ctx, source: &str, patch: Value) -> Result<()> {
    store::set_source_config(&ctx.db.lock(), source, patch)
}
