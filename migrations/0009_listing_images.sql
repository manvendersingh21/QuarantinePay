-- Listing photos fetched server-side during product discovery, so the buyer's browser never
-- contacts a third party. Content-addressed: the SHA-256 of the bytes is the only reference the
-- UI and the contract use. `source_url` is kept for audit.
CREATE TABLE listing_images (
    sha256 TEXT PRIMARY KEY CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    content_type TEXT NOT NULL CHECK (content_type IN ('image/jpeg', 'image/png', 'image/webp', 'image/gif')),
    bytes BYTEA NOT NULL CHECK (octet_length(bytes) BETWEEN 1 AND 2097152),
    source_url TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX listing_images_source_url ON listing_images (source_url);
