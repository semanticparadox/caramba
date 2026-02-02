-- Migration 011: Frontend Servers Infrastructure
-- Add tables for managing distributed frontend servers
-- Includes security fields for token hashing and expiration

-- Frontend servers table
CREATE TABLE IF NOT EXISTS frontend_servers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL UNIQUE,
    ip_address TEXT NOT NULL,
    region TEXT NOT NULL,
    auth_token TEXT,  -- Legacy plaintext (for backwards compat during migration)
    auth_token_hash TEXT,  -- Secure bcrypt hash (used for new tokens)
    token_expires_at TIMESTAMP,  -- Token expiration (default 1 year)
    token_rotated_at TIMESTAMP,  -- Last rotation timestamp
    is_active INTEGER DEFAULT 1,
    last_heartbeat TIMESTAMP,
    traffic_monthly INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Frontend server statistics
CREATE TABLE IF NOT EXISTS frontend_server_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    frontend_id INTEGER NOT NULL,
    requests_count INTEGER DEFAULT 0,
    bandwidth_used INTEGER DEFAULT 0,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (frontend_id) REFERENCES frontend_servers(id) ON DELETE CASCADE
);

-- Indexes for fast queries
CREATE INDEX IF NOT EXISTS idx_frontend_domain ON frontend_servers(domain);
CREATE INDEX IF NOT EXISTS idx_frontend_region ON frontend_servers(region);
CREATE INDEX IF NOT EXISTS idx_frontend_active ON frontend_servers(is_active);
CREATE INDEX IF NOT EXISTS idx_frontend_stats_time ON frontend_server_stats(timestamp);
CREATE INDEX IF NOT EXISTS idx_frontend_stats_server ON frontend_server_stats(frontend_id);

-- Security indexes for performance
CREATE INDEX IF NOT EXISTS idx_frontend_token_expiration ON frontend_servers(token_expires_at);
CREATE INDEX IF NOT EXISTS idx_frontend_auth_hash ON frontend_servers(auth_token_hash);
