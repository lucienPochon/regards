-- mail.db migration 30: new mail to tell the person of (plan §5.12).
--
-- Sync adds the inbox messages that came in since it last looked: never a
-- folder's first look (an account just added), nor mail already read. The
-- app tells of them as the person chose per account, then marks them shown.
-- They are kept a day.

CREATE TABLE arrivals (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    account_id  TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    arrived_at  INTEGER NOT NULL,
    shown       INTEGER NOT NULL DEFAULT 0 CHECK (shown IN (0, 1))
) STRICT;

CREATE INDEX arrivals_unshown ON arrivals (id) WHERE shown = 0;
CREATE INDEX arrivals_by_age ON arrivals (arrived_at);
