-- Persist user's relay country choice so Hiddify/Happ auto-updates
-- get the correct relay configuration without relay_country URL param.
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS relay_country TEXT;
