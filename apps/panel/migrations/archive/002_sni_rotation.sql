-- Create SNI Pool table
CREATE TABLE sni_pool (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL UNIQUE,
    tier INTEGER DEFAULT 0, -- 0=primary, 1=backup, 2=emergency
    health_score INTEGER DEFAULT 100,
    last_check TIMESTAMP,
    is_active BOOLEAN DEFAULT 1,
    notes TEXT
);

-- Create SNI Rotation Log table
CREATE TABLE sni_rotation_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL,
    old_sni TEXT NOT NULL,
    new_sni TEXT NOT NULL,
    reason TEXT,
    rotated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (node_id) REFERENCES nodes(id)
);

-- Add reality_sni column to nodes table
-- SQLite doesn't support adding columns with default values in same transaction blocks easily multiple times if not careful
-- but simple ADD COLUMN is fine.
ALTER TABLE nodes ADD COLUMN reality_sni TEXT DEFAULT 'www.google.com';

-- Seed initial SNI pool
INSERT INTO sni_pool (domain, tier, health_score) VALUES
    ('www.google.com', 0, 100),
    ('www.microsoft.com', 0, 100),
    ('www.cloudflare.com', 0, 100),
    ('www.apple.com', 1, 100),
    ('www.amazon.com', 1, 100),
    ('www.github.com', 1, 100),
    ('www.wikipedia.org', 2, 100);
