-- Migration: Add plan_nodes for easier plan-to-node linkage
CREATE TABLE IF NOT EXISTS plan_nodes (
    plan_id INTEGER NOT NULL,
    node_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plan_id, node_id),
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE,
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- Note: We keep plan_inbounds for backwards compatibility and fine-grained control,
-- but automation will favor plan_nodes.

-- Automated migration: Link all existing plans to all existing nodes
INSERT OR IGNORE INTO plan_nodes (plan_id, node_id)
SELECT p.id, n.id FROM plans p, nodes n;

