-- Agents (ADR-0012 §4): for what an agent changed, how it stood before,
-- and the undo entry that reverses it (while it lasts, ADR-0007).
ALTER TABLE agent_audit ADD COLUMN before_state TEXT;
ALTER TABLE agent_audit ADD COLUMN undo_id INTEGER;
