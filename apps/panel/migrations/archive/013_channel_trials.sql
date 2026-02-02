-- Migration 013: Channel Trial System
-- Add tracking for channel membership trials

-- Add channel trial fields to users table
ALTER TABLE users ADD COLUMN channel_member_verified INTEGER DEFAULT 0;
ALTER TABLE users ADD COLUMN channel_verified_at TIMESTAMP;
ALTER TABLE users ADD COLUMN trial_source TEXT DEFAULT 'default';  -- 'default' or 'channel'

-- Create index for fast channel member queries
CREATE INDEX IF NOT EXISTS idx_users_channel_verified ON users(channel_member_verified);
CREATE INDEX IF NOT EXISTS idx_users_trial_source ON users(trial_source);
