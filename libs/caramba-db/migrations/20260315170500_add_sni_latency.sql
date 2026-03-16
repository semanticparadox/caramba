ALTER TABLE sni_pool
ADD COLUMN IF NOT EXISTS latency_ms INTEGER;

CREATE INDEX IF NOT EXISTS idx_sni_pool_latency_ms ON sni_pool(latency_ms);
