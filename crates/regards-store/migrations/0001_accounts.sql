-- mail.db migration 1: the change-feed revision and mail accounts.
-- Migrations are forward-only and never edited once merged (ADR-0008).

-- Incremented by every committed write transaction: the change feed's cursor (ADR-0007).
CREATE TABLE revision (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    value INTEGER NOT NULL
) STRICT;

INSERT INTO revision (id, value) VALUES (1, 0);

-- One mail account. Secrets live in the OS credential store; `secret_key`
-- only names the entry (ADR-0008 §7). There is no unencrypted transport.
CREATE TABLE accounts (
    id                TEXT PRIMARY KEY,  -- UUID, stable across devices
    display_name      TEXT NOT NULL,
    email             TEXT NOT NULL,
    auth_kind         TEXT NOT NULL CHECK (auth_kind IN ('password', 'oauth_microsoft', 'oauth_google')),
    username          TEXT NOT NULL,
    secret_key        TEXT NOT NULL,
    imap_host         TEXT NOT NULL,
    imap_port         INTEGER NOT NULL CHECK (imap_port BETWEEN 1 AND 65535),
    imap_security     TEXT NOT NULL CHECK (imap_security IN ('tls', 'starttls')),
    smtp_host         TEXT NOT NULL,
    smtp_port         INTEGER NOT NULL CHECK (smtp_port BETWEEN 1 AND 65535),
    smtp_security     TEXT NOT NULL CHECK (smtp_security IN ('tls', 'starttls')),
    -- What the IMAP server advertised after login, as a JSON array of strings.
    imap_capabilities TEXT NOT NULL DEFAULT '[]',
    created_at        INTEGER NOT NULL,  -- Unix time in milliseconds
    position          INTEGER NOT NULL   -- order in the account list
) STRICT;

CREATE UNIQUE INDEX accounts_email ON accounts (email COLLATE NOCASE);
