-- mail.db migration 43: a conversation made a priority (a mark on its
-- messages, as muting is): its mail, and what comes in it later, goes in
-- Priority, whoever sends it.

ALTER TABLE marks ADD COLUMN priority_at INTEGER;
