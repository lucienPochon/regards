-- mail.db migration 32: the people an account writes with, for recipient
-- suggestions in the composer.
--
-- Per account and address (lower case):
-- - `sent`: messages from the account (an address of its own) to them, To or Cc;
-- - `received`: messages from them;
-- - the latest name given with the address, and when it was last seen.
--
-- A trigger keeps it current as messages are stored. It has no conflict
-- clause: the upsert that stores messages would override it, so it updates
-- first, then inserts what is new. The account's own addresses are its
-- identities. Mail stored already is counted below.

CREATE TABLE people (
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    address    TEXT NOT NULL,
    name       TEXT,
    sent       INTEGER NOT NULL DEFAULT 0,
    received   INTEGER NOT NULL DEFAULT 0,
    last_at    INTEGER NOT NULL,
    PRIMARY KEY (account_id, address)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER people_from_mail AFTER INSERT ON messages BEGIN
    -- They wrote to the account.
    UPDATE people SET
        received = received + 1,
        name = COALESCE(NULLIF(trim(new.from_name), ''), name),
        last_at = MAX(last_at, new.received_at)
    WHERE account_id = (SELECT account_id FROM mailboxes WHERE id = new.mailbox_id)
      AND address = lower(trim(new.from_address));
    INSERT INTO people (account_id, address, name, received, last_at)
    SELECT b.account_id, lower(trim(new.from_address)), NULLIF(trim(new.from_name), ''), 1, new.received_at
    FROM mailboxes b
    WHERE b.id = new.mailbox_id AND new.from_address LIKE '%_@_%'
      AND NOT EXISTS (SELECT 1 FROM identities i
                      WHERE i.account_id = b.account_id AND lower(i.email) = lower(trim(new.from_address)))
      AND NOT EXISTS (SELECT 1 FROM people p
                      WHERE p.account_id = b.account_id AND p.address = lower(trim(new.from_address)));

    -- The account wrote to them.
    UPDATE people SET
        sent = sent + 1,
        last_at = MAX(last_at, new.received_at)
    WHERE account_id = (SELECT account_id FROM mailboxes WHERE id = new.mailbox_id)
      AND EXISTS (SELECT 1 FROM identities i JOIN mailboxes b ON b.account_id = i.account_id
                  WHERE b.id = new.mailbox_id AND lower(i.email) = lower(trim(new.from_address)))
      AND address IN (
          SELECT lower(trim(value ->> 'address')) FROM json_each(CASE WHEN json_valid(new.to_json) THEN new.to_json ELSE '[]' END)
          UNION
          SELECT lower(trim(value ->> 'address')) FROM json_each(CASE WHEN json_valid(new.cc_json) THEN new.cc_json ELSE '[]' END));
    INSERT INTO people (account_id, address, name, sent, last_at)
    SELECT b.account_id, r.address, r.name, 1, new.received_at
    FROM mailboxes b,
         (SELECT lower(trim(value ->> 'address')) AS address, MAX(NULLIF(trim(value ->> 'name'), '')) AS name
          FROM (SELECT value FROM json_each(CASE WHEN json_valid(new.to_json) THEN new.to_json ELSE '[]' END)
                UNION ALL
                SELECT value FROM json_each(CASE WHEN json_valid(new.cc_json) THEN new.cc_json ELSE '[]' END))
          GROUP BY 1) r
    WHERE b.id = new.mailbox_id AND r.address LIKE '%_@_%'
      AND EXISTS (SELECT 1 FROM identities i
                  WHERE i.account_id = b.account_id AND lower(i.email) = lower(trim(new.from_address)))
      AND NOT EXISTS (SELECT 1 FROM identities i WHERE i.account_id = b.account_id AND lower(i.email) = r.address)
      AND NOT EXISTS (SELECT 1 FROM people p WHERE p.account_id = b.account_id AND p.address = r.address);
END;

-- What is stored already.
INSERT INTO people (account_id, address, name, sent, received, last_at)
SELECT t.account_id, t.address, MAX(t.name), SUM(t.sent), SUM(t.received), MAX(t.at)
FROM (
    SELECT b.account_id, lower(trim(m.from_address)) AS address, NULLIF(trim(m.from_name), '') AS name,
           0 AS sent, 1 AS received, m.received_at AS at
    FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
    WHERE m.from_address LIKE '%_@_%'
      AND NOT EXISTS (SELECT 1 FROM identities i
                      WHERE i.account_id = b.account_id AND lower(i.email) = lower(trim(m.from_address)))
    UNION ALL
    SELECT b.account_id, lower(trim(r.value ->> 'address')), NULLIF(trim(r.value ->> 'name'), ''), 1, 0, m.received_at
    FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id,
         json_each(CASE WHEN json_valid(m.to_json) THEN m.to_json ELSE '[]' END) r
    WHERE EXISTS (SELECT 1 FROM identities i
                  WHERE i.account_id = b.account_id AND lower(i.email) = lower(trim(m.from_address)))
    UNION ALL
    SELECT b.account_id, lower(trim(r.value ->> 'address')), NULLIF(trim(r.value ->> 'name'), ''), 1, 0, m.received_at
    FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id,
         json_each(CASE WHEN json_valid(m.cc_json) THEN m.cc_json ELSE '[]' END) r
    WHERE EXISTS (SELECT 1 FROM identities i
                  WHERE i.account_id = b.account_id AND lower(i.email) = lower(trim(m.from_address)))
) AS t
WHERE t.address LIKE '%_@_%'
  AND NOT EXISTS (SELECT 1 FROM identities i WHERE i.account_id = t.account_id AND lower(i.email) = t.address)
GROUP BY t.account_id, t.address;
