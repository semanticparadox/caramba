use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Запись в журнале активности. Поля соответствуют схеме таблицы `activity_log`.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(dead_code)]
pub struct Activity {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}
