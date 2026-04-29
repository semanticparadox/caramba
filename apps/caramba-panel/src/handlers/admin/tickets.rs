// Tickets Module — admin web UI for support ticket management.
//
// Архитектурное решение: обращения к БД выполняются напрямую через `state.pool`
// (SQLX) без делегирования в `tickets_service`, чтобы не создавать зависимости
// между admin web UI и сервисным слоем. Интерфейс шаблонов стабилен.
//
// Таблицы (созданы миграцией 20260429000000_notifications_and_tickets.sql):
//   tickets          — основная таблица тикетов
//   ticket_messages  — сообщения в тикете (sender_role: user|admin|system)
//   users            — пользователи (full_name, username, tg_id)
//   admins           — администраторы (username, tg_id)
//
// tg_id администратора для вызовов `assign`/`set_status`/`add_admin_message`
// получаем из записи admins: JOIN admins ON admins.username = <session_username>.
// Если записи нет — падаем в env-var ADMIN_DEFAULT_TG_ID (задокументировано ниже).

use askama::Template;
use axum::{
    extract::{Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use sqlx::FromRow;
use tracing::{error, warn};

use super::auth::{get_auth_user, is_authenticated};
use crate::AppState;

// ── Константы ──────────────────────────────────────────────────────────────

const PAGE_SIZE: i64 = 25;

// ── БД-модели (mapping on raw rows) ───────────────────────────────────────

#[derive(Debug, Clone, FromRow)]
struct TicketRow {
    pub id: i64,
    pub subject: String,
    pub status: String,
    pub category: String,
    pub assignee_name: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[allow(dead_code)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    // JOIN с users
    pub user_id: i64,
    pub user_display_name: Option<String>,
    pub user_username: Option<String>,
    pub message_count: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct TicketDetailRow {
    pub id: i64,
    pub subject: String,
    pub status: String,
    pub category: String,
    pub assignee_name: Option<String>,
    #[allow(dead_code)]
    pub assignee_tg_id: Option<i64>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub user_id: i64,
    pub user_display_name: Option<String>,
    pub user_username: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct MessageRow {
    pub id: i64,
    pub body: String,
    pub sender_role: String, // "user" | "admin" | "system"
    pub sender_tg_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub attachments_json: Option<serde_json::Value>,
}

// ── View-модели для шаблонов ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TicketListItem {
    pub id: i64,
    pub subject: String,
    pub status: String,
    pub category: String,
    pub assignee_name: String,
    pub updated_at_display: String,
    pub updated_at_iso: String,
    pub user_id: i64,
    pub user_display_name: String,
    pub user_username: String,
    pub user_name_initial: String,
    pub message_count: i64,
}

#[derive(Debug, Clone)]
pub struct MessageView {
    pub id: i64,
    pub body: String,
    pub sender_type: String,
    pub sender_label: String,
    pub created_at_display: String,
    pub created_at_iso: String,
    pub has_attachments: bool,
    pub attachments: Vec<AttachmentView>,
}

#[derive(Debug, Clone)]
pub struct AttachmentView {
    pub url: String,
    pub filename: String,
    pub is_image: bool,
}

// ── Askama шаблоны ────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "admin/tickets/list.html")]
pub struct TicketsListTemplate {
    pub tickets: Vec<TicketListItem>,
    pub status_filter: String,
    pub search: String,
    pub page: i64,
    pub total: i64,
    pub total_pages: i64,
    pub offset: i64,
    pub items_end: i64, // offset + tickets.len() as i64, precomputed for template
    pub open_count: i64,
    pub total_display: i64,
    // base.html vars
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "admin/tickets/detail.html")]
pub struct TicketDetailTemplate {
    pub ticket_id: i64,
    pub subject: String,
    pub status: String,
    pub category: String,
    pub created_at_display: String,
    pub updated_at_display: String,
    pub updated_at_iso: String,
    pub user_id: i64,
    pub user_display_name: String,
    pub user_username: String,
    pub user_name_initial: String,
    pub assignee_name: String,
    pub assignee_initial: String,
    pub message_count: i64, // precomputed messages.len() as i64 for template
    pub messages: Vec<MessageView>,
    // base.html vars
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

// ── Query-параметры ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TicketListQuery {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub page: Option<i64>,
}

#[derive(Deserialize)]
pub struct ReplyForm {
    pub body: String,
}

#[derive(Deserialize)]
pub struct StatusForm {
    pub status: String,
}

// ── Вспомогательные функции ────────────────────────────────────────────────

fn format_datetime(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M UTC").to_string()
}

fn name_initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Получить tg_id текущего администратора по имени сессии.
/// Порядок: 1) JOIN admins WHERE username = session_username;
///           2) env var ADMIN_DEFAULT_TG_ID;
///           3) 0 (бот проигнорирует assign/notify).
///
/// Почему env-var: admins таблица может не иметь колонки tg_id
/// в старых инсталляциях; env-var — безопасный fallback для одиночного
/// администратора.
async fn get_admin_tg_id(state: &AppState, session_username: &str) -> i64 {
    // Пробуем извлечь tg_id из таблицы admins
    let result: Option<i64> =
        sqlx::query_scalar("SELECT tg_id FROM admins WHERE username = $1 LIMIT 1")
            .bind(session_username)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

    if let Some(id) = result {
        if id > 0 {
            return id;
        }
    }

    // Fallback: env var
    if let Ok(val) = std::env::var("ADMIN_DEFAULT_TG_ID") {
        if let Ok(id) = val.parse::<i64>() {
            if id > 0 {
                return id;
            }
        }
    }

    warn!(
        session_username,
        "Could not resolve tg_id for admin — ticket actions will use tg_id=0"
    );
    0
}

fn parse_attachments(json: &serde_json::Value) -> Vec<AttachmentView> {
    let arr = match json.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    arr.iter()
        .filter_map(|v| {
            let url = v.get("url").and_then(|u| u.as_str())?.to_string();
            let filename = v
                .get("filename")
                .and_then(|f| f.as_str())
                .unwrap_or("attachment")
                .to_string();
            let ext = url.rsplit('.').next().unwrap_or("").to_lowercase();
            let is_image = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp");
            Some(AttachmentView {
                url,
                filename,
                is_image,
            })
        })
        .collect()
}

fn render_template<T: askama::Template>(tpl: T) -> axum::response::Response {
    match tpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {e}"),
        )
            .into_response(),
    }
}

// ── Обработчики ─────────────────────────────────────────────────────────────

/// GET /admin/tickets — список тикетов с фильтрацией и пагинацией
pub async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<TicketListQuery>,
) -> axum::response::Response {
    if !is_authenticated(&state, &jar).await {
        return Redirect::to(&format!("{}/login", state.admin_path)).into_response();
    }

    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());

    let page = params.page.unwrap_or(0).max(0);
    let offset = page * PAGE_SIZE;
    let status_filter = params.status.trim().to_string();
    let search = params.q.trim().to_string();

    // Запрос тикетов с JOIN на users и подсчётом сообщений.
    // Используем ILIKE для регистронезависимого поиска по subject.
    let (rows, total, open_count): (Vec<TicketRow>, i64, i64) =
        match fetch_ticket_list(&state, &status_filter, &search, offset).await {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to fetch ticket list: {e}");
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to load tickets",
                )
                    .into_response();
            }
        };

    let total_pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;

    let tickets: Vec<TicketListItem> = rows
        .into_iter()
        .map(|r| {
            let display_name = r
                .user_display_name
                .as_deref()
                .unwrap_or("Unknown")
                .to_string();
            let username_str = r
                .user_username
                .as_deref()
                .unwrap_or("unknown")
                .to_string();
            TicketListItem {
                id: r.id,
                subject: r.subject,
                status: r.status,
                category: r.category,
                assignee_name: r.assignee_name.unwrap_or_default(),
                updated_at_display: format_datetime(r.updated_at),
                updated_at_iso: r.updated_at.to_rfc3339(),
                user_id: r.user_id,
                user_name_initial: name_initial(&display_name),
                user_display_name: display_name,
                user_username: username_str,
                message_count: r.message_count.unwrap_or(0),
            }
        })
        .collect();

    let items_end = offset + tickets.len() as i64;
    render_template(TicketsListTemplate {
        tickets,
        status_filter,
        search,
        page,
        total,
        total_pages,
        offset,
        items_end,
        open_count,
        total_display: total,
        is_auth: true,
        username,
        admin_path: state.admin_path.clone(),
        active_page: "tickets".to_string(),
    })
}

/// GET /admin/tickets/{id} — детальная страница тикета
pub async fn detail(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> axum::response::Response {
    if !is_authenticated(&state, &jar).await {
        return Redirect::to(&format!("{}/login", state.admin_path)).into_response();
    }

    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());

    // Загружаем тикет
    let ticket: Option<TicketDetailRow> = sqlx::query_as(
        r#"
        SELECT
            t.id,
            t.subject,
            t.status,
            t.category,
            t.updated_at,
            t.created_at,
            t.user_id,
            u.full_name  AS user_display_name,
            u.username   AS user_username,
            a.username   AS assignee_name,
            t.assignee_tg_id
        FROM tickets t
        LEFT JOIN users  u ON u.id  = t.user_id
        LEFT JOIN admins a ON a.tg_id = t.assignee_tg_id
        WHERE t.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let ticket = match ticket {
        Some(t) => t,
        None => {
            return (axum::http::StatusCode::NOT_FOUND, "Ticket not found").into_response();
        }
    };

    // Загружаем сообщения
    let msg_rows: Vec<MessageRow> = sqlx::query_as(
        r#"
        SELECT
            id,
            body,
            sender_role,
            sender_tg_id,
            created_at,
            attachments_json
        FROM ticket_messages
        WHERE ticket_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let messages: Vec<MessageView> = msg_rows
        .into_iter()
        .map(|m| {
            let atts = m
                .attachments_json
                .as_ref()
                .map(parse_attachments)
                .unwrap_or_default();
            let has_attachments = !atts.is_empty();
            // Формируем человекочитаемую метку отправителя
            let sender_label = match m.sender_role.as_str() {
                "admin" => m
                    .sender_tg_id
                    .map(|id| format!("Admin (tg:{})", id))
                    .unwrap_or_else(|| "Admin".to_string()),
                "system" => "System".to_string(),
                _ => "User".to_string(),
            };
            MessageView {
                id: m.id,
                body: m.body,
                sender_type: m.sender_role,
                sender_label,
                created_at_display: format_datetime(m.created_at),
                created_at_iso: m.created_at.to_rfc3339(),
                has_attachments,
                attachments: atts,
            }
        })
        .collect();

    let user_display_name = ticket
        .user_display_name
        .as_deref()
        .unwrap_or("Unknown")
        .to_string();
    let user_username = ticket
        .user_username
        .as_deref()
        .unwrap_or("unknown")
        .to_string();
    let assignee_name = ticket.assignee_name.unwrap_or_default();

    let message_count = messages.len() as i64;
    render_template(TicketDetailTemplate {
        ticket_id: ticket.id,
        subject: ticket.subject,
        status: ticket.status,
        category: ticket.category,
        created_at_display: format_datetime(ticket.created_at),
        updated_at_display: format_datetime(ticket.updated_at),
        updated_at_iso: ticket.updated_at.to_rfc3339(),
        user_id: ticket.user_id,
        user_name_initial: name_initial(&user_display_name),
        user_display_name,
        user_username,
        assignee_initial: name_initial(&assignee_name),
        assignee_name,
        message_count,
        messages,
        is_auth: true,
        username,
        admin_path: state.admin_path.clone(),
        active_page: "tickets".to_string(),
    })
}

/// POST /admin/tickets/{id}/reply — добавить ответ администратора
pub async fn reply(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<ReplyForm>,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let body = form.body.trim().to_string();

    // Валидация: тело не пустое, не длиннее 4000 символов
    if body.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Reply body is required").into_response();
    }
    if body.chars().count() > 4000 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Reply exceeds 4000 character limit",
        )
            .into_response();
    }

    let session_username = get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());
    let admin_tg_id = get_admin_tg_id(&state, &session_username).await;

    // Delegate to TicketsService so business logic stays in one place:
    // service handles message insert + status transition (awaiting_user) +
    // user notification (Mini App inbox + bot DM per user prefs).
    // This previously was a raw SQL INSERT that flipped status the WRONG
    // way (awaiting_user -> in_progress) and never notified the user.
    if let Err(e) = state
        .tickets_svc
        .add_admin_message(id, admin_tg_id, &body, Vec::new())
        .await
    {
        error!(ticket_id = id, admin = %session_username, "Failed to add admin reply: {e}");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save reply",
        )
            .into_response();
    }

    // HTMX: перезагрузить страницу полностью (проще, чем частичный рендер)
    (
        axum::http::StatusCode::OK,
        [(
            axum::http::header::HeaderName::from_static("hx-redirect"),
            format!("{}/tickets/{}", state.admin_path, id),
        )],
        "",
    )
        .into_response()
}

/// POST /admin/tickets/{id}/assign — назначить тикет себе
pub async fn assign(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let session_username = get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());
    let admin_tg_id = get_admin_tg_id(&state, &session_username).await;

    match state.tickets_svc.assign(id, admin_tg_id).await {
        Ok(_) => (
            axum::http::StatusCode::OK,
            [(
                axum::http::header::HeaderName::from_static("hx-redirect"),
                format!("{}/tickets/{}", state.admin_path, id),
            )],
            "",
        )
            .into_response(),
        Err(e) => {
            error!(ticket_id = id, "Failed to assign ticket: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to assign ticket",
            )
                .into_response()
        }
    }
}

/// POST /admin/tickets/{id}/status — сменить статус тикета
pub async fn set_status(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<StatusForm>,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let new_status = form.status.trim().to_string();
    let session_username = get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());
    let admin_tg_id = get_admin_tg_id(&state, &session_username).await;

    // tickets_service validates allowed values + sets closed_at on 'closed'.
    match state
        .tickets_svc
        .set_status(id, &new_status, admin_tg_id)
        .await
    {
        Ok(_) => (
            axum::http::StatusCode::OK,
            [(
                axum::http::header::HeaderName::from_static("hx-redirect"),
                format!("{}/tickets/{}", state.admin_path, id),
            )],
            "",
        )
            .into_response(),
        Err(e) => {
            let msg = e.to_string();
            let code = if msg.contains("Недопустимый") {
                axum::http::StatusCode::BAD_REQUEST
            } else {
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            error!(ticket_id = id, new_status = %new_status, "Failed to set ticket status: {e}");
            (code, "Failed to update status").into_response()
        }
    }
}

// ── Запросы к БД ──────────────────────────────────────────────────────────

async fn fetch_ticket_list(
    state: &AppState,
    status_filter: &str,
    search: &str,
    offset: i64,
) -> Result<(Vec<TicketRow>, i64, i64), sqlx::Error> {
    // Строим условия фильтрации
    let status_cond = if status_filter.is_empty() {
        "TRUE".to_string()
    } else {
        format!("t.status = '{}'", status_filter.replace('\'', ""))
    };
    let search_cond = if search.is_empty() {
        "TRUE".to_string()
    } else {
        // Используем параметризованный запрос через bind ниже
        "t.subject ILIKE $3".to_string()
    };

    let query_str = format!(
        r#"
        SELECT
            t.id,
            t.subject,
            t.status,
            t.category,
            t.updated_at,
            t.created_at,
            t.user_id,
            u.full_name  AS user_display_name,
            u.username   AS user_username,
            a.username   AS assignee_name,
            (SELECT COUNT(*) FROM ticket_messages m WHERE m.ticket_id = t.id) AS message_count
        FROM tickets t
        LEFT JOIN users  u ON u.id  = t.user_id
        LEFT JOIN admins a ON a.tg_id = t.assignee_tg_id
        WHERE {status_cond} AND {search_cond}
        ORDER BY t.updated_at DESC
        LIMIT $1 OFFSET $2
        "#,
        status_cond = status_cond,
        search_cond = search_cond,
    );

    let search_pattern = format!("%{}%", search);

    let rows: Vec<TicketRow> = if search.is_empty() {
        sqlx::query_as(&query_str)
            .bind(PAGE_SIZE)
            .bind(offset)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query_as(&query_str)
            .bind(PAGE_SIZE)
            .bind(offset)
            .bind(&search_pattern)
            .fetch_all(&state.pool)
            .await?
    };

    // Общий счётчик для пагинации
    let count_str = format!(
        r#"SELECT COUNT(*) FROM tickets t WHERE {status_cond} AND {search_cond}"#,
        status_cond = status_cond,
        search_cond = if search.is_empty() {
            "TRUE".to_string()
        } else {
            "t.subject ILIKE $1".to_string()
        },
    );

    let total: i64 = if search.is_empty() {
        sqlx::query_scalar(&count_str)
            .fetch_one(&state.pool)
            .await?
    } else {
        sqlx::query_scalar(&count_str)
            .bind(&search_pattern)
            .fetch_one(&state.pool)
            .await?
    };

    // Количество открытых тикетов (для заголовка)
    let open_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tickets WHERE status = 'open'")
            .fetch_one(&state.pool)
            .await?;

    Ok((rows, total, open_count))
}
