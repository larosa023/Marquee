-- 0.5.0: private notes on any title, and a price you're waiting for on a game.
CREATE TABLE notes (
    item_id     INTEGER PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    text        TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE price_targets (
    item_id     INTEGER PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    cents       INTEGER NOT NULL CHECK (cents > 0),
    created_at  TEXT NOT NULL
);
