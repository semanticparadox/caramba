//! Resolves the effective sing-box [`ConfigPolicy`] for a node.
//!
//! Precedence (highest first):
//!   1. `nodes.config_profile_id` — per-node override
//!   2. group profile, lowest `node_groups.config_priority` first (tie-break id)
//!   3. the global default profile (`config_profiles.is_default = true`)
//!   4. otherwise [`ConfigPolicy::default`] (legacy, no-op)
//!
//! Any database error (e.g. the table not yet migrated) degrades gracefully to
//! the no-op default so config generation never breaks.

use caramba_db::models::node::Node;
use sqlx::PgPool;
use tracing::warn;

use crate::singbox::policy::ConfigPolicy;

/// Resolve the effective policy for `node`.
pub async fn resolve_policy(pool: &PgPool, node: &Node) -> ConfigPolicy {
    if let Some(profile_id) = node.config_profile_id
        && let Some(policy) = policy_by_id(pool, profile_id).await
    {
        return policy;
    }

    if let Some(policy) = policy_for_groups(pool, node.id).await {
        return policy;
    }

    if let Some(policy) = default_policy(pool).await {
        return policy;
    }

    ConfigPolicy::default()
}

fn parse(raw: &str, ctx: &str) -> Option<ConfigPolicy> {
    match serde_json::from_str::<ConfigPolicy>(raw) {
        Ok(policy) => Some(policy),
        Err(e) => {
            warn!("config profile ({ctx}): invalid policy JSON ({e}); ignoring");
            None
        }
    }
}

async fn policy_by_id(pool: &PgPool, id: i64) -> Option<ConfigPolicy> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT policy FROM config_profiles WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    raw.and_then(|r| parse(&r, "node-override"))
}

async fn policy_for_groups(pool: &PgPool, node_id: i64) -> Option<ConfigPolicy> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT cp.policy \
         FROM node_group_members ngm \
         JOIN node_groups ng ON ng.id = ngm.group_id \
         JOIN config_profiles cp ON cp.id = ng.config_profile_id \
         WHERE ngm.node_id = $1 \
         ORDER BY ng.config_priority ASC, ng.id ASC \
         LIMIT 1",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    raw.and_then(|r| parse(&r, "group"))
}

async fn default_policy(pool: &PgPool) -> Option<ConfigPolicy> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT policy FROM config_profiles WHERE is_default = TRUE LIMIT 1")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    raw.and_then(|r| parse(&r, "default"))
}
