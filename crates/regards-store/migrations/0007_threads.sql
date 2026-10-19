-- mail.db migration 7: conversations, by References threading (ADR-0009 §5).

CREATE TABLE threads (
    id         INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE
) STRICT;

-- Every Message-ID a message has or refers to (In-Reply-To, References),
-- and the thread it belongs to. Message-IDs are per account.
CREATE TABLE thread_ids (
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    thread_id  INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, message_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX thread_ids_by_thread ON thread_ids (thread_id);

-- NULL until threaded; messages synced before this migration are threaded in the background.
ALTER TABLE messages ADD COLUMN thread_id INTEGER REFERENCES threads (id) ON DELETE SET NULL;
CREATE INDEX messages_by_thread ON messages (thread_id);
