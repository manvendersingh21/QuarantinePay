-- Real product discovery (Channel3) for physical-product purchases. `discovery` records exactly
-- what was sent to the provider and how every listing was screened and filtered.
ALTER TABLE agent_tasks ADD COLUMN external_discovery BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE agent_tasks ADD COLUMN discovery JSONB;
