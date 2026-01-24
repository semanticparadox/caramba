-- Create plan_inbounds join table
CREATE TABLE IF NOT EXISTS plan_inbounds (
    plan_id INTEGER NOT NULL,
    inbound_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plan_id, inbound_id),
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE,
    FOREIGN KEY (inbound_id) REFERENCES inbounds(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plan_inbounds_plan ON plan_inbounds(plan_id);
CREATE INDEX IF NOT EXISTS idx_plan_inbounds_inbound ON plan_inbounds(inbound_id);
