-- Device lease model for stable device-limit accounting.
CREATE TABLE IF NOT EXISTS subscription_device_leases (
    id BIGSERIAL PRIMARY KEY,
    subscription_id BIGINT NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    device_fingerprint TEXT NOT NULL,
    device_name TEXT,
    user_agent TEXT,
    last_ip TEXT NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_node_id BIGINT REFERENCES nodes(id) ON DELETE SET NULL,
    UNIQUE(subscription_id, device_fingerprint)
);

CREATE INDEX IF NOT EXISTS idx_subscription_device_leases_sub_id
    ON subscription_device_leases(subscription_id);
CREATE INDEX IF NOT EXISTS idx_subscription_device_leases_last_seen
    ON subscription_device_leases(last_seen_at DESC);
CREATE INDEX IF NOT EXISTS idx_subscription_device_leases_last_ip
    ON subscription_device_leases(last_ip);

-- Backfill lease rows from the existing IP tracking history.
INSERT INTO subscription_device_leases (
    subscription_id,
    device_fingerprint,
    device_name,
    user_agent,
    last_ip,
    first_seen_at,
    last_seen_at
)
SELECT
    sip.subscription_id,
    md5(
        lower(COALESCE(NULLIF(sip.user_agent, ''), 'unknown'))
        || '|'
        || COALESCE(NULLIF(sip.client_ip, ''), '0.0.0.0')
    ),
    COALESCE(NULLIF(sip.user_agent, ''), 'Unknown Device') AS device_name,
    NULLIF(sip.user_agent, '') AS user_agent,
    sip.client_ip,
    sip.last_seen_at,
    sip.last_seen_at
FROM subscription_ip_tracking sip
WHERE COALESCE(NULLIF(sip.client_ip, ''), '0.0.0.0') <> '0.0.0.0'
ON CONFLICT (subscription_id, device_fingerprint) DO UPDATE SET
    last_seen_at = GREATEST(subscription_device_leases.last_seen_at, EXCLUDED.last_seen_at),
    last_ip = EXCLUDED.last_ip,
    user_agent = COALESCE(EXCLUDED.user_agent, subscription_device_leases.user_agent),
    device_name = COALESCE(EXCLUDED.device_name, subscription_device_leases.device_name);
