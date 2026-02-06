-- Add missing columns for existing installations
ALTER TABLE plans ADD COLUMN is_trial INTEGER DEFAULT 0;
ALTER TABLE subscriptions ADD COLUMN is_trial INTEGER DEFAULT 0;
ALTER TABLE nodes ADD COLUMN reality_sni TEXT DEFAULT 'www.google.com';
