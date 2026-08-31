-- Traffic comes from the plan, and from nothing else.
--
-- Until now three separate mechanisms handed out "welcome" traffic, and only
-- one of them was reachable from the panel:
--
--   * `plans.is_free` + `plans.daily_traffic_mb` — the real model, but neither
--     column was in the plan form, so a free plan could not be created at all
--     and `daily_traffic_topup` (which filters on `daily_traffic_mb > 0`) never
--     matched a row. A throttled free user could never come back.
--   * `onboarding_traffic_mb` — a global setting with no UI, default `0`. It
--     gated the only code path that created the free subscription at signup,
--     so registration created no subscription either.
--   * `signup_bonus_traffic_mb` — the one knob that DID have a UI. It credited
--     bonus traffic to a brand-new user who had no subscription at all, i.e.
--     an allowance bump on top of nothing.
--
-- What remains is two numbers on the plan: `traffic_limit_gb` (the ceiling that
-- throttles) and `daily_traffic_mb` (what comes back each day). Promo-code and
-- referral bonuses are untouched — those legitimately top up whatever plan the
-- user is on, which is precisely what the removed signup grant did not.

-- 1. Normalise the seeded headroom BEFORE the column that floored it is gone.
--    `daily_traffic_topup` used to floor at -onboarding_bonus_bytes; with the
--    column dropped the floor becomes 0, and a row left at a negative
--    used_traffic would read as free traffic nobody granted.
UPDATE subscriptions SET used_traffic = 0 WHERE used_traffic < 0;

ALTER TABLE subscriptions DROP COLUMN IF EXISTS onboarding_bonus_bytes;

-- 2. One free plan, enforced by the database.
--    Every lookup is `WHERE is_free = TRUE AND is_active = TRUE LIMIT 1`
--    (handlers/api/bot.rs, services/store_service.rs). With two such plans the
--    winner is whatever the planner returns first — the free tier would differ
--    between a signup and an expiry fallback, and nothing would say so.
CREATE UNIQUE INDEX IF NOT EXISTS plans_single_active_free_idx
    ON plans ((TRUE)) WHERE is_free AND is_active;

-- 3. Settings that no longer have a reader. `free_plan_enabled` never had one:
--    its single occurrence in the codebase was a bot read-allowlist entry.
DELETE FROM settings
 WHERE key IN ('signup_bonus_traffic_mb', 'onboarding_traffic_mb', 'free_plan_enabled');
