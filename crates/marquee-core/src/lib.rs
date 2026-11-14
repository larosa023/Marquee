//! Marquee's engine. Everything that isn't UI lives here: the catalog, account integrations,
//! the taste engine, sync, notifications and data handling. It has no Tauri or Windows code,
//! so it builds and tests on any platform.

pub mod ai;
pub mod api;
pub mod backlog;
pub mod backup;
pub mod catalog;
pub mod ctx;
pub mod db;
pub mod error;
pub mod export;
pub mod feeds;
pub mod hangar;
pub mod images;
pub mod integrations;
pub mod model;
pub mod net;
pub mod notifications;
pub mod secrets;
pub mod store;
pub mod sync;
pub mod taste;
pub mod themes;
pub mod updater;
pub mod util;
pub mod wrapped;

pub use ctx::{Ctx, Host};
pub use error::{Error, Result};
