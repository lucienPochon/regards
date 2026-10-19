-- mail.db migration 8: deleting messages for good (ADR-0010).

-- Operations gain `delete` (\Deleted, then UID EXPUNGE of exactly those
-- messages). SQLite cannot change a CHECK constraint in place, so the table
-- is rebuilt, as in migration 6.
CREATE TABLE operations_new (
    id                INTEGER PRIMARY KEY,
    mailbox_id        INTEGER NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    uid_validity      INTEGER NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('add_flags', 'remove_flags', 'move', 'delete')),
    uids              TEXT NOT NULL,
    flags             TEXT NOT NULL DEFAULT '[]',
    target_mailbox_id INTEGER REFERENCES mailboxes (id) ON DELETE CASCADE,
    created_at        INTEGER NOT NULL,
    attempts          INTEGER NOT NULL DEFAULT 0,
    next_attempt_at   INTEGER NOT NULL,
    last_error        TEXT,
    CHECK ((kind = 'move') = (target_mailbox_id IS NOT NULL))
) STRICT;

INSERT INTO operations_new
    SELECT id, mailbox_id, uid_validity, kind, uids, flags, target_mailbox_id, created_at, attempts,
           next_attempt_at, last_error
    FROM operations;
DROP TABLE operations;
ALTER TABLE operations_new RENAME TO operations;
CREATE INDEX operations_by_mailbox ON operations (mailbox_id, id);
