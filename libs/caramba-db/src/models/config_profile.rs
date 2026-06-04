use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// A reusable sing-box configuration profile.
///
/// `policy` holds a JSON document (stored as TEXT) that the panel deserializes
/// into `singbox::policy::ConfigPolicy`. Keeping it as an opaque string here
/// lets `caramba-db` stay free of panel-specific policy types while still being
/// fully version-controlled per row.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ConfigProfile {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    /// JSON-encoded `ConfigPolicy`. Defaults to `"{}"` (no-op policy).
    pub policy: String,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
