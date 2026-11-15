-- 0.4.0: building your own taste. Besides pin (love) and block (never), a genre, theme or
-- creator can be marked "like" (a nudge up) or "dislike" (a nudge down, not banned).
CREATE TABLE taste_overrides_new (
    feature  TEXT PRIMARY KEY,
    mode     TEXT NOT NULL CHECK (mode IN ('pin', 'like', 'dislike', 'block'))
) WITHOUT ROWID;
INSERT INTO taste_overrides_new (feature, mode) SELECT feature, mode FROM taste_overrides;
DROP TABLE taste_overrides;
ALTER TABLE taste_overrides_new RENAME TO taste_overrides;
