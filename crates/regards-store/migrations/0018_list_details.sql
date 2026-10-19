-- mail.db migration 18: what the list shows beyond the headers.
--
-- - Unread counts per folder come from this index, so they cost little even
--   for a large inbox.
-- - has_attachments and preview come from the message's structure and the
--   start of its text, fetched at sync. NULL for mail stored before, until
--   they are fetched.

CREATE INDEX messages_unseen ON messages (mailbox_id) WHERE seen = 0 AND deleted = 0;

ALTER TABLE messages ADD COLUMN has_attachments INTEGER CHECK (has_attachments IN (0, 1));
ALTER TABLE messages ADD COLUMN preview TEXT;

CREATE INDEX messages_details_unknown ON messages (mailbox_id) WHERE has_attachments IS NULL;
