-- Remove SSH-related columns (we use Agent API now)
ALTER TABLE nodes DROP COLUMN ssh_user;
ALTER TABLE nodes DROP COLUMN ssh_port;
ALTER TABLE nodes DROP COLUMN ssh_password;
ALTER TABLE nodes DROP COLUMN root_password;

-- Also drop ssh_public_key from settings if it exists
-- Note: SQLite doesn't support DROP COLUMN for all cases, so we may need to recreate table
-- For now, we'll leave old columns as-is and add new migration when needed
-- This migration is a placeholder for documentation purposes
