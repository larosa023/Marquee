-- 0.3.0: indexes for the lookups every feed makes ("is this on a list?", "is it followed?",
-- best matches first), and for the TV / Anime split.
CREATE INDEX IF NOT EXISTS list_items_item ON list_items (item_id);
CREATE INDEX IF NOT EXISTS follows_item ON follows (item_id);
CREATE INDEX IF NOT EXISTS scores_score ON scores (score DESC);
CREATE INDEX IF NOT EXISTS user_events_item_kind ON user_events (item_id, kind);
CREATE INDEX IF NOT EXISTS items_popularity ON items (media, popularity DESC);
