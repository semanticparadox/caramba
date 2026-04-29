use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Тикет поддержки.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Ticket {
    pub id: i64,
    pub user_id: i64,
    pub category: String,
    pub subject: String,
    pub status: String,
    pub assignee_tg_id: Option<i64>,
    pub related_payment_id: Option<i64>,
    pub related_subscription_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

/// Сводка тикета (для списков) — включает превью последнего сообщения.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TicketSummary {
    pub id: i64,
    pub user_id: i64,
    pub category: String,
    pub subject: String,
    pub status: String,
    pub assignee_tg_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_message_preview: Option<String>,
    pub unread_for_user: i64,
}

/// Тикет с информацией о пользователе (для admin-списков).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TicketWithUser {
    pub id: i64,
    pub user_id: i64,
    pub username: Option<String>,
    pub tg_id: Option<i64>,
    pub category: String,
    pub subject: String,
    pub status: String,
    pub assignee_tg_id: Option<i64>,
    pub related_payment_id: Option<i64>,
    pub related_subscription_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_message_preview: Option<String>,
}

/// Сообщение в тикете.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TicketMessage {
    pub id: i64,
    pub ticket_id: i64,
    pub sender_role: String,
    pub sender_tg_id: Option<i64>,
    pub body: String,
    #[sqlx(json)]
    pub attachments_json: Option<Value>,
    pub created_at: DateTime<Utc>,
}

/// Вложение тикета.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TicketAttachment {
    pub id: i64,
    pub ticket_id: i64,
    pub message_id: Option<i64>,
    pub filename: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub storage_path: String,
    pub created_at: DateTime<Utc>,
}
