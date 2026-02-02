-- Migration 012: Secure Frontend Tokens
-- Add token hashing and expiration mechanism
-- Addresses Critical Security Issue: Plaintext token storage (CVSS 8.5)

-- Step 1: Add new security columns
ALTER TABLE frontend_servers ADD COLUMN auth_token_hash TEXT;
ALTER TABLE frontend_servers ADD COLUMN token_expires_at TIMESTAMP;
ALTER TABLE frontend_servers ADD COLUMN token_rotated_at TIMESTAMP;

-- Step 2: Set default expiration for existing tokens (1 year from creation)
-- This gives time for migration without immediately invalidating tokens
UPDATE frontend_servers 
SET token_expires_at = datetime(created_at, '+1 year')
WHERE token_expires_at IS NULL;

-- Step 3: Mark existing tokens for rotation (they're in plaintext and need to be rehashed)
-- token_rotated_at = NULL means "needs rotation" for monitoring
UPDATE frontend_servers
SET token_rotated_at = created_at
WHERE auth_token_hash IS NULL;

-- Step 4: Create index for performance on expiration checks
CREATE INDEX IF NOT EXISTS idx_frontend_token_expiration 
ON frontend_servers(token_expires_at);

-- Step 5: Create index for token hash lookups (for verification)
CREATE INDEX IF NOT EXISTS idx_frontend_auth_hash 
ON frontend_servers(auth_token_hash);

-- Note: We keep auth_token column for backwards compatibility during migration
-- It will be set to NULL after rotation and eventually removed in future migration
