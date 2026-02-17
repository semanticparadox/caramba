use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SniPoolItem {
    pub id: i64,
    pub domain: String,
    pub tier: i32,
    pub health_score: i32,
    pub last_check: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
    pub notes: Option<String>,
    pub discovered_by_node_id: Option<i64>,
    #[sqlx(default)]
    pub is_premium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SniBlacklistItem {
    pub domain: String,
    pub reason: Option<String>,
    pub blocked_at: Option<chrono::NaiveDateTime>,
}
