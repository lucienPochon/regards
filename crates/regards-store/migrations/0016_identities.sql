-- mail.db migration 16: identities (plan §5.9): the names and addresses an
-- account sends as (its own and aliases), each with a signature. Exactly one
-- per account is the default; the account's own address is created as it.

CREATE TABLE identities (
    id          INTEGER PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name        TEXT NOT NULL DEFAULT '',
    email       TEXT NOT NULL,
    -- Plain text, added below what is written (and above a quote).
    signature   TEXT NOT NULL DEFAULT '',
    is_default  INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at  INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX identities_by_address ON identities (account_id, lower(email));
CREATE UNIQUE INDEX identities_one_default ON identities (account_id) WHERE is_default = 1;

INSERT INTO identities (account_id, name, email, is_default, created_at)
SELECT id, display_name, email, 1, created_at FROM accounts;

-- Who a draft is from; the account's default identity when NULL.
ALTER TABLE drafts ADD COLUMN identity_id INTEGER REFERENCES identities (id) ON DELETE SET NULL;
