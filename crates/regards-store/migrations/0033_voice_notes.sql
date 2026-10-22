-- mail.db migration 33: voices and notes (ADR-0020).
--
-- - A voice per identity: how agents should write as that address. Only the
--   person changes it; switching it off keeps the text.
-- - Notes about the people an account writes with: one fact each, by the
--   person or an agent, with the message it was drawn from.
-- - Two new hold reasons: an agent's suggested voice, and a note to look at
--   before it is kept.

ALTER TABLE identities ADD COLUMN voice TEXT NOT NULL DEFAULT '';
ALTER TABLE identities ADD COLUMN voice_on INTEGER NOT NULL DEFAULT 0 CHECK (voice_on IN (0, 1));

CREATE TABLE notes (
    id                INTEGER PRIMARY KEY,
    account_id        TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    -- The person's address, lower case.
    address           TEXT NOT NULL,
    text              TEXT NOT NULL,
    author            TEXT NOT NULL CHECK (author IN ('person', 'agent')),
    -- For a note an agent wrote: its session.
    session           TEXT,
    -- The message it was drawn from, when given: by Message-ID, subject and
    -- sender, so it survives the message moving or going.
    source_message_id TEXT,
    source_subject    TEXT,
    source_from       TEXT,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
) STRICT;

CREATE INDEX notes_by_person ON notes (account_id, address, created_at);

-- SQLite cannot change a CHECK constraint in place, so the table is rebuilt;
-- nothing references it.
CREATE TABLE holds_new (
    id            INTEGER PRIMARY KEY,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    session       TEXT NOT NULL,
    client        TEXT NOT NULL,
    account_id    TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    method        TEXT NOT NULL,
    params        TEXT NOT NULL,
    description   TEXT NOT NULL,
    reason        TEXT NOT NULL CHECK (reason IN ('sending', 'bulk', 'delete', 'voice', 'note')),
    draft_id      INTEGER,
    draft_version INTEGER,
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'approved', 'rejected', 'failed')),
    decided_at    INTEGER,
    result        TEXT,
    error         TEXT
) STRICT;

INSERT INTO holds_new (id, created_at, expires_at, session, client, account_id, method, params, description,
                       reason, draft_id, draft_version, status, decided_at, result, error)
SELECT id, created_at, expires_at, session, client, account_id, method, params, description,
       reason, draft_id, draft_version, status, decided_at, result, error
FROM holds;

DROP TABLE holds;
ALTER TABLE holds_new RENAME TO holds;

CREATE INDEX holds_pending ON holds (expires_at) WHERE status = 'pending';
