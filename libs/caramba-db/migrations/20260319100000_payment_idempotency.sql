-- Payment idempotency: prevent duplicate payments from webhook retries
-- Clean up any existing duplicates (keep the earliest by id)
DELETE FROM payments p1
USING payments p2
WHERE p1.id > p2.id
  AND p1.method = p2.method
  AND p1.external_id = p2.external_id
  AND p1.external_id IS NOT NULL
  AND p2.external_id IS NOT NULL;

-- Partial unique index (only where external_id is not null)
CREATE UNIQUE INDEX IF NOT EXISTS idx_payments_method_external_id
ON payments (method, external_id) WHERE external_id IS NOT NULL;
