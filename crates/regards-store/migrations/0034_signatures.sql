-- mail.db migration 34: signatures (ADR-0021). A list of their own, for
-- every account, each named for the person; each identity has one as its
-- default, or none. What each identity had becomes one of the list.

CREATE TABLE signatures (
    id         INTEGER PRIMARY KEY,
    -- For the person only: "Formal", "Short".
    name       TEXT NOT NULL,
    -- Plain text, added below what is written (and above a quote).
    body       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

ALTER TABLE identities ADD COLUMN signature_id INTEGER REFERENCES signatures (id) ON DELETE SET NULL;

-- Named after the first address that has it; the same text becomes one.
INSERT INTO signatures (name, body, created_at, updated_at)
SELECT (SELECT j.email FROM identities j WHERE j.signature = i.signature ORDER BY j.id LIMIT 1),
       i.signature, min(i.created_at), min(i.created_at)
FROM identities i
WHERE trim(i.signature) <> ''
GROUP BY i.signature
ORDER BY min(i.id);

UPDATE identities SET signature_id = (SELECT s.id FROM signatures s WHERE s.body = identities.signature)
WHERE trim(signature) <> '';

ALTER TABLE identities DROP COLUMN signature;
