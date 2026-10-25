-- mail.db migration 39: signatures with formatting and pictures (ADR-0021).
-- `html` is the formatted version, sanitised, its pictures embedded as
-- data: URIs; `body` stays its plain-text version, for plain-text mail and
-- to recognise a signature in a draft. NULL: a plain-text signature.
ALTER TABLE signatures ADD COLUMN html TEXT;
