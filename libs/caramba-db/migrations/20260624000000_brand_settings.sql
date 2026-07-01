-- Brand + license-tier settings seed for Caramba Connect (P3).
--
-- SHARED K/V CONTRACT (B): brand is stored as plain settings(key,value) rows.
-- The BOT admin commands WRITE these keys; the public panel branding endpoint
-- (GET /api/v2/app/branding) READS them and overlays the license-tier gate;
-- the Flutter client converges on the same six brand_* fields. This migration
-- only SEEDS empty/false defaults so the keys always exist.
--
-- Fully additive + idempotent: INSERT ... ON CONFLICT (key) DO NOTHING. It does
-- NOT ALTER the settings table (already TEXT/TEXT) and re-running against the
-- live ~20-user DB is safe and changes nothing already present.
--
-- Default state (Free tier, no configured brand) is intentional and does NOT
-- brick existing flows: the branding endpoint returns enabled=false +
-- upstream_ads=true, and the client renders the default Caramba Connect look.

-- Operator brand fields (read by the panel branding endpoint, written by the bot).
-- brand_enabled gates whether the operator has turned branding on; the endpoint
-- additionally requires the license tier to permit branding (Pro) and a non-empty
-- brand_name before enabled=true is returned.
INSERT INTO settings (key, value) VALUES ('brand_enabled', 'false')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value) VALUES ('brand_name', '')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value) VALUES ('brand_logo_url', '')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value) VALUES ('brand_accent_hex', '')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value) VALUES ('brand_support_url', '')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value) VALUES ('brand_bot_url', '')
ON CONFLICT (key) DO NOTHING;

-- License tier seam (C). Default 'free' so an un-activated instance gates to the
-- Free limits/branding=false path. Overridable by hand for now; P4 replaces the
-- SOURCE of the effective tier (activated, ed25519-signature-verified license)
-- while keeping the license::effective_tier signature, so this key remains the
-- documented P3 fallback.
INSERT INTO settings (key, value) VALUES ('license_tier', 'free')
ON CONFLICT (key) DO NOTHING;
