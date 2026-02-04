use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(dead_code)]
pub struct Activity {
    pub id: i64,
    pub category: String,
    pub event: String,
    pub created_at: DateTime<Utc>,
}
