-- mail.db migration 21: drafts kept in the server's Drafts folder too
-- (plan §5.9, IMAP draft sync), so other devices and webmail have them.
--
-- - drafts.server_*: the copy of the draft in the Drafts folder. Its
--   Message-ID stays the same from one upload to the next; `uploaded_at` is
--   the version (`updated_at`) it holds.
-- - draft_copies_to_delete: copies to remove from the server, because their
--   draft went (sent, discarded) or a newer copy replaced them. A trigger
--   records them whenever a draft is deleted, whatever the reason.

ALTER TABLE drafts ADD COLUMN server_message_id TEXT;
ALTER TABLE drafts ADD COLUMN server_mailbox_id INTEGER;
ALTER TABLE drafts ADD COLUMN server_uid INTEGER;
ALTER TABLE drafts ADD COLUMN uploaded_at INTEGER;

CREATE TABLE draft_copies_to_delete (
    id         INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL,
    mailbox_id INTEGER NOT NULL,
    uid        INTEGER NOT NULL
) STRICT;

CREATE TRIGGER drafts_leave_no_copy AFTER DELETE ON drafts
WHEN OLD.server_uid IS NOT NULL
BEGIN
    INSERT INTO draft_copies_to_delete (account_id, mailbox_id, uid)
    VALUES (OLD.account_id, OLD.server_mailbox_id, OLD.server_uid);
END;
