-- Search (plan §5.10): every folder's messages newest first, for searches
-- that match much of the mail (the index itself is in search.db). It also
-- holds what searches filter on, so a walk reads only the index until it
-- finds what passes.
CREATE INDEX messages_by_time ON messages
    (received_at DESC, id DESC, mailbox_id, moving_to, deleted, seen, flagged, has_attachments);
