-- A real Shopify merchant (development store) as the seller. The merchant order is placed only
-- after PayPal authorized, and its fulfilment record is read back as delivery evidence.
ALTER TABLE orders ADD COLUMN shopify_order TEXT;
ALTER TABLE orders ADD COLUMN shopify_name TEXT;
ALTER TABLE orders ADD COLUMN shopify_checked_at TIMESTAMPTZ;
ALTER TABLE orders ADD COLUMN shopify_error TEXT;
ALTER TABLE orders ADD COLUMN shopify_settlement JSONB;
