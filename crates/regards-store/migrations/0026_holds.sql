-- mail.db migration 26: holds (ADR-0012 §3). What an agent asked that waits
-- for the person: sending, changing many messages at once, deleting for
-- good. The person approves (it then runs, as asked) or rejects it; after a
-- day it expires.
--
-- - `params`: the request as the agent made it (JSON), run as is once approved.
-- - `draft_id`, `draft_version`: for a send, the draft as it was when the
--   agent asked; one changed since is not sent on that approval.
-- - `result` or `error`: what running it gave, once approved.

CREATE TABLE holds (
    id            INTEGER PRIMARY KEY,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    session       TEXT NOT NULL,
    client        TEXT NOT NULL,
    account_id    TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    method        TEXT NOT NULL,
    params        TEXT NOT NULL,
    description   TEXT NOT NULL,
    reason        TEXT NOT NULL CHECK (reason IN ('sending', 'bulk', 'delete')),
    draft_id      INTEGER,
    draft_version INTEGER,
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'approved', 'rejected', 'failed')),
    decided_at    INTEGER,
    result        TEXT,
    error         TEXT
) STRICT;

CREATE INDEX holds_pending ON holds (expires_at) WHERE status = 'pending';

-- The activity entry of a held request, to follow its decision.
ALTER TABLE agent_audit ADD COLUMN hold_id INTEGER;

-- A new notice: an agent waits for the person. SQLite cannot change a CHECK
-- constraint in place, so the table is rebuilt; nothing references it.
CREATE TABLE notices_new (
    id          INTEGER PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('snooze_ended', 'reminder_due', 'send_held', 'send_failed',
                                              'send_uncertain', 'agent_asks')),
    message_id  INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    outbox_id   INTEGER,
    people_json TEXT NOT NULL DEFAULT '[]',
    subject     TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    shown       INTEGER NOT NULL DEFAULT 0 CHECK (shown IN (0, 1))
) STRICT;

INSERT INTO notices_new (id, account_id, kind, message_id, outbox_id, people_json, subject, created_at, shown)
SELECT id, account_id, kind, message_id, outbox_id, people_json, subject, created_at, shown FROM notices;

DROP TABLE notices;
ALTER TABLE notices_new RENAME TO notices;

CREATE INDEX notices_unshown ON notices (id) WHERE shown = 0;
CREATE INDEX notices_by_age ON notices (created_at);
