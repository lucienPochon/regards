-- mail.db migration 6: moving messages, and undo (ADR-0007, ADR-0010).

-- Where a message is being moved to, until the server has done it. Lists show
-- such a message in the target folder at once.
ALTER TABLE messages ADD COLUMN moving_to INTEGER REFERENCES mailboxes (id) ON DELETE SET NULL;
CREATE INDEX messages_moving_to ON messages (moving_to, received_at DESC, id DESC) WHERE moving_to IS NOT NULL;

-- Operations gain moves. SQLite cannot change a CHECK constraint in place,
-- so the table is rebuilt.
CREATE TABLE operations_new (
    id                INTEGER PRIMARY KEY,
    mailbox_id        INTEGER NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    uid_validity      INTEGER NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('add_flags', 'remove_flags', 'move')),
    uids              TEXT NOT NULL,
    flags             TEXT NOT NULL DEFAULT '[]',
    -- For moves: the folder the messages go to.
    target_mailbox_id INTEGER REFERENCES mailboxes (id) ON DELETE CASCADE,
    created_at        INTEGER NOT NULL,
    attempts          INTEGER NOT NULL DEFAULT 0,
    next_attempt_at   INTEGER NOT NULL,
    last_error        TEXT,
    CHECK ((kind = 'move') = (target_mailbox_id IS NOT NULL))
) STRICT;

INSERT INTO operations_new (id, mailbox_id, uid_validity, kind, uids, flags, created_at, attempts, next_attempt_at, last_error)
    SELECT id, mailbox_id, uid_validity, kind, uids, flags, created_at, attempts, next_attempt_at, last_error FROM operations;
DROP TABLE operations;
ALTER TABLE operations_new RENAME TO operations;
CREATE INDEX operations_by_mailbox ON operations (mailbox_id, id);

-- How to undo recent actions: the inverse, as the intent that reverses it.
CREATE TABLE undo_log (
    id          INTEGER PRIMARY KEY,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    description TEXT NOT NULL,  -- for the toast, for example "Archived 2 messages"
    inverse     TEXT NOT NULL   -- JSON
) STRICT;
