use crate::api_client::ApiClient;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// ============================================================================
// Модели ответов от панели (API v2 /bot/*)
// Поля десериализуются с сервера — не все могут использоваться локально.
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct TicketWithUser {
    pub id: i64,
    pub user_tg_id: i64,
    pub user_name: Option<String>,
    pub category: String,
    pub subject: String,
    pub status: String,
    pub assignee_tg_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_message_preview: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct TicketMessage {
    pub id: i64,
    pub sender_role: String, // "user" | "admin"
    pub sender_tg_id: Option<i64>,
    pub body: String,
    pub attachments_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TicketDetail {
    pub ticket: TicketWithUser,
    pub user: TicketUserInfo,
    pub messages: Vec<TicketMessage>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct TicketUserInfo {
    pub tg_id: i64,
    pub username: Option<String>,
    pub full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendMessageRequest {
    pub admin_tg_id: i64,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignRequest {
    pub admin_tg_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeStatusRequest {
    pub admin_tg_id: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BroadcastRequest {
    pub category: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BroadcastResponse {
    pub queued: i64,
}

// ============================================================================
// Кэш админских tg_id с TTL 60 секунд
// ============================================================================

#[derive(Clone)]
struct AdminCache {
    ids: HashSet<i64>,
    fetched_at: Instant,
}

impl AdminCache {
    fn is_valid(&self) -> bool {
        self.fetched_at.elapsed() < Duration::from_secs(60)
    }
}

// ============================================================================
// AdminService
// ============================================================================

#[derive(Clone)]
pub struct AdminService {
    api: ApiClient,
    cache: Arc<RwLock<Option<AdminCache>>>,
}

impl AdminService {
    pub fn new(api: ApiClient) -> Self {
        Self {
            api,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    // -------------------------------------------------------------------------
    // Проверка прав администратора (с кэшем 60 сек).
    // Пробуем сначала GET /admins, при ошибке — GET /settings/admin_notification_tg_ids.
    // -------------------------------------------------------------------------
    pub async fn is_admin(&self, tg_id: i64) -> bool {
        // Читаем кэш
        {
            let cache = self.cache.read().await;
            if let Some(ref c) = *cache {
                if c.is_valid() {
                    return c.ids.contains(&tg_id);
                }
            }
        }

        // Перезагружаем
        let ids = self.fetch_admin_ids().await;
        let result = ids.contains(&tg_id);

        let mut cache = self.cache.write().await;
        *cache = Some(AdminCache {
            ids,
            fetched_at: Instant::now(),
        });

        result
    }

    async fn fetch_admin_ids(&self) -> HashSet<i64> {
        // Вариант 1: GET /api/v2/bot/admins
        #[derive(Deserialize)]
        struct AdminsResp {
            #[serde(default)]
            admins: Vec<serde_json::Value>,
        }

        if let Ok(resp) = self.api.get::<AdminsResp>("/admins").await {
            let ids: HashSet<i64> = resp
                .admins
                .iter()
                .filter_map(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
                .collect();
            if !ids.is_empty() {
                return ids;
            }
        }

        // Вариант 2: GET /api/v2/bot/settings/admin_notification_tg_ids
        if let Ok(raw) = self.api.get::<String>("/settings/admin_notification_tg_ids").await {
            return raw
                .split(',')
                .filter_map(|s| s.trim().parse::<i64>().ok())
                .collect();
        }

        HashSet::new()
    }

    // -------------------------------------------------------------------------
    // Тикеты
    // -------------------------------------------------------------------------

    /// Получить список тикетов с фильтрацией по статусу и пагинацией.
    pub async fn list_tickets(
        &self,
        status: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TicketWithUser>> {
        let mut query = format!("/tickets?limit={}&offset={}", limit, offset);
        if let Some(s) = status {
            query.push_str(&format!("&status={}", s));
        }
        self.api.get(&query).await
    }

    /// Получить детали тикета с сообщениями.
    pub async fn get_ticket(&self, ticket_id: i64) -> Result<TicketDetail> {
        self.api.get(&format!("/tickets/{}", ticket_id)).await
    }

    /// Отправить ответ администратора в тикет.
    pub async fn reply_to_ticket(
        &self,
        ticket_id: i64,
        admin_tg_id: i64,
        body: &str,
    ) -> Result<TicketMessage> {
        self.api
            .post(
                &format!("/tickets/{}/messages", ticket_id),
                &SendMessageRequest {
                    admin_tg_id,
                    body: body.to_string(),
                },
            )
            .await
    }

    /// Назначить тикет на администратора.
    pub async fn assign_ticket(&self, ticket_id: i64, admin_tg_id: i64) -> Result<TicketWithUser> {
        self.api
            .post(
                &format!("/tickets/{}/assign", ticket_id),
                &AssignRequest { admin_tg_id },
            )
            .await
    }

    /// Изменить статус тикета.
    pub async fn change_ticket_status(
        &self,
        ticket_id: i64,
        admin_tg_id: i64,
        status: &str,
    ) -> Result<TicketWithUser> {
        self.api
            .post(
                &format!("/tickets/{}/status", ticket_id),
                &ChangeStatusRequest {
                    admin_tg_id,
                    status: status.to_string(),
                },
            )
            .await
    }

    // -------------------------------------------------------------------------
    // Broadcast
    // -------------------------------------------------------------------------

    pub async fn send_broadcast(&self, req: BroadcastRequest) -> Result<i64> {
        let resp: BroadcastResponse = self.api.post("/notifications/broadcast", &req).await?;
        Ok(resp.queued)
    }
}
