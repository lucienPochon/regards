-- mail.db migration 11: drafts written with formatting. When set, `html` is
-- the (sanitised) formatted version and `body` its plain-text version.

ALTER TABLE drafts ADD COLUMN html TEXT;
