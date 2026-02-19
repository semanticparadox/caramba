use axum::{
    extract::{Json, State},
    response::IntoResponse,
};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::error;

use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
struct ClientClaims {
    sub: String,
    #[serde(rename = "exp")]
    _exp: usize,
    role: String,
}

fn sign_legacy_user_id(user_id: i64, secret: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts arbitrary key size");
    mac.update(user_id.to_string().as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    format!("{}.{}", user_id, signature)
}

fn verify_legacy_user_id(token: &str, secret: &str) -> Option<i64> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return None;
    }
    let user_id = parts[0].parse::<i64>().ok()?;
    let expected = sign_legacy_user_id(user_id, secret);
    let expected_signature = expected.split('.').nth(1)?;
    if parts[1] == expected_signature {
        Some(user_id)
    } else {
        None
    }
}

fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

async fn resolve_user_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<i64> {
    let token = extract_bearer_token(headers)?;

    // 1) Preferred path: JWT issued by /api/client/auth/telegram
    if let Ok(token_data) = decode::<ClientClaims>(
        token,
        &DecodingKey::from_secret(state.session_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    ) {
        if token_data.claims.role == "client" {
            if let Ok(tg_id) = token_data.claims.sub.parse::<i64>() {
                let user_id: Option<i64> =
                    sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
                        .bind(tg_id)
                        .fetch_optional(&state.pool)
                        .await
                        .ok()
                        .flatten();
                if user_id.is_some() {
                    return user_id;
                }
            }
        }
    }

    // 2) Backward compatibility: legacy signed token used by old handlers/client flow.
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    if bot_token.is_empty() {
        return None;
    }
    verify_legacy_user_id(token, &bot_token)
}

#[derive(Serialize)]
pub struct InviteResponse {
    pub code: String,
    pub expires_at: String,
    pub max_uses: i32,
    pub used_count: i32,
}

#[derive(Deserialize)]
pub struct GenerateInviteRequest {
    pub max_uses: Option<i32>,      // Default 1
    pub duration_days: Option<i32>, // Default 7
}

#[derive(Deserialize)]
pub struct RedeemInviteRequest {
    pub code: String,
}

pub async fn generate_invite(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<GenerateInviteRequest>,
) -> impl IntoResponse {
    let user_id = match resolve_user_id(&state, &headers).await {
        Some(id) => id,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    // Check if user has an active subscription?
    // Policy: Only users with active subscriptions can invite? Or any user?
    // Let's assume any user for now, but strictly speaking only those with valid plans should probably invite.
    // For now, let's just proceed.

    let max_uses = payload.max_uses.unwrap_or(1).max(1).min(100); // specific limits
    let duration = payload.duration_days.unwrap_or(7).max(1).min(30);

    match state
        .store_service
        .create_family_invite(user_id, max_uses, duration)
        .await
    {
        Ok(invite) => Json(InviteResponse {
            code: invite.code,
            expires_at: invite.expires_at.to_rfc3339(),
            max_uses: invite.max_uses,
            used_count: invite.used_count,
        })
        .into_response(),
        Err(e) => {
            error!("Failed to generate invite: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to generate invite",
            )
                .into_response()
        }
    }
}

pub async fn redeem_invite(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<RedeemInviteRequest>,
) -> impl IntoResponse {
    let user_id = match resolve_user_id(&state, &headers).await {
        Some(id) => id,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    match state
        .store_service
        .redeem_family_invite(user_id, &payload.code)
        .await
    {
        Ok(_) => (axum::http::StatusCode::OK, "Successfully joined family").into_response(),
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("Invalid or expired") || err_msg.contains("already") {
                return (axum::http::StatusCode::BAD_REQUEST, err_msg).into_response();
            }
            error!("Failed to redeem invite: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to redeem invite",
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{sign_legacy_user_id, verify_legacy_user_id};

    #[test]
    fn legacy_token_roundtrip_valid() {
        let secret = "secret123";
        let token = sign_legacy_user_id(42, secret);
        assert_eq!(verify_legacy_user_id(&token, secret), Some(42));
    }

    #[test]
    fn legacy_token_rejects_tampered_signature() {
        let secret = "secret123";
        let mut token = sign_legacy_user_id(42, secret);
        token.push('x');
        assert_eq!(verify_legacy_user_id(&token, secret), None);
    }

    #[test]
    fn legacy_token_rejects_invalid_format() {
        assert_eq!(verify_legacy_user_id("not-a-token", "secret123"), None);
    }

    #[test]
    fn legacy_token_rejects_wrong_secret() {
        let token = sign_legacy_user_id(42, "secret123");
        assert_eq!(verify_legacy_user_id(&token, "another-secret"), None);
    }
}
