-- mail.db migration 19: new-sender screening and blocking (plan §5.4).
--
-- - senders.blocked: the person blocked the sender (or, under an `@domain`
--   key, a whole domain): their mail goes to Junk.
-- - settings: small choices the daemon keeps, by key (`screening`: off,
--   section or hold).

ALTER TABLE senders ADD COLUMN blocked INTEGER NOT NULL DEFAULT 0 CHECK (blocked IN (0, 1));

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;
