-- Agents (ADR-0012): what each account allows agents to do. Off by
-- default: agents do not see the account until its owner allows them.
ALTER TABLE accounts ADD COLUMN agent_access TEXT NOT NULL DEFAULT 'off'
    CHECK (agent_access IN ('off', 'read', 'triage', 'send'));
