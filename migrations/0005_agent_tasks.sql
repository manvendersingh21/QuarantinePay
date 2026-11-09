-- Agent purchasing tasks. A task can propose a contract; only buyer approval of the exact
-- digest turns it into a signed intent and an order.
CREATE TABLE agent_tasks (
    id UUID PRIMARY KEY,
    buyer TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('replay','sandbox')),
    request TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planning','negotiating','awaiting_approval','approved','purchasing','no_deal','cancelled','failed')),
    planner JSONB,
    objective JSONB,
    negotiation JSONB,
    proposal JSONB,
    proposal_digest TEXT,
    order_id UUID,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agent_tasks_recent ON agent_tasks(buyer, mode, created_at DESC);
ALTER TABLE events ADD COLUMN task_id UUID;
CREATE INDEX events_task ON events(task_id, id);
ALTER TABLE orders ADD COLUMN analysis JSONB;
ALTER TABLE orders ADD COLUMN risk_override BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE orders ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();
