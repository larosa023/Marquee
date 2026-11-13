-- Marquee schema v1.
-- Every later change goes in a new numbered file; never edit a shipped migration.

CREATE TABLE items (
    id            INTEGER PRIMARY KEY,
    media         TEXT NOT NULL CHECK (media IN ('movie','tv','game','album','artist')),
    title         TEXT NOT NULL,
    sort_title    TEXT NOT NULL,
    year          INTEGER,
    release_date  TEXT,              -- ISO date (YYYY-MM-DD), region-aware where the source allows
    poster        TEXT,
    backdrop      TEXT,
    overview      TEXT,
    runtime       INTEGER,           -- minutes (movies, episodes), hours-to-beat * 60 (games), album length
    popularity    REAL NOT NULL DEFAULT 0,
    rating        REAL,              -- 0..10 from the catalog source
    data          TEXT NOT NULL DEFAULT '{}',  -- JSON: genres, creators, providers, trailers, platforms, price...
    detail_level  INTEGER NOT NULL DEFAULT 0,  -- 0 = list stub, 1 = full detail fetched
    updated_at    TEXT NOT NULL,
    expires_at    TEXT
);
CREATE INDEX items_media_date ON items (media, release_date);
CREATE INDEX items_title ON items (sort_title);

-- External ids: one item can be known to many services.
CREATE TABLE item_ids (
    source   TEXT NOT NULL,          -- tmdb, imdb, igdb, steam, mbid, spotify, trakt, lastfm, letterboxd
    ext_id   TEXT NOT NULL,
    media    TEXT NOT NULL,
    item_id  INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    PRIMARY KEY (source, media, ext_id)
);
CREATE INDEX item_ids_item ON item_ids (item_id);

-- Features the taste engine scores on. feature = "<kind>:<value>" (g: genre, t: shared tag,
-- c: creator, e: era, l: language, n: length bucket).
CREATE TABLE item_features (
    item_id  INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    feature  TEXT NOT NULL,
    weight   REAL NOT NULL DEFAULT 1,
    PRIMARY KEY (item_id, feature)
) WITHOUT ROWID;
CREATE INDEX item_features_feature ON item_features (feature);

-- "Similar to" edges reported by catalog sources.
CREATE TABLE item_similar (
    item_id     INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    similar_id  INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    PRIMARY KEY (item_id, similar_id)
) WITHOUT ROWID;

CREATE TABLE sources (
    id         TEXT PRIMARY KEY,     -- steam, letterboxd, trakt, spotify, lastfm, serializd, import
    status     TEXT NOT NULL DEFAULT 'disconnected',  -- disconnected | connected | syncing | error
    account    TEXT,
    last_sync  TEXT,
    error      TEXT,
    config     TEXT NOT NULL DEFAULT '{}'   -- non-secret settings only (usernames, steamid, playlist ids)
);

-- Everything the user did, from every service. One row per (item, source, kind).
CREATE TABLE user_events (
    id       INTEGER PRIMARY KEY,
    item_id  INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    source   TEXT NOT NULL,
    kind     TEXT NOT NULL,          -- rating | like | play | watched | owned | wishlist | dismiss | more_like | pin
    value    REAL NOT NULL DEFAULT 1,  -- stars (0.5..5), minutes played, scrobble count...
    at       TEXT NOT NULL,          -- when it last happened
    first_at TEXT,                   -- when it first happened (sustained vs one-off activity)
    meta     TEXT NOT NULL DEFAULT '{}',
    UNIQUE (item_id, source, kind)
);
CREATE INDEX user_events_item ON user_events (item_id);
CREATE INDEX user_events_kind ON user_events (kind, at);

CREATE TABLE lists (
    id          INTEGER PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'custom',   -- system | custom
    mirror      TEXT,                -- trakt:watchlist | steam:wishlist | spotify:<playlist id> | NULL
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL
);

CREATE TABLE list_items (
    list_id     INTEGER NOT NULL REFERENCES lists (id) ON DELETE CASCADE,
    item_id     INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL DEFAULT 0,
    added_at    TEXT NOT NULL,
    sync_state  TEXT NOT NULL DEFAULT 'local',    -- local | synced | pending | failed
    note        TEXT,
    PRIMARY KEY (list_id, item_id)
);

CREATE TABLE follows (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,       -- item | artist | franchise | person
    ref         TEXT NOT NULL,       -- item id, tmdb collection id, tmdb person id...
    name        TEXT NOT NULL,
    item_id     INTEGER REFERENCES items (id) ON DELETE CASCADE,
    remind      INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,
    UNIQUE (kind, ref)
);

CREATE TABLE notifications (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,       -- release | season | album | sale | digest | system
    dedupe      TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    item_id     INTEGER REFERENCES items (id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL,
    read_at     TEXT,
    toasted     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX notifications_created ON notifications (created_at DESC);

-- HTTP response cache (TTL + ETag revalidation).
CREATE TABLE cache (
    key         TEXT PRIMARY KEY,
    body        BLOB NOT NULL,
    etag        TEXT,
    fetched_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL
) WITHOUT ROWID;

-- On-disk image cache index (the files live in <data>/images).
CREATE TABLE image_cache (
    key          TEXT PRIMARY KEY,
    bytes        INTEGER NOT NULL,
    mime         TEXT NOT NULL,
    last_access  TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL            -- JSON
) WITHOUT ROWID;

-- Writes made while offline (or that failed) and still need replaying.
CREATE TABLE pending_writes (
    id          INTEGER PRIMARY KEY,
    source      TEXT NOT NULL,
    op          TEXT NOT NULL,       -- JSON-encoded WriteOp
    created_at  TEXT NOT NULL,
    attempts    INTEGER NOT NULL DEFAULT 0,
    last_error  TEXT
);

-- The computed taste profile (rebuilt after every sync and rating).
CREATE TABLE taste_profile (
    feature  TEXT PRIMARY KEY,
    weight   REAL NOT NULL,
    support  INTEGER NOT NULL DEFAULT 0   -- how many items back this feature
) WITHOUT ROWID;

-- User edits to the profile: pinned loves and blocked genres/creators/tags.
CREATE TABLE taste_overrides (
    feature  TEXT PRIMARY KEY,
    mode     TEXT NOT NULL CHECK (mode IN ('pin','block'))
) WITHOUT ROWID;

-- Cached scores so feeds render instantly.
CREATE TABLE scores (
    item_id     INTEGER PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    score       REAL NOT NULL,
    match_pct   INTEGER NOT NULL,
    why         TEXT NOT NULL DEFAULT '[]',
    computed_at TEXT NOT NULL
);

CREATE TABLE price_history (
    item_id   INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    at        TEXT NOT NULL,
    cents     INTEGER NOT NULL,
    discount  INTEGER NOT NULL DEFAULT 0,
    currency  TEXT NOT NULL DEFAULT 'USD',
    PRIMARY KEY (item_id, at)
) WITHOUT ROWID;

CREATE TABLE embeddings (
    item_id  INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    model    TEXT NOT NULL,
    vector   BLOB NOT NULL,          -- little-endian f32s
    PRIMARY KEY (item_id, model)
) WITHOUT ROWID;

INSERT INTO lists (slug, name, kind, position, created_at) VALUES
    ('watchlist',    'Watchlist',    'system', 0, datetime('now')),
    ('play-later',   'Play Later',   'system', 1, datetime('now')),
    ('listen-later', 'Listen Later', 'system', 2, datetime('now')),
    ('favorites',    'Favorites',    'system', 3, datetime('now'));

INSERT INTO sources (id) VALUES ('steam'), ('letterboxd'), ('trakt'), ('spotify'), ('lastfm'), ('serializd'), ('import');
