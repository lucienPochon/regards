-- mail.db migration 28: an agent's sends can go with an undo window
-- instead of waiting for approval (ADR-0012 §3), per account.
--
-- - `accounts.agent_send_window`: the seconds an agent's send waits, during
--   which the person can call it back, before it goes. NULL (the default):
--   each send waits for the person's approval.
-- - A new notice, `agent_sending`: an agent's message is on its way and can
--   still be called back. SQLite cannot change a CHECK constraint in place,
--   so the notices table is rebuilt; nothing references it.

ALTER TABLE accounts ADD COLUMN agent_send_window INTEGER
    CHECK (agent_send_window IS NULL OR agent_send_window BETWEEN 10 AND 600);

CREATE TABLE notices_new (
    id          INTEGER PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('snooze_ended', 'reminder_due', 'send_held', 'send_failed',
                                              'send_uncertain', 'agent_asks', 'agent_sending')),
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
