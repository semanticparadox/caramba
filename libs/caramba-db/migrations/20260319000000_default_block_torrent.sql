-- Make torrent blocking enabled by default for new nodes
ALTER TABLE nodes ALTER COLUMN config_block_torrent SET DEFAULT TRUE;

-- Enable for existing nodes
UPDATE nodes SET config_block_torrent = TRUE;
