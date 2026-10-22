-- mail.db migration 35: a person one asked not to be suggested any more (a
-- mistyped address, say), until there is new mail with them: `forgotten_at`
-- is when, and `last_at` past it brings them back.

ALTER TABLE people ADD COLUMN forgotten_at INTEGER;
