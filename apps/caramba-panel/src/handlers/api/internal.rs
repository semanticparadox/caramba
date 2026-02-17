use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use crate::AppState;
use serde::Serialize;
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

pub async fn get_active_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let nodes: Vec<Node> = sqlx::query_as(
        "SELECT * FROM nodes WHERE is_enabled = true AND status = 'online' ORDER BY sort_order ASC",
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
        Json(Some(s))
    } else {
        Json(None)
    }
}

pub async fn get_user_keys(
    State(state): State<AppState>,
    Path(user_id): Path<i64>
) -> impl IntoResponse {
    let user: Option<(String, String)> = sqlx::query_as(
        "SELECT uuid, COALESCE(password_hash, '') as hy2_password FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some((uuid, hy2_password)) = user {
        Json(Some(InternalUserKeys {
            user_uuid: uuid,
            hy2_password,
        }))
    } else {
        Json(None)
    }
}
