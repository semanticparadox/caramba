-- License activation cache for Caramba Connect (P4, contract D).
--
-- Single-row table holding the last ed25519-signature-verified activation
-- response from the license server. The panel reads CARAMBA_LICENSE_* from the
-- process env on startup, calls POST {server}/v1/activate, verifies the
-- signature against CARAMBA_LICENSE_PUBKEY, and upserts the verified result
-- here. effective_tier/effective_limits read this cached row.
--
-- OFFLINE GRACE: last_verified_at is the anchor for the 14-day grace window. If
-- the server is unreachable on a later startup/re-verify, the panel keeps
-- serving the cached tier while now < last_verified_at + 14 days, then SOFT
-- degrades to Free LIMITS for new privileged actions only. Existing users,
-- traffic, and active subscriptions are never touched.
--
-- Fully additive + idempotent: CREATE TABLE IF NOT EXISTS, no ALTER of any live
-- table. Re-running against the live ~20-user DB is safe and changes nothing.
-- The single-row invariant is enforced by id smallint PRIMARY KEY CHECK (id = 1)
-- with upserts via ON CONFLICT (id) DO UPDATE.

CREATE TABLE IF NOT EXISTS license_state (
    -- Single-row sentinel: only id = 1 is ever written.
    id              SMALLINT     PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    -- Verified tier: 'free' | 'pro'.
    tier            TEXT         NOT NULL,
    -- Verified LicenseLimits as JSON (max_nodes, max_users, end_user_billing,
    -- branding, upstream_ads, manual_approval).
    limits_json     JSONB        NOT NULL,
    -- License expiry from the signed activation response (RFC3339/UTC).
    expires_at      TIMESTAMPTZ  NOT NULL,
    -- Base64 ed25519 signature over the canonical activation message.
    signature       TEXT         NOT NULL,
    -- When this row was last successfully verified (grace-window anchor).
    last_verified_at TIMESTAMPTZ NOT NULL,
    -- Raw activation response payload, kept for audit/debug.
    raw_payload     JSONB        NOT NULL
);
