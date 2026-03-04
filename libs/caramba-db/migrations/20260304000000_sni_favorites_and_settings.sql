ALTER TABLE sni_pool ADD COLUMN IF NOT EXISTS is_favorite BOOLEAN NOT NULL DEFAULT FALSE;

-- Add setting for SNI Rotation Interval (default 24h)
INSERT INTO panel_settings (key, value) VALUES ('auto_sni_rotation_interval_hours', '24') ON CONFLICT (key) DO NOTHING;

-- Add setting for Custom Deny Patterns (default includes ovh.net, duckdns.net)
INSERT INTO panel_settings (key, value) VALUES ('sni_scanner_deny_patterns', 'ovh.net,duckdns.net') ON CONFLICT (key) DO NOTHING;

-- Remove existing bad domains based on the new defaults
DELETE FROM sni_pool WHERE domain LIKE '%ovh.net' OR domain LIKE '%duckdns.net';
