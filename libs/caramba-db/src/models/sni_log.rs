use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SniRotationLog {
    pub id: i64,
    pub node_id: i64,
    pub old_sni: String,
    pub new_sni: String,
    pub reason: Option<String>,
    pub rotated_at: DateTime<Utc>,
    // Optional node name if joined
    pub node_name: Option<String>,
}
