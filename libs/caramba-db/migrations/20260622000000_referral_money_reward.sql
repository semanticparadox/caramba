-- Referral reward model: TRAFFIC -> MONEY.
--
-- Reward model (authoritative):
--   * Referee (invited new user): a percentage DISCOUNT on their FIRST paid
--     purchase only. Applied to the charged amount at checkout, never twice.
--   * Referrer (inviter): internal BALANCE credit = reward_percent of the
--     referee's first payment amount, granted when that first payment is
--     FULFILLED. A given referee can credit the referrer at most once.
--
-- There was never a TRAFFIC referral reward in this schema: all historical
-- referral rewards were already balance (cents) via referral_bonuses
-- (bonus_type 'signup'/'signup_welcome'/'payment'). No traffic column exists,
-- so nothing to stop writing on the traffic side. The old per-top-up
-- 'payment' granting is superseded by the first-purchase model below; existing
-- referral_bonuses rows stay valid and keep counting toward lifetime earnings.

-- ------------------------------------------------------------------
-- 1. Unblock repeat/per-purchase referral accounting.
--
-- init.sql put UNIQUE(user_id, referred_user_id) on referral_bonuses, which
-- made any second bonus row for the same (referrer, referee) pair fail the
-- whole payment transaction. The money model records first-purchase payouts in
-- a dedicated table (below), but we still drop the blanket constraint and keep
-- signup idempotency as a PARTIAL unique index so signup bonuses remain
-- once-only while other bonus types are unconstrained.
-- ------------------------------------------------------------------
DO $$
DECLARE
    con_name TEXT;
BEGIN
    -- Drop any UNIQUE constraint on referral_bonuses covering exactly
    -- (user_id, referred_user_id), regardless of its generated name.
    SELECT c.conname INTO con_name
    FROM pg_constraint c
    WHERE c.conrelid = 'referral_bonuses'::regclass
      AND c.contype = 'u'
      AND (
          SELECT array_agg(att.attname::text ORDER BY att.attname::text)
          FROM unnest(c.conkey) AS k(attnum)
          JOIN pg_attribute att
            ON att.attrelid = c.conrelid AND att.attnum = k.attnum
      ) = ARRAY['referred_user_id', 'user_id']
    LIMIT 1;

    IF con_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE referral_bonuses DROP CONSTRAINT %I', con_name);
    END IF;
END$$;

CREATE UNIQUE INDEX IF NOT EXISTS referral_bonuses_signup_unique
    ON referral_bonuses (user_id, referred_user_id)
    WHERE bonus_type = 'signup';

-- ------------------------------------------------------------------
-- 2. First-purchase referral payout ledger (idempotency + lifetime earnings).
--
-- One row per referee whose FIRST paid purchase has been rewarded. The UNIQUE
-- on referred_user_id guarantees the referrer is credited at most once per
-- referee, independent of provider retries / double webhooks. amount_cents is
-- the referrer's credited balance (minor units); base_amount_cents is the
-- referee payment it was computed from (audit).
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS referral_rewards (
    id                BIGSERIAL PRIMARY KEY,
    referrer_id       BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    referred_user_id  BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    base_amount_cents BIGINT NOT NULL,
    amount_cents      BIGINT NOT NULL,
    reward_percent    INTEGER NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (referred_user_id)
);

CREATE INDEX IF NOT EXISTS referral_rewards_referrer_idx
    ON referral_rewards (referrer_id);

-- ------------------------------------------------------------------
-- 3. Global defaults for the money model.
--
-- reward_percent  : % of a referee payment credited to the referrer.
-- referee_discount: % off the invited user's FIRST paid purchase.
-- Per-user overrides keep living in user_referral_rates.bonus_percent
-- (referrer reward). Defaults match the app contract (20 / 15). Stored as TEXT
-- to match the existing settings(key TEXT, value TEXT) convention.
-- ------------------------------------------------------------------
INSERT INTO settings (key, value)
VALUES ('referral_reward_percent', '20')
ON CONFLICT (key) DO NOTHING;

INSERT INTO settings (key, value)
VALUES ('referral_referee_discount_percent', '15')
ON CONFLICT (key) DO NOTHING;
