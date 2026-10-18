-- mail.db migration 5: the operation queue (ADR-0010).
-- A user action changes the local copy at once and records here what the
-- server still has to be told. A runner per account sends them in order.

CREATE TABLE operations (
    id              INTEGER PRIMARY KEY,
    mailbox_id      INTEGER NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    -- The mailbox's UIDVALIDITY when the action was taken: UIDs mean nothing under another.
    uid_validity    INTEGER NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('add_flags', 'remove_flags')),
    uids            TEXT NOT NULL,  -- JSON array of UIDs
    flags           TEXT NOT NULL,  -- JSON array: "seen", "flagged", "answered", "draft", "deleted"
    created_at      INTEGER NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    last_error      TEXT
) STRICT;

CREATE INDEX operations_by_mailbox ON operations (mailbox_id, id);
