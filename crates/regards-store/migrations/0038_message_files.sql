-- mail.db migration 38: the files each message carries, by name and type, as
-- its structure (BODYSTRUCTURE) describes them. They are read with its
-- preview, so a search can find mail by attachment (`filename:`, `has:pdf`)
-- before any is downloaded.

CREATE TABLE message_files (
    message_id INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    -- As the sender named it, decoded and cleaned; NULL when unnamed.
    name       TEXT,
    -- "type/subtype", lowercase.
    mime       TEXT NOT NULL
) STRICT;

CREATE INDEX message_files_by_message ON message_files (message_id);

-- 1 once the files of a message carrying some were listed. Messages read
-- before this have their structure read again by the backfill, which then
-- lists them; their paperclip stays meanwhile.
ALTER TABLE messages ADD COLUMN files_read INTEGER;
