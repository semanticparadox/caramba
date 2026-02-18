use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use crate::AppState;
use serde::Serialize;
use serde::Deserialize;
use caramba_db::models::node::Node;
use caramba_db::models::network::Inbound;

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

pub async fn get_active_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let nodes: Vec<Node> = sqlx::query_as(
        "SELECT * FROM nodes WHERE is_enabled = true AND status = 'active' ORDER BY sort_order ASC",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut internal_nodes = Vec::new();

    for node in nodes {
        let inbounds: Vec<Inbound> = sqlx::query_as(
            "SELECT * FROM inbounds WHERE node_id = $1 AND enable = true",
        )
        .bind(node.id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        internal_nodes.push(InternalNode {
            node,
            inbounds,
        });
    }

    Json(internal_nodes)
}

pub async fn get_subscription(
    State(state): State<AppState>,
    Path(uuid): Path<String>
) -> impl IntoResponse {
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
    Path(user_id): Path<i64>
) -> impl IntoResponse {
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
pub async fn frontend_heartbeat(
    Json(payload): Json<LegacyFrontendHeartbeat>,
) -> impl IntoResponse {
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
