-- mail.db migration 44: a calendar part's method, as its structure gives it
-- (`request`, `reply`, `cancel`…): an invitation to answer, or an answer
-- to one. Calendar mail stored before has its structure read again (its
-- details as yet unknown), which records the method.

ALTER TABLE message_files ADD COLUMN method TEXT;

UPDATE messages SET has_attachments = NULL WHERE id IN (
    SELECT message_id FROM message_files
    WHERE mime IN ('text/calendar', 'application/ics') OR lower(name) LIKE '%.ics'
);
