-- mail.db migration 17: the Smart Inbox (plan §5.4).
--
-- - messages.traits: how it was sent, read from its headers
--   (regards_core::smart::traits). NULL for mail stored before, until its
--   headers are read again.
-- - messages.section: its Smart Inbox section, kept current as the person's
--   choices and the account's correspondence change. NULL until classified.
-- - senders: per account and address (lowercase), the section the person
--   chose for them, whether they are a priority, and when the account first
--   wrote to them (the relationship that turns a new sender into a person).

ALTER TABLE messages ADD COLUMN traits INTEGER;
ALTER TABLE messages ADD COLUMN section TEXT
    CHECK (section IN ('priority', 'people', 'new_senders', 'notifications', 'newsletters'));

CREATE INDEX messages_by_section ON messages (mailbox_id, section, received_at DESC, id DESC);
CREATE INDEX messages_by_sender ON messages (lower(from_address)) WHERE from_address IS NOT NULL;
CREATE INDEX messages_unclassified ON messages (mailbox_id) WHERE section IS NULL;
CREATE INDEX messages_traits_unknown ON messages (mailbox_id) WHERE traits IS NULL;

CREATE TABLE senders (
    account_id    TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    address       TEXT NOT NULL,
    policy        TEXT CHECK (policy IN ('people', 'notifications', 'newsletters')),
    priority      INTEGER NOT NULL DEFAULT 0 CHECK (priority IN (0, 1)),
    written_to_at INTEGER,
    PRIMARY KEY (account_id, address)
) STRICT, WITHOUT ROWID;

-- Who each account has written to already, from the mail it has: the To and
-- Cc of messages from one of its own addresses.
INSERT INTO senders (account_id, address, written_to_at)
SELECT b.account_id, lower(json_extract(p.value, '$.address')), MIN(m.received_at)
FROM messages m
JOIN mailboxes b ON b.id = m.mailbox_id
JOIN json_each(m.to_json) p
WHERE json_extract(p.value, '$.address') IS NOT NULL
  AND lower(m.from_address) IN (
      SELECT lower(email) FROM identities WHERE account_id = b.account_id
      UNION SELECT lower(email) FROM accounts WHERE id = b.account_id)
GROUP BY b.account_id, lower(json_extract(p.value, '$.address'))
ON CONFLICT (account_id, address) DO NOTHING;

INSERT INTO senders (account_id, address, written_to_at)
SELECT b.account_id, lower(json_extract(p.value, '$.address')), MIN(m.received_at)
FROM messages m
JOIN mailboxes b ON b.id = m.mailbox_id
JOIN json_each(m.cc_json) p
WHERE json_extract(p.value, '$.address') IS NOT NULL
  AND lower(m.from_address) IN (
      SELECT lower(email) FROM identities WHERE account_id = b.account_id
      UNION SELECT lower(email) FROM accounts WHERE id = b.account_id)
GROUP BY b.account_id, lower(json_extract(p.value, '$.address'))
ON CONFLICT (account_id, address) DO UPDATE SET written_to_at = MIN(written_to_at, excluded.written_to_at);
