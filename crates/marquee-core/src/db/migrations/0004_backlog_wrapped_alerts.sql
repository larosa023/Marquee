-- 0.2.0: a link on notifications ("Where to watch"), and a daily log of game playtime so a
-- year in review can count hours played in that year (Steam only reports lifetime totals).
ALTER TABLE notifications ADD COLUMN url TEXT;

CREATE TABLE playtime_log (
    item_id  INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    day      TEXT NOT NULL,          -- YYYY-MM-DD
    minutes  REAL NOT NULL,          -- lifetime minutes as of that day
    PRIMARY KEY (item_id, day)
);
