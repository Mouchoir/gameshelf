-- When an agent was found silent, so the player is told once per silence (task T123).
ALTER TABLE agents ADD COLUMN silent_since TEXT;
