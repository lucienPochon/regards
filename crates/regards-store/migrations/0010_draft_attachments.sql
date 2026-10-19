-- mail.db migration 10: files attached to drafts. The bytes live in blobs/
-- (content-addressed, ADR-0008); a row names one file on one draft.

CREATE TABLE draft_attachments (
    id         INTEGER PRIMARY KEY,
    draft_id   INTEGER NOT NULL REFERENCES drafts (id) ON DELETE CASCADE,
    blob_hash  TEXT NOT NULL,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL CHECK (size >= 0)
) STRICT;

CREATE INDEX draft_attachments_by_draft ON draft_attachments (draft_id, id);
