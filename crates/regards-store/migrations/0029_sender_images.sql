-- mail.db migration 29: remote images, allowed per sender (ADR-0015 §4).
-- Blocked by default; the person can let a sender's (an address's, or a
-- whole `@domain`'s) load from then on.

ALTER TABLE senders ADD COLUMN images INTEGER NOT NULL DEFAULT 0 CHECK (images IN (0, 1));
