use serde::{Deserialize, Serialize};
use sqlx::FromRow;
// use chrono::{DateTime, Utc}; // Removed


#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: i64,
    pub key: String,
    pub name: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub key_type: String, // 'enrollment'
    pub max_uses: Option<i64>,
    pub current_uses: i64,
    pub is_active: bool,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by: Option<i64>,
}
