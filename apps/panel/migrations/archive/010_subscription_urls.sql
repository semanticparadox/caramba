-- Migration: Subscription URLs
-- Add UUID for subscription links

-- Add subscription_uuid column
ALTER TABLE subscriptions ADD COLUMN subscription_uuid TEXT UNIQUE;

-- Generate UUIDs for existing subscriptions (SQLite hex format)
UPDATE subscriptions 
SET subscription_uuid = (
    lower(
        hex(randomblob(4)) || '-' || 
        hex(randomblob(2)) || '-' || 
        hex(randomblob(2)) || '-' || 
        hex(randomblob(2)) || '-' || 
        hex(randomblob(6))
    )
)
WHERE subscription_uuid IS NULL;

-- Create index for fast lookups
CREATE INDEX idx_subscriptions_uuid ON subscriptions(subscription_uuid);

-- Add last_sub_access timestamp (track usage)
ALTER TABLE subscriptions ADD COLUMN last_sub_access TIMESTAMP NULL;
