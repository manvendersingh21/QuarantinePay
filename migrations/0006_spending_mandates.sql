-- Buyer-signed spending mandates let the agent purchase without per-purchase approval, within
-- signed limits. The vault reference never leaves the server and is never shown to a model.
CREATE TABLE mandates (
    id UUID PRIMARY KEY,
    buyer TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('replay','sandbox')),
    signed JSONB NOT NULL,
    digest TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending_vault','active','revoked')),
    vault_id TEXT,
    setup_order TEXT,
    setup_request_id UUID NOT NULL,
    approval_url TEXT,
    verification_authorization TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);
CREATE INDEX mandates_buyer ON mandates(buyer, mode, created_at DESC);
ALTER TABLE agent_tasks ADD COLUMN mandate_id UUID REFERENCES mandates(id);
ALTER TABLE agent_tasks ADD COLUMN autonomy JSONB;
ALTER TABLE orders ADD COLUMN mandate_id UUID REFERENCES mandates(id);
CREATE INDEX orders_mandate ON orders(mandate_id);
