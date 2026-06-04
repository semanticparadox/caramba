-- Reusable sing-box config profiles bound to node groups (with per-node override).
--
-- `policy` is a JSON document (stored as TEXT, matching the existing convention
-- for inbounds.settings / nodes.load_stats) that the panel deserializes into the
-- typed `singbox::policy::ConfigPolicy`. New core knobs can be added to the policy
-- without further schema migrations.
CREATE TABLE IF NOT EXISTS config_profiles (
    id          BIGSERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    description TEXT,
    policy      TEXT NOT NULL DEFAULT '{}',
    is_default  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- At most one profile may be the global default.
CREATE UNIQUE INDEX IF NOT EXISTS ux_config_profiles_default
    ON config_profiles (is_default) WHERE is_default;

-- Group binding + deterministic priority (lower = higher precedence) so the
-- many-to-many node_group_members membership resolves to a single profile.
ALTER TABLE node_groups
    ADD COLUMN IF NOT EXISTS config_profile_id BIGINT
        REFERENCES config_profiles(id) ON DELETE SET NULL;
ALTER TABLE node_groups
    ADD COLUMN IF NOT EXISTS config_priority INTEGER NOT NULL DEFAULT 100;

-- Per-node override (highest precedence, beats any group profile).
ALTER TABLE nodes
    ADD COLUMN IF NOT EXISTS config_profile_id BIGINT
        REFERENCES config_profiles(id) ON DELETE SET NULL;
