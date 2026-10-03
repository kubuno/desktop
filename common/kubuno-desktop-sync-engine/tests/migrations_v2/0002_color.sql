-- Adds a synced column: the feed must be re-pulled so existing rows get it.
ALTER TABLE notes ADD COLUMN color TEXT;
