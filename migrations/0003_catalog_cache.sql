CREATE TABLE catalog_products (
    product_id TEXT PRIMARY KEY,
    product JSONB NOT NULL,
    search_text TEXT NOT NULL,
    screened_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX catalog_products_search ON catalog_products USING GIN (to_tsvector('simple', search_text));
