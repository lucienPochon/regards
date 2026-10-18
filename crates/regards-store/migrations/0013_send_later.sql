-- mail.db migration 13: send later (ADR-0011 §2, plan §5.7).
--
-- - `send_at`: when the person asked for it to go (Unix ms), or NULL for "as
--   soon as the undo window is over".
-- - `held`: a scheduled message found too late (the computer was off or
--   asleep at its time) waits for the person instead of going out unasked.
-- - `executor`: who sends it. 1.0 has only `device`; the other names and
--   `handed_off` (a remote executor holds it) are reserved for 1.1, so adding
--   them needs no new table.
--
-- SQLite cannot change a CHECK constraint in place, so the table is rebuilt.
-- Nothing references the outbox, so dropping the old one is safe with foreign
-- keys on.

CREATE TABLE outbox_new (
    id               INTEGER PRIMARY KEY,
    account_id       TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    draft_id         INTEGER REFERENCES drafts (id) ON DELETE SET NULL,
    message_id       TEXT NOT NULL,
    subject          TEXT NOT NULL,
    recipients_json  TEXT NOT NULL,
    envelope_from    TEXT NOT NULL,
    envelope_to      TEXT NOT NULL,
    mime             BLOB NOT NULL,
    mime_sha256      TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('queued', 'held', 'handed_off', 'sending', 'sent', 'failed', 'uncertain')),
    executor         TEXT NOT NULL DEFAULT 'device' CHECK (executor IN ('device', 'provider', 'self_hosted', 'send_service')),
    send_at          INTEGER,
    undo_until       INTEGER NOT NULL,
    next_attempt_at  INTEGER NOT NULL,
    attempts         INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    server_reply     TEXT,
    sent_at          INTEGER,
    copied           INTEGER NOT NULL DEFAULT 0 CHECK (copied IN (0, 1)),
    answering        INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    created_at       INTEGER NOT NULL
) STRICT;

INSERT INTO outbox_new (id, account_id, draft_id, message_id, subject, recipients_json, envelope_from, envelope_to,
                        mime, mime_sha256, state, undo_until, next_attempt_at, attempts, last_error, server_reply,
                        sent_at, copied, answering, created_at)
SELECT id, account_id, draft_id, message_id, subject, recipients_json, envelope_from, envelope_to,
       mime, mime_sha256, state, undo_until, next_attempt_at, attempts, last_error, server_reply,
       sent_at, copied, answering, created_at
FROM outbox;

DROP TABLE outbox;
ALTER TABLE outbox_new RENAME TO outbox;

CREATE INDEX outbox_by_state ON outbox (state, next_attempt_at);
CREATE UNIQUE INDEX outbox_one_per_draft ON outbox (draft_id) WHERE draft_id IS NOT NULL;
