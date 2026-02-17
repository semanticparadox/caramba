use axum::{
    extract::{State, Path},
    Json,
    response::IntoResponse,
    http::StatusCode,
};
use crate::AppState;
use serde::{Deserialize, Serialize};

// Helper struct for bot verification (stub)
#[derive(Deserialize)]
pub struct VerifyUserRequest {
    pub telegram_id: i64,
}

#[derive(Serialize)]
pub struct VerifyUserResponse {
    pub verified: bool,
    pub user_id: Option<i64>,
    pub username: Option<String>,
}

pub async fn verify_user(
    State(state): State<AppState>,
    Json(payload): Json<VerifyUserRequest>,
) -> impl IntoResponse {
    // Check if user exists with this telegram_id
    let user: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, username FROM users WHERE telegram_id = $1"
    )
    .bind(payload.telegram_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some((id, username)) = user {
        Json(VerifyUserResponse {
            verified: true,
            user_id: Some(id),
            username: Some(username),
        })
    } else {
        Json(VerifyUserResponse {
            verified: false,
            user_id: None,
            username: None,
        })
    }
}

pub async fn link_user(
    State(state): State<AppState>,
    Json(payload): Json<VerifyUserRequest>,
) -> impl IntoResponse {
     // Stub: Logic to link user
     // In real imp, this would take a link token
     StatusCode::NOT_IMPLEMENTED
}
