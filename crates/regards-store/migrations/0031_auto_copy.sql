-- mail.db migration 31: a copy every new draft of an account gets (Settings ›
-- Accounts: always Cc or Bcc an address). Applied by the daemon, so drafts
-- made by the CLI and agents get it too. NULL: none.

ALTER TABLE accounts ADD COLUMN auto_copy_kind TEXT CHECK (auto_copy_kind IN ('cc', 'bcc'));
ALTER TABLE accounts ADD COLUMN auto_copy_address TEXT;
