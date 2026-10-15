-- Agents: scan interval in hours (simpler for players than a cron expression), what the agent last reported
-- about itself, and accounts the agent created because no connected account matched.

ALTER TABLE agents ADD COLUMN scan_hours INTEGER NOT NULL DEFAULT 6 CHECK (scan_hours BETWEEN 1 AND 168);
ALTER TABLE agents ADD COLUMN paused_locally INTEGER NOT NULL DEFAULT 0;
ALTER TABLE agents ADD COLUMN source_states TEXT NOT NULL DEFAULT '{}';

ALTER TABLE agent_reports ADD COLUMN platform TEXT NOT NULL DEFAULT '';

ALTER TABLE platform_accounts ADD COLUMN agent_id TEXT REFERENCES agents (id) ON DELETE SET NULL;
