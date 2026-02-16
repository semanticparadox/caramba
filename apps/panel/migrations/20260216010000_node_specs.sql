-- Add hardware specs to nodes table
ALTER TABLE nodes ADD COLUMN max_ram BIGINT DEFAULT 0;
ALTER TABLE nodes ADD COLUMN cpu_cores INTEGER DEFAULT 0;
ALTER TABLE nodes ADD COLUMN cpu_model TEXT;

-- Node Overhaul & SNI Pinning
ALTER TABLE nodes ADD COLUMN is_relay BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN last_sync_trigger TEXT;
ALTER TABLE nodes ADD COLUMN pending_log_collection BOOLEAN NOT NULL DEFAULT 0;

-- SNI Pool Updates
ALTER TABLE sni_pool ADD COLUMN is_premium BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE sni_pool ADD COLUMN discovered_by_node_id BIGINT REFERENCES nodes(id) ON DELETE SET NULL;

-- Pinned SNIs
CREATE TABLE IF NOT EXISTS node_pinned_snis (
    node_id INTEGER NOT NULL,
    sni_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (node_id, sni_id),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (sni_id) REFERENCES sni_pool(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_node_pinned_snis_node ON node_pinned_snis(node_id);

-- SNI Blacklist
CREATE TABLE IF NOT EXISTS sni_blacklist (
    domain TEXT PRIMARY KEY,
    reason TEXT,
    blocked_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Rotation Log
CREATE TABLE IF NOT EXISTS sni_rotation_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL,
    old_sni TEXT NOT NULL,
    new_sni TEXT NOT NULL,
    reason TEXT,
    rotated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (node_id) REFERENCES nodes(id)
);


