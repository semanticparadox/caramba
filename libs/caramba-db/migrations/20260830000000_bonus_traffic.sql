-- Bonus traffic: a one-off traffic allowance granted on top of whatever the
-- user's plan gives.
--
-- Storage model (ledger + denormalized balance), and why both:
--
--   * `traffic_bonuses` is the LEDGER. One row per grant, with the natural key
--     (user_id, source, reference) UNIQUE. That key IS the idempotency
--     mechanism: a retried enrollment, a duplicate promo redemption, a
--     re-delivered payment webhook — all collapse to `ON CONFLICT DO NOTHING`,
--     so no source can ever double-grant. It also gives support the per-grant
--     history ("where did this user's 5 GB come from?") that a bare counter
--     column cannot.
--
--   * `users.bonus_traffic_mb` is the DENORMALIZED total, written in the SAME
--     transaction as the ledger insert and only when that insert actually
--     produced a row. It exists because the quota gates are hot paths: the
--     heartbeat ingestion sweep and the expiry/throttle sweeps evaluate the
--     limit for every touched subscription, and making each of them SUM a
--     ledger would put an aggregate on the critical path of every node
--     heartbeat. The column is a cache of `SUM(traffic_bonuses.amount_mb)` and
--     can always be rebuilt from the ledger (see the backfill at the bottom).
--
-- The balance is an ALLOWANCE BUMP, not a consumable: enforcement compares
-- `used_traffic >= plan_limit + bonus`, exactly like the existing
-- `onboarding_bonus_bytes` headroom, rather than decrementing on use. Megabytes
-- (not bytes) are the unit because every operator-facing knob in this system
-- (plans.daily_traffic_mb, onboarding_traffic_mb, promo traffic) is already
-- expressed in MB/GB.

-- ------------------------------------------------------------------
-- 1. Denormalized per-user balance.
-- ------------------------------------------------------------------
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS bonus_traffic_mb BIGINT NOT NULL DEFAULT 0;

-- ------------------------------------------------------------------
-- 2. Grant ledger.
--
-- source    : which feature granted it ('signup', 'promo', 'referral_referrer',
--             'referral_referee', 'admin').
-- reference : the thing that makes the grant unique WITHIN that source — the
--             promo code id, the referred user id, and so on. NOT NULL and
--             never empty on purpose: Postgres treats NULLs as distinct in a
--             UNIQUE index, so a nullable reference would silently disable the
--             idempotency guard. Sources that are once-per-user (signup) use a
--             constant literal.
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS traffic_bonuses (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source     TEXT NOT NULL,
    reference  TEXT NOT NULL,
    amount_mb  BIGINT NOT NULL,
    note       TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (user_id, source, reference)
);

CREATE INDEX IF NOT EXISTS traffic_bonuses_user_idx
    ON traffic_bonuses (user_id, created_at DESC);

-- ------------------------------------------------------------------
-- 3. Global defaults for the three bonus sources.
--
-- 0 everywhere = feature off, so this migration changes nobody's allowance
-- until the operator turns something on. Stored as TEXT to match the existing
-- settings(key TEXT, value TEXT) convention.
-- ------------------------------------------------------------------
INSERT INTO settings (key, value)
VALUES ('signup_bonus_traffic_mb', '0')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value)
VALUES ('referral_bonus_traffic_mb_referrer', '0')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value)
VALUES ('referral_bonus_traffic_mb_referee', '0')
ON CONFLICT (key) DO NOTHING;

-- ------------------------------------------------------------------
-- 4. Rebuild the denormalized balance from the ledger.
--
-- A no-op on a fresh install (the ledger is empty); written as an idempotent
-- statement so it doubles as the repair procedure if the cache ever drifts.
-- ------------------------------------------------------------------
UPDATE users u
SET bonus_traffic_mb = COALESCE(b.total, 0)
FROM (
    SELECT user_id, SUM(amount_mb) AS total
    FROM traffic_bonuses
    GROUP BY user_id
) b
WHERE b.user_id = u.id
  AND u.bonus_traffic_mb IS DISTINCT FROM COALESCE(b.total, 0);
