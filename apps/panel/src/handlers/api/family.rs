use axum::{
    extract::{State, Json},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use axum_extra::extract::cookie::CookieJar;
use tracing::error;

use crate::AppState;
use crate::handlers::admin::auth::get_auth_user_id;

#[derive(Serialize)]
pub struct InviteResponse {
    pub code: String,
    pub expires_at: String,
    pub max_uses: i32,
    pub used_count: i32,
}

#[derive(Deserialize)]
pub struct GenerateInviteRequest {
    pub max_uses: Option<i32>, // Default 1
    pub duration_days: Option<i32>, // Default 7
}

#[derive(Deserialize)]
pub struct RedeemInviteRequest {
    pub code: String,
}

pub async fn generate_invite(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(payload): Json<GenerateInviteRequest>,
) -> impl IntoResponse {
    let user_id = match get_auth_user_id(&state, &jar).await {
        Some(id) => id,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    // Check if user has an active subscription?
    // Policy: Only users with active subscriptions can invite? Or any user?
    // Let's assume any user for now, but strictly speaking only those with valid plans should probably invite.
    // For now, let's just proceed.

    let max_uses = payload.max_uses.unwrap_or(1).max(1).min(100); // specific limits
    let duration = payload.duration_days.unwrap_or(7).max(1).min(30);

    match state.store_service.create_family_invite(user_id, max_uses, duration).await {
        Ok(invite) => {
             Json(InviteResponse {
                 code: invite.code,
                 expires_at: invite.expires_at.to_rfc3339(),
                 max_uses: invite.max_uses,
                 used_count: invite.used_count,
             }).into_response()
        },
        Err(e) => {
            error!("Failed to generate invite: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate invite").into_response()
        }
    }
}

pub async fn redeem_invite(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(payload): Json<RedeemInviteRequest>,
) -> impl IntoResponse {
    let user_id = match get_auth_user_id(&state, &jar).await {
        Some(id) => id,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    match state.store_service.redeem_family_invite(user_id, &payload.code).await {
        Ok(_) => {
            (axum::http::StatusCode::OK, "Successfully joined family").into_response()
        },
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("Invalid or expired") || err_msg.contains("already") {
                return (axum::http::StatusCode::BAD_REQUEST, err_msg).into_response();
            }
            error!("Failed to redeem invite: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to redeem invite").into_response()
        }
    }
}
