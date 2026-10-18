-- mail.db migration 4: where a message's full source is stored once downloaded.
-- The value is the SHA-256 (hex) of the source, the file name under `blobs/`.
ALTER TABLE messages ADD COLUMN blob_hash TEXT CHECK (blob_hash IS NULL OR length(blob_hash) = 64);
