use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Уведомление пользователя (строка в user_notifications).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserNotification {
    pub id: i64,
    pub user_id: i64,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    #[sqlx(json)]
    pub payload_json: Option<Value>,
    pub status: String,
    pub delivered_via_bot: bool,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

/// Запись настроек уведомлений по каналу для одной категории.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NotificationChannelPref {
    pub user_id: i64,
    pub category: String,
    pub channel: String,
    pub enabled: bool,
}
