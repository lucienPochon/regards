-- mail.db migration 40: templates keep their recipients and files (plan
-- §5.9). The files' bytes are blobs (ADR-0008), kept while the template is.

ALTER TABLE templates ADD COLUMN to_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE templates ADD COLUMN cc_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE templates ADD COLUMN bcc_json TEXT NOT NULL DEFAULT '[]';

CREATE TABLE template_attachments (
    id          INTEGER PRIMARY KEY,
    template_id INTEGER NOT NULL REFERENCES templates (id) ON DELETE CASCADE,
    blob_hash   TEXT NOT NULL,
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL,
    size        INTEGER NOT NULL CHECK (size >= 0),
    -- An image in the template's text, which its HTML names as `cid:`;
    -- NULL for a file beside it.
    content_id  TEXT
) STRICT;

CREATE INDEX template_attachments_by_template ON template_attachments (template_id, id);
