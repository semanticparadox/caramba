//! JWT-защищённые эндпоинты поддержки standalone-приложения: уведомления и
//! тикеты. Дополняют `app_account.rs`/`app_billing.rs` разделом, который рисует
//! Flutter-клиент (inbox уведомлений + переписка с поддержкой).
//!
//! Все хендлеры идут за `app_auth::require_app_jwt` — `AuthUser` берётся из
//! extensions. Хранилище НЕ реализуется здесь: вся работа делегируется
//! `state.notifications_svc` и `state.tickets_svc`. Проверки владения тикетом
//! выполняют сами сервисы (`get_ticket`/`add_user_message` сверяют user_id),
//! здесь мы лишь маппим результат в стабильные DTO для клиента.
//!
//! Стиль повторяет `app_account.rs`: локальные DTO с `Serialize`, сервисы из
//! `AppState`, JSON-формы согласованы с Flutter-моделями.

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

// ============================================================
// NOTIFICATIONS
// ============================================================

#[derive(Serialize)]
struct AppNotification {
    id: i64,
    title: String,
    body: String,
    /// Категория уведомления (support_ticket, billing, ...). Клиент решает,
    /// какую иконку показать. Маппится из `UserNotification.category`.
    kind: String,
    created_at: String,
    /// `true`, если статус не "unread" (read/archived считаются прочитанными).
    read: bool,
    /// Сырой `payload_json` уведомления (`{"ticket_id":..,"url":..}` у
    /// support_ticket). Отдаём как есть: клиент решает, куда вести тап.
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
    /// Тикет, к которому относится уведомление, вынутый из payload — чтобы
    /// клиенту не разбирать JSON ради навигации «уведомление -> тикет».
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket_id: Option<i64>,
}

/// Достаёт `ticket_id` из payload уведомления. Основной источник — числовое
/// поле `ticket_id`; запасной — `url` вида `/support/{id}`, которым старые
/// записи могли обходиться без явного id.
fn ticket_id_from_payload(payload: Option<&serde_json::Value>) -> Option<i64> {
    let p = payload?;
    if let Some(id) = p.get("ticket_id").and_then(|v| v.as_i64()) {
        return Some(id);
    }
    // Строковый id тоже принимаем: часть уведомлений писалась через json!
    // с уже отформатированными значениями.
    if let Some(id) = p
        .get("ticket_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<i64>().ok())
    {
        return Some(id);
    }
    p.get("url")
        .and_then(|v| v.as_str())
        .and_then(|u| u.strip_prefix("/support/"))
        .and_then(|rest| rest.split(['/', '?']).next())
        .and_then(|s| s.parse::<i64>().ok())
}

#[derive(Serialize)]
struct AppNotificationsResponse {
    notifications: Vec<AppNotification>,
    unread_count: i64,
}

#[derive(Deserialize)]
pub struct NotificationsQuery {
    /// Опциональный фильтр статуса: "unread" | "read" | "archived".
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// GET /api/v2/app/notifications — inbox пользователя + счётчик непрочитанных.
pub async fn list_notifications(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Query(q): Query<NotificationsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);
    let status_filter = q
        .status
        .as_deref()
        .filter(|s| matches!(*s, "unread" | "read" | "archived"));

    let rows = match state
        .notifications_svc
        .list(auth.user_id, status_filter, limit, offset)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(err = %e, "app: list notifications failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let unread_count = state
        .notifications_svc
        .unread_count(auth.user_id)
        .await
        .unwrap_or(0);

    let notifications: Vec<AppNotification> = rows
        .into_iter()
        .map(|n| AppNotification {
            id: n.id,
            title: n.title,
            body: n.body,
            kind: n.category,
            created_at: n.created_at.to_rfc3339(),
            read: n.status != "unread",
            ticket_id: ticket_id_from_payload(n.payload_json.as_ref()),
            payload: n.payload_json,
        })
        .collect();

    Json(AppNotificationsResponse {
        notifications,
        unread_count,
    })
    .into_response()
}

/// POST /api/v2/app/notifications/{id}/read — пометить одно уведомление.
///
/// Сервис сам проверяет владение (UPDATE ... WHERE user_id = $auth), поэтому
/// чужие/несуществующие id просто не меняют строк — это не ошибка.
pub async fn mark_notification_read(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(notification_id): Path<i64>,
) -> impl IntoResponse {
    match state
        .notifications_svc
        .mark_read(auth.user_id, notification_id)
        .await
    {
        Ok(_) => {
            let unread_count = state
                .notifications_svc
                .unread_count(auth.user_id)
                .await
                .unwrap_or(0);
            Json(serde_json::json!({ "ok": true, "unread_count": unread_count })).into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, "app: mark notification read failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// POST /api/v2/app/notifications/read-all — пометить все как прочитанные.
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    match state.notifications_svc.mark_all_read(auth.user_id).await {
        Ok(updated) => Json(serde_json::json!({
            "ok": true,
            "updated": updated,
            "unread_count": 0
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: mark all notifications read failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ============================================================
// TICKETS
// ============================================================

/// Допустимые категории тикета (см. миграцию notifications_and_tickets и
/// admin-фильтры бота). Неизвестные значения от клиента откатываются в
/// "general", чтобы в строку не попадала произвольная метка.
const ALLOWED_TICKET_CATEGORIES: &[&str] = &[
    "general",
    "billing",
    "connection",
    "device",
    "feature_request",
    "technical",
    "other",
];

/// Кап тела сообщения тикета в символах (зеркалит кап темы в 200 символов).
const MAX_MESSAGE_CHARS: usize = 5000;

#[derive(Serialize)]
struct AppTicketSummary {
    id: i64,
    subject: String,
    /// Категория из `ALLOWED_TICKET_CATEGORIES` — клиент подписывает её в
    /// списке и в форме нового тикета.
    category: String,
    status: String,
    created_at: String,
    updated_at: String,
    /// Первые 120 символов последнего сообщения — превью в карточке списка.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_message_preview: Option<String>,
    /// Число непрочитанных ответов поддержки/системы: сообщения новее и
    /// последнего открытия переписки владельцем (`tickets.user_last_read_at`),
    /// и его последнего сообщения. Считается в `list_user_tickets`.
    unread_for_user: i64,
    /// `unread_for_user > 0` — булев флаг для бейджа «новое» и для счётчика
    /// тикетов с непрочитанным ответом в профиле.
    has_unread: bool,
}

/// GET /api/v2/app/tickets — список тикетов пользователя.
pub async fn list_tickets(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let rows = match state.tickets_svc.list_user_tickets(auth.user_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(err = %e, "app: list tickets failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let tickets: Vec<AppTicketSummary> = rows
        .into_iter()
        .map(|t| AppTicketSummary {
            id: t.id,
            subject: t.subject,
            category: t.category,
            status: t.status,
            created_at: t.created_at.to_rfc3339(),
            updated_at: t.updated_at.to_rfc3339(),
            last_message_preview: t.last_message_preview,
            unread_for_user: t.unread_for_user,
            has_unread: t.unread_for_user > 0,
        })
        .collect();

    Json(tickets).into_response()
}

#[derive(Deserialize)]
pub struct CreateTicketRequest {
    pub subject: String,
    pub message: String,
    /// Необязательная категория; по умолчанию "general".
    pub category: Option<String>,
}

#[derive(Serialize)]
struct AppTicketCreated {
    id: i64,
    subject: String,
    status: String,
    created_at: String,
    updated_at: String,
}

/// POST /api/v2/app/tickets — создать тикет с первым сообщением.
pub async fn create_ticket(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Json(payload): Json<CreateTicketRequest>,
) -> impl IntoResponse {
    let subject = payload.subject.trim();
    let message = payload.message.trim();

    if subject.is_empty() {
        return (StatusCode::BAD_REQUEST, "Subject is required").into_response();
    }
    if message.is_empty() {
        return (StatusCode::BAD_REQUEST, "Message is required").into_response();
    }

    let subject = subject.chars().take(200).collect::<String>();
    // Ограничиваем тело сообщения, чтобы в БД не уезжал неограниченный текст
    // (зеркалит кап темы). 5000 символов с запасом покрывает обычный запрос.
    let message = message.chars().take(MAX_MESSAGE_CHARS).collect::<String>();
    // Валидируем категорию против известного набора (используется admin-фильтрами
    // и маршрутизацией уведомлений). Неизвестное значение откатываем в "general",
    // чтобы клиент не записал в строку произвольную метку.
    let category = payload
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| ALLOWED_TICKET_CATEGORIES.contains(s))
        .unwrap_or("general");

    match state
        .tickets_svc
        .create_ticket(auth.user_id, category, &subject, &message, None, None)
        .await
    {
        Ok(t) => Json(AppTicketCreated {
            id: t.id,
            subject: t.subject,
            status: t.status,
            created_at: t.created_at.to_rfc3339(),
            updated_at: t.updated_at.to_rfc3339(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: create ticket failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Serialize)]
struct AppTicketMessage {
    /// "user" | "support" — маппим sender_role ('admin'/'system' -> support).
    author: String,
    body: String,
    created_at: String,
}

#[derive(Serialize)]
struct AppTicketDetail {
    id: i64,
    subject: String,
    category: String,
    status: String,
    created_at: String,
    updated_at: String,
    messages: Vec<AppTicketMessage>,
}

/// GET /api/v2/app/tickets/{id} — тикет с перепиской. Владение проверяет
/// сервис (`get_ticket` с is_admin=false и Some(user_id)); он же отмечает
/// переписку прочитанной владельцем, поэтому после этого вызова список
/// тикетов отдаёт `has_unread = false` для этого тикета.
pub async fn get_ticket(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(ticket_id): Path<i64>,
) -> impl IntoResponse {
    let (ticket, messages) = match state
        .tickets_svc
        .get_ticket(ticket_id, false, Some(auth.user_id))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            // Сервис возвращает ошибку и для "не найден", и для "доступ запрещён".
            // Отдаём 404, чтобы не раскрывать существование чужих тикетов.
            tracing::debug!(err = %e, ticket_id, "app: get ticket denied/not found");
            return (StatusCode::NOT_FOUND, "Ticket not found").into_response();
        }
    };

    let messages: Vec<AppTicketMessage> = messages
        .into_iter()
        .map(|m| AppTicketMessage {
            author: if m.sender_role == "user" {
                "user".to_string()
            } else {
                "support".to_string()
            },
            body: m.body,
            created_at: m.created_at.to_rfc3339(),
        })
        .collect();

    Json(AppTicketDetail {
        id: ticket.id,
        subject: ticket.subject,
        category: ticket.category,
        status: ticket.status,
        created_at: ticket.created_at.to_rfc3339(),
        updated_at: ticket.updated_at.to_rfc3339(),
        messages,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct ReplyTicketRequest {
    pub message: String,
}

/// POST /api/v2/app/tickets/{id}/reply — добавить сообщение пользователя.
/// Владение и статус тикета проверяет `add_user_message`.
pub async fn reply_ticket(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(ticket_id): Path<i64>,
    Json(payload): Json<ReplyTicketRequest>,
) -> impl IntoResponse {
    let message = payload.message.trim();
    if message.is_empty() {
        return (StatusCode::BAD_REQUEST, "Message is required").into_response();
    }
    let message = message.chars().take(MAX_MESSAGE_CHARS).collect::<String>();

    match state
        .tickets_svc
        .add_user_message(ticket_id, auth.user_id, &message, Vec::new())
        .await
    {
        Ok(m) => Json(AppTicketMessage {
            author: "user".to_string(),
            body: m.body,
            created_at: m.created_at.to_rfc3339(),
        })
        .into_response(),
        Err(e) => {
            use crate::services::tickets_service::TicketError;
            tracing::debug!(err = %e, ticket_id, "app: reply ticket rejected");
            match e {
                // Чужой/несуществующий тикет прячем за единым 404, чтобы не
                // раскрывать существование чужих тикетов (как в get_ticket).
                TicketError::NotFound | TicketError::Forbidden => {
                    (StatusCode::NOT_FOUND, "Ticket not found").into_response()
                }
                TicketError::Closed => {
                    (StatusCode::BAD_REQUEST, "Ticket is closed").into_response()
                }
                TicketError::Internal(_) => {
                    tracing::error!(err = %e, ticket_id, "app: reply ticket internal error");
                    (StatusCode::INTERNAL_SERVER_ERROR, "Cannot reply to ticket").into_response()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Основной путь: числовой ticket_id из payload, как его пишет
    /// tickets_service (add_admin_message / set_status / auto_close_stale).
    #[test]
    fn ticket_id_comes_from_numeric_payload_field() {
        let p = json!({"ticket_id": 42, "url": "/support/42"});
        assert_eq!(ticket_id_from_payload(Some(&p)), Some(42));
    }

    /// Строковый id и голый url тоже разбираются: старые записи и
    /// уведомления, собранные вручную, не должны терять переход к тикету.
    #[test]
    fn ticket_id_falls_back_to_string_and_url() {
        let s = json!({"ticket_id": "7"});
        assert_eq!(ticket_id_from_payload(Some(&s)), Some(7));
        let u = json!({"url": "/support/13?from=bot"});
        assert_eq!(ticket_id_from_payload(Some(&u)), Some(13));
    }

    /// Не-тикетные payload (billing, устройства) и пустой payload дают None,
    /// а не 0: клиент по None не рисует переход.
    #[test]
    fn ticket_id_is_none_for_foreign_or_missing_payload() {
        assert_eq!(ticket_id_from_payload(None), None);
        let billing = json!({"payment_id": 5, "url": "/pay"});
        assert_eq!(ticket_id_from_payload(Some(&billing)), None);
        let junk = json!({"url": "/support/abc"});
        assert_eq!(ticket_id_from_payload(Some(&junk)), None);
    }
}
