-- Per-node Clash API (external_controller) secret.
-- Closes the "Clash API exposed on 0.0.0.0:9090 with no secret" hole (caramba-4cs):
-- the panel emits this value as clash_api.secret in the generated sing-box config
-- and sends it as `Authorization: Bearer <secret>` when polling/closing connections.
-- NULL means "not provisioned yet"; the panel lazily generates one on next config build.
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS clash_api_secret TEXT;
