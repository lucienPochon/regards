-- mail.db migration 3: messages.
-- One row per message per mailbox (an IMAP mailbox and UID). The same message
-- in two mailboxes is two rows; threading links them through `message_id_header`.

CREATE TABLE messages (
    id                INTEGER PRIMARY KEY,
    mailbox_id        INTEGER NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    uid               INTEGER NOT NULL CHECK (uid > 0),
    modseq            INTEGER,
    seen              INTEGER NOT NULL DEFAULT 0 CHECK (seen IN (0, 1)),
    answered          INTEGER NOT NULL DEFAULT 0 CHECK (answered IN (0, 1)),
    flagged           INTEGER NOT NULL DEFAULT 0 CHECK (flagged IN (0, 1)),
    draft             INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0, 1)),
    deleted           INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    keywords          TEXT NOT NULL DEFAULT '[]',  -- JSON array of other flags
    received_at       INTEGER NOT NULL,            -- INTERNALDATE (else the Date header), Unix ms
    size              INTEGER,
    sent_at           INTEGER,                     -- Date header, Unix time in milliseconds
    subject           TEXT NOT NULL DEFAULT '',
    from_name         TEXT,
    from_address      TEXT,
    to_json           TEXT NOT NULL DEFAULT '[]',  -- JSON array of {name, address}
    cc_json           TEXT NOT NULL DEFAULT '[]',
    message_id_header TEXT,
    in_reply_to       TEXT NOT NULL DEFAULT '[]',  -- JSON array of Message-IDs
    references_ids    TEXT NOT NULL DEFAULT '[]',  -- JSON array of Message-IDs
    UNIQUE (mailbox_id, uid)
) STRICT;

-- Message lists: newest first within a mailbox.
CREATE INDEX messages_by_date ON messages (mailbox_id, received_at DESC, id DESC);
-- Threading.
CREATE INDEX messages_by_message_id ON messages (message_id_header) WHERE message_id_header IS NOT NULL;
