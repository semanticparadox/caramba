-- Migration 009: Quick Wins Features
-- Auto-Renewal, Traffic Alerts, Free Trial System

-- 1. Auto-Renewal System
ALTER TABLE subscriptions ADD COLUMN auto_renew INTEGER DEFAULT 0;
CREATE INDEX idx_subscriptions_auto_renew ON subscriptions(expires_at, auto_renew, status);

-- 2. Traffic Alert Notifications
ALTER TABLE subscriptions ADD COLUMN alerts_sent TEXT DEFAULT '[]';

-- 3. Free Trial System
ALTER TABLE users ADD COLUMN trial_used INTEGER DEFAULT 0;
ALTER TABLE users ADD COLUMN trial_used_at TIMESTAMP NULL;
ALTER TABLE subscriptions ADD COLUMN is_trial INTEGER DEFAULT 0;
ALTER TABLE plans ADD COLUMN is_trial INTEGER DEFAULT 0;

-- Insert Free Trial Plan
INSERT INTO plans (name, description, is_active, is_trial, created_at)
VALUES ('Free Trial', '24h trial with 10GB traffic', 1, 1, CURRENT_TIMESTAMP);

-- Get the trial plan ID and insert duration
INSERT INTO plan_durations (plan_id, duration_days, traffic_gb, price, is_default)
SELECT id, 1, 10, 0, 1
FROM plans WHERE is_trial = 1;
