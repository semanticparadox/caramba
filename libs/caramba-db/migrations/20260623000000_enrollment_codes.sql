-- Enrollment codes for the standalone app (Caramba Connect).
--
-- Mirrors the proven family_invites shape (code + max_uses + used_count +
-- expires_at) so the same FOR UPDATE + conditional-increment redemption pattern
-- can be reused verbatim. Unlike family_invites this is NOT tied to a family
-- parent: an enrollment code may be org/panel-level (inviter_user_id NULL) or
-- attributed to a specific inviter that feeds the signup-source machinery.
--
-- Fully additive + idempotent (IF NOT EXISTS / ON CONFLICT DO NOTHING) so it is
-- safe against the live ~20-user database and re-runnable.

CREATE TABLE IF NOT EXISTS enrollment_codes (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    -- Inviter/org attribution. NULLABLE: a code may be panel-level with no
    -- specific inviter. ON DELETE SET NULL keeps the code valid if the inviter
    -- user is removed (the code itself is not invalidated by that).
    inviter_user_id BIGINT NULL REFERENCES users(id) ON DELETE SET NULL,
    max_uses INTEGER NOT NULL DEFAULT 1,
    used_count INTEGER NOT NULL DEFAULT 0,
    -- NULLABLE expiry: NULL = never expires (validity predicate must guard with
    -- `expires_at IS NULL OR expires_at > now`). This DIVERGES from
    -- family_invites.expires_at which is NOT NULL — do not copy that predicate.
    expires_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_enrollment_codes_code
    ON enrollment_codes(code);

-- One-time onboarding traffic grant (MB) for fresh unpaid accounts that enroll
-- with a valid code. '0' = off (no grant). Stored as TEXT like every settings
-- value; parsed by the caller via state.settings.get_or_default(...).parse().
INSERT INTO settings (key, value)
VALUES ('onboarding_traffic_mb', '0')
ON CONFLICT (key) DO NOTHING;

-- One-time onboarding bonus, in BYTES, recorded per subscription.
--
-- The grant must INCREASE the usable quota, not decrease used_traffic. Every
-- enforcement comparison in the system is `used_traffic >= traffic_limit_gb*1GB`
-- (subscription_service.rs expire/throttle queries, the subscription header in
-- subscription.rs/client.rs). A fresh account has used_traffic = 0, so reducing
-- it via GREATEST(0, 0 - X) is a no-op and grants nothing.
--
-- Instead we seed used_traffic = -onboarding_bonus_bytes (negative headroom):
-- the comparison then yields `-bonus >= limit` = false, giving exactly `bonus`
-- extra usable bytes before the cap, and node usage accounting
-- (used_traffic = used_traffic + bytes) eats into that headroom first.
--
-- The column itself records the granted amount so the daily-traffic top-up
-- (monitoring.rs) can floor at GREATEST(-onboarding_bonus_bytes, ...) instead of
-- GREATEST(0, ...) and therefore never erase the one-time onboarding headroom.
-- For every pre-existing subscription it is 0, so the floor stays at 0 exactly
-- as before (no behaviour change for the live ~20 users).
ALTER TABLE subscriptions
    ADD COLUMN IF NOT EXISTS onboarding_bonus_bytes BIGINT NOT NULL DEFAULT 0;
