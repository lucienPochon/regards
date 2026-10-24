-- mail.db migration 37: Gmail labels, which Gmail shows over IMAP as folders.
-- A message has a label when a copy of it is in that label's folder. Adding
-- a label copies the message there (a `copy` operation); removing one deletes
-- that copy, which Gmail takes as removing the label (the mail stays in All
-- Mail).

-- Operations gain `copy`, which, like `move`, has a target. SQLite cannot
-- change a CHECK constraint in place, so the table is rebuilt, as in
-- migration 8.
CREATE TABLE operations_new (
    id                INTEGER PRIMARY KEY,
    mailbox_id        INTEGER NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    uid_validity      INTEGER NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('add_flags', 'remove_flags', 'move', 'delete', 'copy')),
    uids              TEXT NOT NULL,
    flags             TEXT NOT NULL DEFAULT '[]',
    target_mailbox_id INTEGER REFERENCES mailboxes (id) ON DELETE CASCADE,
    created_at        INTEGER NOT NULL,
    attempts          INTEGER NOT NULL DEFAULT 0,
    next_attempt_at   INTEGER NOT NULL,
    last_error        TEXT,
    CHECK ((kind IN ('move', 'copy')) = (target_mailbox_id IS NOT NULL))
) STRICT;

INSERT INTO operations_new
    SELECT id, mailbox_id, uid_validity, kind, uids, flags, target_mailbox_id, created_at, attempts,
           next_attempt_at, last_error
    FROM operations;
DROP TABLE operations;
ALTER TABLE operations_new RENAME TO operations;
CREATE INDEX operations_by_mailbox ON operations (mailbox_id, id);

-- A label added on this device, until the copy comes back from the label's
-- folder in a sync: then the copy itself says the message has it.
CREATE TABLE label_adds (
    account_id        TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    message_id_header TEXT NOT NULL,
    mailbox_id        INTEGER NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    added_at          INTEGER NOT NULL,
    PRIMARY KEY (account_id, message_id_header, mailbox_id)
) STRICT;
