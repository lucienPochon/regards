-- mail.db migration 27: images in a draft's text (plan §5.9). Such an
-- attachment has a Content-ID, which the draft's HTML refers to as cid:.
-- `added_at` (Unix ms): an image just added is kept even if a save that
-- began before it was put in the text does not have it yet.
ALTER TABLE draft_attachments ADD COLUMN content_id TEXT;
ALTER TABLE draft_attachments ADD COLUMN added_at INTEGER;
