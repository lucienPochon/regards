-- mail.db migration 14: reminders, and mail that comes back to the top of the
-- inbox (ADR-0011 §3–4).
--
-- - `remind_at`: "remind me if no reply by" this time (Unix ms), usually on
--   a message the person sent.
-- - `reminded_at`: the time came with no reply. The message shows in the
--   inbox wherever it is filed (Sent, usually) until it is done.
-- - `surfaced_at`: when it came back to the inbox (a snooze ended, or a
--   reminder came due). The inbox lists it by this time, so it is on top.

ALTER TABLE marks ADD COLUMN remind_at INTEGER;
ALTER TABLE marks ADD COLUMN reminded_at INTEGER;
ALTER TABLE marks ADD COLUMN surfaced_at INTEGER;

CREATE INDEX marks_remind ON marks (remind_at) WHERE remind_at IS NOT NULL;
CREATE INDEX marks_surfaced ON marks (account_id, surfaced_at) WHERE surfaced_at IS NOT NULL;
