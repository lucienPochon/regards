-- mail.db migration 12: marks the person puts on messages, kept on this
-- device (plan §5.5–5.6): pinned, set aside, snoozed. Keyed by Message-ID, so
-- a mark follows its message across folders, moves and resyncs (ADR-0010 §2).
-- A row with no mark left is deleted.

CREATE TABLE marks (
    account_id    TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    message_id    TEXT NOT NULL,
    pinned_at     INTEGER,
    aside_at      INTEGER,
    -- Until when it is out of the inbox (Unix ms); far future for "someday".
    snoozed_until INTEGER,
    PRIMARY KEY (account_id, message_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX marks_snoozed ON marks (snoozed_until) WHERE snoozed_until IS NOT NULL;
