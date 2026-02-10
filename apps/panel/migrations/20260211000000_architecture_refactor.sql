-- ================================================
-- PHASE 1.8: Architecture Refactor (Node Groups & Generators)
-- ================================================

-- 1. Node Groups (Tiers)
CREATE TABLE node_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT UNIQUE, -- e.g., 'basic', 'premium'
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 2. Link Nodes to Groups
CREATE TABLE node_group_members (
    node_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(node_id, group_id),
    FOREIGN KEY(node_id) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY(group_id) REFERENCES node_groups(id) ON DELETE CASCADE
);

-- 3. Link Plans to Groups (Replacing plan_nodes)
CREATE TABLE plan_groups (
    plan_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(plan_id, group_id),
    FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE,
    FOREIGN KEY(group_id) REFERENCES node_groups(id) ON DELETE CASCADE
);

-- 4. Inbound Templates (Generators)
CREATE TABLE inbound_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL, -- 'vless', 'vmess', etc.
    
    -- Template JSON with placeholders {{uuid}}, {{port}}, {{sni}}
    settings_template TEXT NOT NULL, 
    stream_settings_template TEXT NOT NULL,
    
    -- Target Logic
    target_group_id INTEGER, -- Only deploy to this group
    
    -- Dynamic Options
    port_range_start INTEGER DEFAULT 10000,
    port_range_end INTEGER DEFAULT 60000,
    renew_interval_hours INTEGER DEFAULT 0, -- 0 = static (no rotation)
    
    is_active BOOLEAN DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY(target_group_id) REFERENCES node_groups(id) ON DELETE SET NULL
);

-- 5. Migration Logic (Optional: Preserve existing manual links as "Legacy" group?)
-- Ideally we migrate data, but since we don't have complex production data yet, 
-- we can just create a "Default" group and add all current nodes to it.

INSERT INTO node_groups (name, slug, description) VALUES ('Default', 'default', 'Legacy Default Group');
INSERT INTO node_group_members (node_id, group_id) SELECT id, 1 FROM nodes;
INSERT INTO plan_groups (plan_id, group_id) SELECT id, 1 FROM plans; -- Give all plans access to default group
