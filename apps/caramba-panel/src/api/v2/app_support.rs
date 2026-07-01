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
    let status_filter = q.status.as_deref().filter(|s| {
        matches!(*s, "unread" | "read" | "archived")
    });

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
            Json(serde_json::json!({ "ok": true, "unread_count": unread_count }))
                .into_response()
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
    status: String,
    updated_at: String,
    /// Число непрочитанных ответов поддержки/системы (сообщения после
    /// последнего сообщения пользователя). Клиент рисует бейдж по этому
    /// счётчику (`unread_for_user > 0`). Считается в `list_user_tickets`.
    unread_for_user: i64,
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
            status: t.status,
            updated_at: t.updated_at.to_rfc3339(),
            unread_for_user: t.unread_for_user,
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
    status: String,
    messages: Vec<AppTicketMessage>,
}

/// GET /api/v2/app/tickets/{id} — тикет с перепиской. Владение проверяет
/// сервис (`get_ticket` с is_admin=false и Some(user_id)).
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
        status: ticket.status,
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
                    (StatusCode::INTERNAL_SERVER_ERROR, "Cannot reply to ticket")
                        .into_response()
                }
            }
        }
    }
}
