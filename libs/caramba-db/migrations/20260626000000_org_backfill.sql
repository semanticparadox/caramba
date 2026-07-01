-- Default-organization backfill for Caramba Connect (P6).
--
-- The organizations / organization_members tables and subscriptions.organization_id
-- shipped in the init migration but have stayed empty/NULL: nothing seeds a default
-- org and no code populates membership. This migration adopts a single default org
-- (tenant #1) and backfills the dormant tables so the org seam is populated, without
-- changing any behavior for the ~20 live users.
--
-- v1 is single-tenant by design (memory: full multi-tenant scoping is NOT required).
-- exarobot is tenant #1 as an internal org record, not a user-facing brand string.
-- No per-org scoping is added because P4 enforcement counts nodes/users
-- per-instance/global, not per-org, so the populated columns stay read-but-unfiltered.
--
-- Fully additive + idempotent + reversibly-neutral:
--   - seeds the default org by slug via ON CONFLICT (slug) DO NOTHING (reuses any
--     pre-existing 'exarobot' org instead of duplicating it),
--   - resolves the org id BY SLUG (never hardcodes id = 1),
--   - inserts one membership row per existing user via ON CONFLICT (organization_id,
--     user_id) DO NOTHING (role left at the table default 'member'),
--   - sets subscriptions.organization_id to the default org ONLY where it is NULL.
-- No ALTER, no NOT NULL, no destructive statement. Re-running against the live DB is
-- safe and changes nothing already present.

DO $$
DECLARE
    default_org_id BIGINT;
BEGIN
    -- Seed the default org (tenant #1). ON CONFLICT (slug) keeps re-runs and any
    -- pre-existing 'exarobot' org safe; balance/created_at fall back to their
    -- column defaults.
    INSERT INTO organizations (name, slug)
    VALUES ('exarobot', 'exarobot')
    ON CONFLICT (slug) DO NOTHING;

    -- Resolve the id by slug so we never assume id = 1.
    SELECT id INTO default_org_id FROM organizations WHERE slug = 'exarobot';

    -- Defensive guard: after the seed above the default org must exist. If it
    -- does not (unexpected schema, failed seed), abort loudly instead of running
    -- the backfills against a NULL id, which would silently insert nothing and
    -- leave the org seam half-populated. The whole DO block is one transaction,
    -- so RAISE rolls back any partial work.
    IF default_org_id IS NULL THEN
        RAISE EXCEPTION 'org_backfill: default organization (slug=exarobot) not found after seed';
    END IF;

    -- Backfill one membership row per existing user. role is omitted so it takes
    -- the table default ('member'); the PK (organization_id, user_id) makes this
    -- a no-op for users already in the org.
    INSERT INTO organization_members (organization_id, user_id)
    SELECT default_org_id, u.id
    FROM users u
    ON CONFLICT (organization_id, user_id) DO NOTHING;

    -- Adopt orphan subscriptions into the default org. Guarded to touch only NULL
    -- rows, so existing assignments are never overwritten.
    UPDATE subscriptions
    SET organization_id = default_org_id
    WHERE organization_id IS NULL;
END $$;
