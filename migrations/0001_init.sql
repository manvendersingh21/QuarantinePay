CREATE TABLE intents (id UUID PRIMARY KEY, buyer TEXT NOT NULL, token JSONB NOT NULL, used BOOLEAN NOT NULL DEFAULT FALSE);
CREATE TABLE orders (
 id UUID PRIMARY KEY, buyer TEXT NOT NULL, intent JSONB NOT NULL,
 state TEXT NOT NULL, paypal_order TEXT, authorization_id TEXT, approval_url TEXT,
 deadline TIMESTAMPTZ, operation TEXT, request_id UUID,
 artifact JSONB, verification JSONB, provider_result JSONB,
 updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE events (id BIGSERIAL PRIMARY KEY, order_id UUID, buyer TEXT NOT NULL, kind TEXT NOT NULL, detail JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX orders_work ON orders(state, updated_at);
CREATE INDEX events_buyer ON events(buyer, id);
CREATE TABLE webhook_receipts (id TEXT PRIMARY KEY, received_at TIMESTAMPTZ NOT NULL DEFAULT now());
