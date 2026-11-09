-- Existing orders remain unchanged and retain their original payment mode.
ALTER TABLE orders ADD COLUMN review_decision TEXT CHECK (review_decision IN ('approve', 'reject'));
ALTER TABLE orders ADD COLUMN simulation_outcome TEXT NOT NULL DEFAULT 'approve' CHECK (simulation_outcome IN ('approve', 'decline', 'ambiguous'));
