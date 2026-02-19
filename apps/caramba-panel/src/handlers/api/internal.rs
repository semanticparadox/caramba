use crate::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use caramba_db::models::network::Inbound;
use caramba_db::models::node::Node;
use caramba_db::repositories::node_repo::NodeRepository;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

#[derive(sqlx::FromRow)]
struct FrontendTokenRow {
    auth_token_hash: Option<String>,
    token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

async fn is_valid_frontend_token(state: &AppState, token: &str) -> bool {
    let rows: Vec<FrontendTokenRow> = sqlx::query_as(
        "SELECT auth_token_hash, token_expires_at
         FROM frontend_servers
         WHERE is_active = TRUE AND auth_token_hash IS NOT NULL",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    for row in rows {
        if let Some(expires_at) = row.token_expires_at {
            if expires_at < Utc::now() {
                continue;
            }
        }

        if let Some(hash) = row.auth_token_hash {
            if bcrypt::verify(token, &hash).unwrap_or(false) {
                return true;
            }
        }
    }

    false
}

async fn authorize_internal_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), StatusCode> {
    let token = extract_bearer_token(headers).ok_or(StatusCode::UNAUTHORIZED)?;
    if token.trim().is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Preferred authorization: dedicated internal shared token.
    if let Ok(internal_token) = std::env::var("INTERNAL_API_TOKEN") {
        let expected = internal_token.trim();
        if !expected.is_empty() && expected == token {
            return Ok(());
        }
    }

    // Compatibility authorization: active frontend token (hashed in DB).
    if is_valid_frontend_token(state, token).await {
        return Ok(());
    }

    Err(StatusCode::UNAUTHORIZED)
}

#[derive(Serialize, sqlx::FromRow)]
pub struct InternalNode {
    #[serde(flatten)]
    pub node: Node,
    pub inbounds: Vec<Inbound>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct InternalSubscription {
    pub id: i64,
    pub user_id: i64,
    pub status: String,
    pub used_traffic: i64,
    pub subscription_uuid: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct InternalUserKeys {
    pub user_uuid: String,
    pub hy2_password: String,
}

#[derive(Deserialize)]
pub struct LegacyFrontendHeartbeat {
    pub requests_count: u64,
    pub bandwidth_used: u64,
}

pub async fn get_active_nodes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = authorize_internal_request(&state, &headers).await {
        return status.into_response();
    }

    let node_repo = NodeRepository::new(state.pool.clone());
    let nodes: Vec<Node> = node_repo
        .get_all_nodes()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n.is_enabled && n.status == "active")
        .collect();

    let mut internal_nodes = Vec::new();

    for node in nodes {
        let inbounds: Vec<Inbound> = node_repo
            .get_inbounds_by_node(node.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|inb| inb.enable)
            .collect();

        internal_nodes.push(InternalNode { node, inbounds });
    }

    Json(internal_nodes).into_response()
}

pub async fn get_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(uuid): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = authorize_internal_request(&state, &headers).await {
        return status.into_response();
    }

    let sub: Option<InternalSubscription> = sqlx::query_as(
        "SELECT id, user_id, status, used_traffic, subscription_uuid FROM subscriptions WHERE subscription_uuid = $1",
    )
    .bind(uuid)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some(s) = sub {
        Json(s).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Subscription not found").into_response()
    }
}

pub async fn get_user_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    if let Err(status) = authorize_internal_request(&state, &headers).await {
        return status.into_response();
    }

    let user: Option<(String, i64)> = sqlx::query_as(
        r#"
        SELECT s.vless_uuid, u.tg_id
        FROM subscriptions s
        JOIN users u ON u.id = s.user_id
        WHERE s.user_id = $1
          AND s.status = 'active'
          AND s.vless_uuid IS NOT NULL
        ORDER BY s.expires_at DESC NULLS LAST, s.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some((uuid, tg_id)) = user {
        let hy2_password = format!("{}:{}", tg_id, uuid.replace("-", ""));
        Json(InternalUserKeys {
            user_uuid: uuid,
            hy2_password,
        })
        .into_response()
    } else {
        (StatusCode::NOT_FOUND, "User keys not found").into_response()
    }
}

/// Backward-compatible no-op endpoint used by legacy caramba-sub.
pub async fn frontend_heartbeat(Json(payload): Json<LegacyFrontendHeartbeat>) -> impl IntoResponse {
    let _ = (payload.requests_count, payload.bandwidth_used);
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn frontend_heartbeat_returns_ok() {
        let response = frontend_heartbeat(Json(LegacyFrontendHeartbeat {
            requests_count: 1,
            bandwidth_used: 1024,
        }))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
