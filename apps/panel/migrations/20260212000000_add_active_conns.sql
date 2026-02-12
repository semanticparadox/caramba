-- Add active_connections to nodes table
ALTER TABLE nodes ADD COLUMN active_connections INTEGER DEFAULT 0;
