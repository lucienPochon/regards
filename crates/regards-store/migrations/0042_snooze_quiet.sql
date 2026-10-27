-- mail.db migration 42: a snooze can come back quietly. Set with the snooze
-- (1) and cleared when it ends: the message comes back unread, without a
-- notification.

ALTER TABLE marks ADD COLUMN snooze_quiet INTEGER;
