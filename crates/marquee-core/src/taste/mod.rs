//! The taste engine: a local, explainable recommender.
//!
//! 1. `profile::rebuild` turns everything the user has done (ratings, likes, playtime,
//!    scrobbles, watches, wishlists, dismissals) into one weighted feature vector.
//! 2. `score::score_all` compares every title in the catalog with that vector and stores a
//!    score, a match percentage and one or two plain-English reasons.
//! 3. `score::diversify` re-ranks a feed so it isn't ten copies of the same thing.
//!
//! Features come from `vocab`, which maps every service's genres and tags into one shared
//! vocabulary. That shared vocabulary is what makes recommendations cross media.

pub mod profile;
pub mod score;
pub mod vibe;
pub mod vocab;

use crate::db::Db;
use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// User-adjustable knobs, stored in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TasteSettings {
    /// 0 = Comfort (close to your history), 1 = Adventurous (wider, discovery-heavy).
    pub dial: f64,
    /// Per-source multipliers, e.g. {"steam": 1.0, "lastfm": 0.5}. Missing means 1.
    pub source_weights: HashMap<String, f64>,
}

impl Default for TasteSettings {
    fn default() -> Self {
        TasteSettings { dial: 0.35, source_weights: HashMap::new() }
    }
}

impl TasteSettings {
    pub fn load(db: &Db) -> Self {
        let mut s: TasteSettings = store::get_setting(db, "taste").unwrap_or_default();
        s.dial = s.dial.clamp(0.0, 1.0);
        s
    }

    pub fn source_weight(&self, source: &str) -> f64 {
        self.source_weights.get(source).copied().unwrap_or(1.0).clamp(0.0, 2.0)
    }
}

/// How much each kind of feature counts when scoring, given the dial.
pub fn kind_multiplier(feature: &str, dial: f64) -> f64 {
    match feature.as_bytes().first() {
        Some(b'c') => 1.5 - 0.9 * dial, // creators: safe bets, so they matter less when adventurous
        Some(b't') => 1.0 + 0.3 * dial, // mood/theme tags: how discovery crosses media
        Some(b'g') => 0.9 - 0.2 * dial,
        Some(b'e') => 0.3,
        Some(b'l') => 0.2,
        Some(b'n') => 0.2,
        _ => 0.5,
    }
}
