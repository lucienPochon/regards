-- mail.db migration 9: drafts and the outbox (ADR-0010 §3–4).

-- What is being written. A draft may answer a stored message (to mark it
-- \Answered once sent).
CREATE TABLE drafts (
    id               INTEGER PRIMARY KEY,
    account_id       TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    to_json          TEXT NOT NULL DEFAULT '[]',
    cc_json          TEXT NOT NULL DEFAULT '[]',
    bcc_json         TEXT NOT NULL DEFAULT '[]',
    subject          TEXT NOT NULL DEFAULT '',
    body             TEXT NOT NULL DEFAULT '',
    in_reply_to      TEXT,
    references_json  TEXT NOT NULL DEFAULT '[]',
    replying_to      INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
) STRICT;

CREATE INDEX drafts_by_account ON drafts (account_id, updated_at DESC);

-- What is being sent. The exact bytes are frozen when the draft is queued;
-- editing first takes the entry back out (undo), so preview and send never
-- differ. `sending` is entered by one atomic claim; a crash or a lost
-- connection after the message was handed over leaves `uncertain`, which is
-- never retried without the user (at-most-once).
CREATE TABLE outbox (
    id               INTEGER PRIMARY KEY,
    account_id       TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    draft_id         INTEGER REFERENCES drafts (id) ON DELETE SET NULL,
    message_id       TEXT NOT NULL,
    subject          TEXT NOT NULL,
    recipients_json  TEXT NOT NULL,                 -- for display: who it goes to
    envelope_from    TEXT NOT NULL,
    envelope_to      TEXT NOT NULL,                 -- JSON array of addresses, Bcc included
    mime             BLOB NOT NULL,
    mime_sha256      TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('queued', 'sending', 'sent', 'failed', 'uncertain')),
    undo_until       INTEGER NOT NULL,
    next_attempt_at  INTEGER NOT NULL,
    attempts         INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    server_reply     TEXT,
    sent_at          INTEGER,
    -- Once sent: whether the copy in the Sent folder is done (or not needed).
    copied           INTEGER NOT NULL DEFAULT 0 CHECK (copied IN (0, 1)),
    answering        INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    created_at       INTEGER NOT NULL
) STRICT;

CREATE INDEX outbox_by_state ON outbox (state, next_attempt_at);
CREATE UNIQUE INDEX outbox_one_per_draft ON outbox (draft_id) WHERE draft_id IS NOT NULL;
