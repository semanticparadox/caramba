use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: i64,
    pub key: String,
    pub name: String,
    #[serde(rename = "type")]
    pub key_type: String, // 'enrollment'
    pub max_uses: Option<i64>,
    pub current_uses: i64,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<i64>,
}
