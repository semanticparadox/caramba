//! Moderator ticket API — admin-panel-side JSON endpoints under
//! /admin/api/moderator/* for staff with role 'moderator' or 'admin'.
//!
//! This is the panel admin surface, NOT the client app: it is gated by the
//! admin cookie session (handlers/admin/auth.rs) plus the new admins.role
//! column. A 'moderator' can triage tickets (list/filter), claim them
//! (assign-to-self), and reply as support — nothing else. A full 'admin'
//! passes the same gate. All ticket business logic is delegated to
//! TicketsService so behaviour matches the bot + web admin paths exactly
//! (status transitions, user notifications).
//!
//! Moderator tools are intentionally absent from the client (/api/v2/app/*)
//! router — moderation is staff-only.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use crate::AppState;
use crate::handlers::admin::auth::{get_auth_user, is_authenticated};

/// Allowed staff roles for moderator endpoints.
const STAFF_ROLES: &[&str] = &["moderator", "admin"];

/// Resolves the session admin's (role, tg_id). Returns None when the session is
/// missing/invalid or the admin row is gone. tg_id falls back to
/// ADMIN_DEFAULT_TG_ID, mirroring handlers/admin/tickets.rs::get_admin_tg_id, so
/// assignment/reply still attribute correctly on single-admin installs.
async fn resolve_staff(state: &AppState, jar: &CookieJar) -> Option<(String, i64)> {
    let username = get_auth_user(state, jar).await?;

    let row: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT role, tg_id FROM admins WHERE username = $1 LIMIT 1")
            .bind(&username)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

    let (role, tg_id_opt) = row?;

    let tg_id = match tg_id_opt {
        Some(id) if id > 0 => id,
        _ => std::env::var("ADMIN_DEFAULT_TG_ID")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .unwrap_or(0),
    };

    Some((role, tg_id))
}

/// Gate: requires a valid admin session AND a staff role. Returns the resolved
/// (role, admin_tg_id) on success, or an HTTP response to return on failure.
async fn require_staff(
    state: &AppState,
    jar: &CookieJar,
) -> Result<(String, i64), axum::response::Response> {
    if !is_authenticated(state, jar).await {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    }

    match resolve_staff(state, jar).await {
        Some((role, tg_id)) if STAFF_ROLES.contains(&role.as_str()) => Ok((role, tg_id)),
        Some(_) => Err((StatusCode::FORBIDDEN, "Insufficient role").into_response()),
        None => Err((StatusCode::UNAUTHORIZED, "Unauthorized").into_response()),
    }
}

#[derive(Deserialize)]
pub struct TicketListQuery {
    /// Optional status filter (open|in_progress|awaiting_user|resolved|closed).
    pub status: Option<String>,
    /// When true, only tickets assigned to the calling moderator.
    #[serde(default)]
    pub mine: bool,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    25
}

/// GET /admin/api/moderator/tickets — list/filter tickets for triage.
pub async fn list_tickets(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<TicketListQuery>,
) -> impl IntoResponse {
    let (_role, admin_tg_id) = match require_staff(&state, &jar).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let limit = q.limit.clamp(1, 100);
    let offset = q.offset.max(0);
    let assignee_filter = if q.mine { Some(admin_tg_id) } else { None };

    match state
        .tickets_svc
        .list_admin_tickets(q.status.as_deref(), assignee_filter, limit, offset)
        .await
    {
        Ok(tickets) => Json(tickets).into_response(),
        Err(e) => {
            tracing::error!(err = %e, "moderator list_tickets failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /admin/api/moderator/tickets/{id} — ticket detail + messages.
pub async fn get_ticket(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(ticket_id): Path<i64>,
) -> impl IntoResponse {
    if let Err(resp) = require_staff(&state, &jar).await {
        return resp;
    }

    match state.tickets_svc.get_ticket(ticket_id, true, None).await {
        Ok((ticket, messages)) => Json(serde_json::json!({
            "ticket": ticket,
            "messages": messages,
        }))
        .into_response(),
        Err(e) => {
            use crate::services::tickets_service::TicketError;
            match e {
                TicketError::NotFound => {
                    (StatusCode::NOT_FOUND, "Ticket not found").into_response()
                }
                TicketError::Forbidden => (StatusCode::FORBIDDEN, "Access denied").into_response(),
                TicketError::Closed => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "Ticket is closed").into_response()
                }
                TicketError::Internal(_) => {
                    tracing::error!(err = %e, "moderator get_ticket failed");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
    }
}

/// POST /admin/api/moderator/tickets/{id}/assign — claim the ticket for the
/// calling moderator (self-assign). Mirrors the web admin self-assign.
pub async fn assign_ticket(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(ticket_id): Path<i64>,
) -> impl IntoResponse {
    let (_role, admin_tg_id) = match require_staff(&state, &jar).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    match state.tickets_svc.assign(ticket_id, admin_tg_id).await {
        Ok(ticket) => Json(ticket).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("не найден") {
                (StatusCode::NOT_FOUND, "Ticket not found").into_response()
            } else {
                tracing::error!(err = %e, ticket_id, "moderator assign_ticket failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

#[derive(Deserialize)]
pub struct ReplyReq {
    pub body: String,
}

/// POST /admin/api/moderator/tickets/{id}/reply — reply as support.
/// Delegates to TicketsService::add_admin_message (status -> awaiting_user +
/// user notification), attributing the message to the moderator's tg_id.
pub async fn reply_ticket(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(ticket_id): Path<i64>,
    Json(req): Json<ReplyReq>,
) -> impl IntoResponse {
    let (_role, admin_tg_id) = match require_staff(&state, &jar).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let body = req.body.trim();
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "Reply body is required").into_response();
    }
    if body.chars().count() > 4000 {
        return (
            StatusCode::BAD_REQUEST,
            "Reply exceeds 4000 character limit",
        )
            .into_response();
    }

    match state
        .tickets_svc
        .add_admin_message(ticket_id, admin_tg_id, body, Vec::new())
        .await
    {
        Ok(msg) => (StatusCode::CREATED, Json(msg)).into_response(),
        Err(e) => {
            let txt = e.to_string();
            if txt.contains("не найден") {
                (StatusCode::NOT_FOUND, "Ticket not found").into_response()
            } else {
                tracing::error!(err = %e, ticket_id, "moderator reply_ticket failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
