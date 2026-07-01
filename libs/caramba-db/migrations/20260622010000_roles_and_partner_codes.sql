-- Staff roles + partner per-source attribution codes.
--
-- Two net-new concepts land here. Neither existed before:
--   1. A role column on `admins` so a staff member can be a 'moderator'
--      (ticket triage + reply only) instead of full 'admin'. Every existing
--      admin row is a full admin, so the column DEFAULTs to 'admin' and the
--      backfill keeps the ~20 prod installs unchanged.
--   2. A partner flag on `users` plus a `partner_codes` table so a partner
--      can mint several per-source referral aliases (e.g. one per channel),
--      each mapping back to the same partner user via the existing signup
--      attribution mechanism (users.referrer_id). Money is NOT duplicated:
--      partner earnings are derived from referral_rewards / referral_bonuses
--      that the normal referral path already writes.
--
-- All statements are idempotent (IF NOT EXISTS / guarded DO blocks) so the
-- migration is safe to re-run.

-- ------------------------------------------------------------------
-- 1. Admin role. 'admin' = full panel access (existing behaviour),
--    'moderator' = ticket triage/reply only (gated in handlers).
-- ------------------------------------------------------------------
ALTER TABLE admins
    ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'admin';

-- Backfill is implicit via the DEFAULT, but make it explicit for any row that
-- somehow predates the default (defensive; cheap on ~20 rows).
UPDATE admins SET role = 'admin' WHERE role IS NULL;

CREATE INDEX IF NOT EXISTS idx_admins_role ON admins(role);

-- ------------------------------------------------------------------
-- 2. Partner flag on users. Gates the /app/partner/* endpoints; a user with
--    is_partner = FALSE gets is_partner:false (codes path) or 403 (mutations).
-- ------------------------------------------------------------------
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS is_partner BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_users_is_partner ON users(is_partner) WHERE is_partner = TRUE;

-- ------------------------------------------------------------------
-- 3. Partner per-source codes.
--
-- Each row is a referral alias owned by a partner. `code` is globally UNIQUE so
-- resolve_referrer_id can consult it the same way it consults users.referral_code.
-- `source_label` is the partner's own label for the channel/source. `clicks` is
-- a best-effort counter incremented on a public deep-link hit (cheap UPDATE);
-- signups / conversions / earnings are DERIVED at read time from attributed
-- users + referral payouts, never stored here, so there is one source of truth
-- for money.
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS partner_codes (
    id              BIGSERIAL PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,
    partner_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_label    TEXT NOT NULL DEFAULT '',
    clicks          BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_partner_codes_partner ON partner_codes(partner_user_id);

-- ------------------------------------------------------------------
-- 4. Per-code signup attribution on users.
--
-- users.referrer_id already records WHO referred a user, but a partner can own
-- many codes, so referrer_id alone cannot tell which code a signup came from.
-- This nullable FK records the specific partner_code a user signed up through,
-- letting per-code `signups` and `conversions` be derived by joining users back
-- to partner_codes. It is set ONCE at signup attribution time (alongside
-- referrer_id) and never rewritten. Plain referral signups leave it NULL.
-- ------------------------------------------------------------------
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS signup_partner_code_id BIGINT REFERENCES partner_codes(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_users_signup_partner_code
    ON users(signup_partner_code_id) WHERE signup_partner_code_id IS NOT NULL;
