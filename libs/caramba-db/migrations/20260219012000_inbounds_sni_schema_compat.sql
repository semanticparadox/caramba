-- Backward-compat migration for legacy installs.
-- Ensures inbound/sni related columns and tables exist for current panel features.

-- ==============================
-- INBOUNDS
-- ==============================
CREATE TABLE IF NOT EXISTS inbounds (
    id BIGSERIAL PRIMARY KEY,
    node_id BIGINT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    protocol TEXT NOT NULL,
    listen_port INTEGER NOT NULL DEFAULT 443
);

ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS tag TEXT;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS protocol TEXT;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS listen_port INTEGER DEFAULT 443;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS listen_ip TEXT DEFAULT '::';
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS settings TEXT NOT NULL DEFAULT '{}';
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS stream_settings TEXT NOT NULL DEFAULT '{}';
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS remark TEXT;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS enable BOOLEAN DEFAULT TRUE;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS renew_interval_mins INTEGER DEFAULT 0;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS port_range_start INTEGER DEFAULT 10000;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS port_range_end INTEGER DEFAULT 60000;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS last_rotated_at TIMESTAMPTZ;
ALTER TABLE inbounds ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP;

CREATE INDEX IF NOT EXISTS idx_inbounds_node ON inbounds (node_id);
CREATE INDEX IF NOT EXISTS idx_inbounds_protocol ON inbounds (protocol);

-- Deduplicate legacy rows to safely add unique pair constraint for upsert logic.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_name = 'inbounds' AND column_name = 'id'
    ) THEN
        DELETE FROM inbounds a
        USING inbounds b
        WHERE a.id < b.id
          AND a.node_id = b.node_id
          AND a.listen_port = b.listen_port;
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'inbounds_node_listen_port_uniq'
    ) THEN
        ALTER TABLE inbounds
            ADD CONSTRAINT inbounds_node_listen_port_uniq
            UNIQUE (node_id, listen_port);
    END IF;
END $$;

-- ==============================
-- SNI POOL
-- ==============================
CREATE TABLE IF NOT EXISTS sni_pool (
    id BIGSERIAL PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE
);

ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS tier INTEGER DEFAULT 0;
ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS health_score INTEGER DEFAULT 100;
ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS last_check TIMESTAMPTZ;
ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS is_active BOOLEAN DEFAULT TRUE;
ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS is_premium BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS notes TEXT;
ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS discovered_by_node_id BIGINT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'fk_sni_pool_discovered_by_node'
    ) THEN
        ALTER TABLE sni_pool
            ADD CONSTRAINT fk_sni_pool_discovered_by_node
            FOREIGN KEY (discovered_by_node_id) REFERENCES nodes(id) ON DELETE SET NULL;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_sni_pool_discovered_by_node ON sni_pool(discovered_by_node_id);
CREATE INDEX IF NOT EXISTS idx_sni_pool_health_score ON sni_pool(health_score DESC);

-- Legacy installs might miss these tables entirely.
CREATE TABLE IF NOT EXISTS node_pinned_snis (
    node_id BIGINT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    sni_id BIGINT NOT NULL REFERENCES sni_pool(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (node_id, sni_id)
);

CREATE TABLE IF NOT EXISTS sni_blacklist (
    domain TEXT PRIMARY KEY,
    reason TEXT,
    blocked_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);
