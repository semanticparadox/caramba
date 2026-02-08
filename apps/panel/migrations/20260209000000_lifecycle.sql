-- Add status column to frontend_servers
ALTER TABLE frontend_servers ADD COLUMN status TEXT DEFAULT 'offline';

-- Ensure we index it for faster lookups
CREATE INDEX IF NOT EXISTS idx_frontend_status ON frontend_servers(status);
