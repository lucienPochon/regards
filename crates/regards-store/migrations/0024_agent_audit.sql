-- Agents (ADR-0012 §4): every request an agent makes, allowed or not, for
-- the person to see what agents did (Settings › Agents). Kept 90 days.
CREATE TABLE agent_audit (
    id          INTEGER PRIMARY KEY,
    -- Unix time in milliseconds.
    at          INTEGER NOT NULL,
    -- The agent's connection, and what it came through (cli, mcp).
    session     TEXT NOT NULL,
    client      TEXT NOT NULL,
    method      TEXT NOT NULL,
    -- Its parameters as JSON, secrets taken out, cut short when long.
    args        TEXT NOT NULL,
    -- The account it acted on, when the request named one, and what that
    -- account allowed agents then. Kept when the account is removed.
    account_id  TEXT,
    access      TEXT,
    decision    TEXT NOT NULL CHECK (decision IN ('allowed', 'denied', 'held', 'approved', 'rejected')),
    -- NULL when it succeeded; otherwise the error.
    error_code  INTEGER,
    error       TEXT,
    duration_ms INTEGER NOT NULL
);
CREATE INDEX agent_audit_by_time ON agent_audit (at);
CREATE INDEX agent_audit_by_account ON agent_audit (account_id, id);
