-- mail.db migration 41: quiet (plan §5.12). A conversation muted (a mark on
-- its messages, as pins are) or a sender made quiet (or a whole `@domain`)
-- tells of no new mail: it still comes in, without a notification.

ALTER TABLE marks ADD COLUMN muted_at INTEGER;
ALTER TABLE senders ADD COLUMN quiet INTEGER NOT NULL DEFAULT 0 CHECK (quiet IN (0, 1));
