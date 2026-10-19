-- mail.db migration 20: templates (plan §5.9): text to start a message
-- with, kept on this device for every account.

CREATE TABLE templates (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    subject    TEXT NOT NULL DEFAULT '',
    -- The text; with `html`, its plain-text version.
    body       TEXT NOT NULL DEFAULT '',
    -- The formatted version, sanitised, if it was written with formatting.
    html       TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
