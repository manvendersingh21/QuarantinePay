-- Existing data was created in replay mode. Keep it out of live Sandbox operations.
ALTER TABLE orders ADD COLUMN mode TEXT NOT NULL DEFAULT 'replay' CHECK (mode IN ('replay','sandbox'));
ALTER TABLE intents ADD COLUMN mode TEXT NOT NULL DEFAULT 'replay' CHECK (mode IN ('replay','sandbox'));
CREATE INDEX orders_mode_work ON orders(mode,state,updated_at);
