-- mail.db migration 15: what to tell the person (ADR-0011): a snooze ended,
-- a reminder came due, a scheduled message was held, a message was not sent.
-- The desktop shows each once as a notification, and marks it shown; what
-- piled up while it was closed becomes one summary. Kept a week.
--
-- Who and what are copied in, so a notice still reads right after its message
-- moved or went. Never the message's text.

CREATE TABLE notices (
    id          INTEGER PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('snooze_ended', 'reminder_due', 'send_held', 'send_failed', 'send_uncertain')),
    -- The stored message it is about, or the outbox entry.
    message_id  INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    outbox_id   INTEGER,
    people_json TEXT NOT NULL DEFAULT '[]',   -- the sender, or the recipients
    subject     TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    shown       INTEGER NOT NULL DEFAULT 0 CHECK (shown IN (0, 1))
) STRICT;

CREATE INDEX notices_unshown ON notices (id) WHERE shown = 0;
CREATE INDEX notices_by_age ON notices (created_at);
