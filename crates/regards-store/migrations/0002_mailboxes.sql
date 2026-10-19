-- mail.db migration 2: the mailboxes (folders) of each account.

CREATE TABLE mailboxes (
    id             INTEGER PRIMARY KEY,
    account_id     TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    remote_name    BLOB NOT NULL,  -- the server's name byte for byte; used in commands
    name           TEXT NOT NULL,  -- decoded for display, components joined by `delimiter`
    delimiter      TEXT CHECK (delimiter IS NULL OR length(delimiter) = 1),
    role           TEXT CHECK (role IN ('inbox', 'sent', 'drafts', 'trash', 'junk', 'archive', 'all', 'flagged')),
    selectable     INTEGER NOT NULL CHECK (selectable IN (0, 1)),
    attributes     TEXT NOT NULL DEFAULT '[]',  -- LIST attributes as a JSON array
    uid_validity   INTEGER,
    uid_next       INTEGER,
    highest_modseq INTEGER,
    synced_at      INTEGER,        -- last successful message sync, Unix time in milliseconds
    UNIQUE (account_id, remote_name)
) STRICT;

-- At most one inbox, sent folder, … per account.
CREATE UNIQUE INDEX mailboxes_one_per_role ON mailboxes (account_id, role) WHERE role IS NOT NULL;
