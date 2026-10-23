-- mail.db migration 36: saved searches (Spark's Smart Folders): a search kept
-- by name, run again whenever it is opened from the sidebar, in `position`
-- order. Of every account, or of one (gone with it).

CREATE TABLE saved_searches (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    -- What was typed, in the search language (regards-core::search).
    query      TEXT NOT NULL,
    account_id TEXT REFERENCES accounts (id) ON DELETE CASCADE,
    -- "#rrggbb", or none.
    color      TEXT,
    position   INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
