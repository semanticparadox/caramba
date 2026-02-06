-- Fix missing columns for existing installations
-- This is necessary because the consolidated init migration won't run on existing DBs.

ALTER TABLE plans ADD COLUMN is_trial INTEGER DEFAULT 0;
ALTER TABLE subscriptions ADD COLUMN is_trial INTEGER DEFAULT 0;
ALTER TABLE nodes ADD COLUMN reality_sni TEXT DEFAULT 'www.google.com';
