use crate::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
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

pub async fn ensure_worker_update_tables(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS worker_update_reports (
            id BIGSERIAL PRIMARY KEY,
            role TEXT NOT NULL,
            worker_id TEXT NOT NULL,
            current_version TEXT,
            target_version TEXT,
            status TEXT NOT NULL,
            message TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_worker_update_reports_role_created_at ON worker_update_reports(role, created_at DESC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS worker_runtime_status (
            id BIGSERIAL PRIMARY KEY,
            role TEXT NOT NULL,
            worker_id TEXT NOT NULL,
            current_version TEXT,
            target_version TEXT,
            last_state TEXT NOT NULL DEFAULT 'poll',
            last_message TEXT,
            last_seen TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(role, worker_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_worker_runtime_status_role_last_seen ON worker_runtime_status(role, last_seen DESC)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Deserialize)]
pub struct WorkerUpdatePollQuery {
    pub worker_id: Option<String>,
    pub current_version: Option<String>,
}

#[derive(Serialize)]
pub struct WorkerUpdatePollResponse {
    pub update: bool,
    pub target_version: Option<String>,
    pub asset_url: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkerUpdateReportRequest {
    pub worker_id: Option<String>,
    pub current_version: Option<String>,
    pub target_version: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

fn normalize_worker_role(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "sub" => Some("sub"),
        "bot" => Some("bot"),
        _ => None,
    }
}

fn normalize_version(raw: &str) -> String {
    raw.trim().trim_start_matches('v').to_string()
}

fn parse_semver_tuple(raw: &str) -> Option<(u64, u64, u64)> {
    let mut parts = raw.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn should_offer_worker_update(target_version: &str, current_version: &str) -> bool {
    let target = normalize_version(target_version);
    let current = normalize_version(current_version);

    if target.is_empty() {
        return false;
    }
    if current.is_empty() {
        return true;
    }

    match (parse_semver_tuple(&target), parse_semver_tuple(&current)) {
        (Some(t), Some(c)) => t > c,
        _ => target != current,
    }
}

pub async fn poll_worker_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(role): Path<String>,
    Query(query): Query<WorkerUpdatePollQuery>,
) -> impl IntoResponse {
    if let Err(status) = authorize_internal_request(&state, &headers).await {
        return status.into_response();
    }

    let role = match normalize_worker_role(&role) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "Invalid worker role").into_response(),
    };

    if let Err(e) = ensure_worker_update_tables(&state.pool).await {
        tracing::error!("Failed to ensure worker update tables: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
    }

    let target_version_key = format!("worker_{}_target_version", role);
    let update_url_key = format!("worker_{}_update_url", role);
    let update_hash_key = format!("worker_{}_update_hash", role);

    let target_version = state.settings.get_or_default(&target_version_key, "").await;
    let asset_url = state.settings.get_or_default(&update_url_key, "").await;
    let sha256 = state.settings.get_or_default(&update_hash_key, "").await;

    let current_version = query.current_version.unwrap_or_default();
    let worker_id = query
        .worker_id
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string();
    let has_update = !asset_url.trim().is_empty()
        && should_offer_worker_update(target_version.trim(), current_version.trim());

    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO worker_runtime_status (
            role, worker_id, current_version, target_version, last_state, last_message, last_seen, updated_at
        )
        VALUES (
            $1, $2, NULLIF($3, ''), NULLIF($4, ''), 'poll', NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
        )
        ON CONFLICT (role, worker_id)
        DO UPDATE SET
            current_version = EXCLUDED.current_version,
            target_version = EXCLUDED.target_version,
            last_state = 'poll',
            last_message = NULL,
            last_seen = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(role)
    .bind(if worker_id.is_empty() {
        "unknown"
    } else {
        worker_id.as_str()
    })
    .bind(current_version.trim())
    .bind(target_version.trim())
    .execute(&state.pool)
    .await
    {
        tracing::warn!("Failed to upsert worker runtime status on poll: {}", e);
    }

    let response = if has_update {
        WorkerUpdatePollResponse {
            update: true,
            target_version: Some(target_version.trim().to_string()),
            asset_url: Some(asset_url.trim().to_string()),
            sha256: if sha256.trim().is_empty() {
                None
            } else {
                Some(sha256.trim().to_string())
            },
        }
    } else {
        WorkerUpdatePollResponse {
            update: false,
            target_version: None,
            asset_url: None,
            sha256: None,
        }
    };

    Json(response).into_response()
}

pub async fn report_worker_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(role): Path<String>,
    Json(payload): Json<WorkerUpdateReportRequest>,
) -> impl IntoResponse {
    if let Err(status) = authorize_internal_request(&state, &headers).await {
        return status.into_response();
    }

    let role = match normalize_worker_role(&role) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "Invalid worker role").into_response(),
    };

    if let Err(e) = ensure_worker_update_tables(&state.pool).await {
        tracing::error!("Failed to ensure worker update tables: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
    }

    let status = payload.status.trim().to_ascii_lowercase();
    if !matches!(
        status.as_str(),
        "queued" | "started" | "success" | "failed" | "skipped"
    ) {
        return (StatusCode::BAD_REQUEST, "Invalid status").into_response();
    }

    let worker_id = payload
        .worker_id
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string();
    let current_version = payload.current_version.unwrap_or_default();
    let target_version = payload.target_version.unwrap_or_default();
    let message = payload.message.unwrap_or_default();

    let insert_result = sqlx::query(
        r#"
        INSERT INTO worker_update_reports (role, worker_id, current_version, target_version, status, message)
        VALUES ($1, $2, NULLIF($3, ''), NULLIF($4, ''), $5, NULLIF($6, ''))
        "#,
    )
    .bind(role)
    .bind(if worker_id.is_empty() {
        "unknown"
    } else {
        worker_id.as_str()
    })
    .bind(current_version.trim())
    .bind(target_version.trim())
    .bind(&status)
    .bind(message.trim())
    .execute(&state.pool)
    .await;

    match insert_result {
        Ok(_) => {
            if let Err(e) = sqlx::query(
                r#"
                INSERT INTO worker_runtime_status (
                    role, worker_id, current_version, target_version, last_state, last_message, last_seen, updated_at
                )
                VALUES (
                    $1, $2, NULLIF($3, ''), NULLIF($4, ''), $5, NULLIF($6, ''), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                )
                ON CONFLICT (role, worker_id)
                DO UPDATE SET
                    current_version = EXCLUDED.current_version,
                    target_version = EXCLUDED.target_version,
                    last_state = EXCLUDED.last_state,
                    last_message = EXCLUDED.last_message,
                    last_seen = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                "#,
            )
            .bind(role)
            .bind(if worker_id.is_empty() {
                "unknown"
            } else {
                worker_id.as_str()
            })
            .bind(current_version.trim())
            .bind(target_version.trim())
            .bind(status.as_str())
            .bind(message.trim())
            .execute(&state.pool)
            .await
            {
                tracing::warn!("Failed to upsert worker runtime status on report: {}", e);
            }
            StatusCode::OK.into_response()
        }
        Err(e) => {
            tracing::error!("Failed to store worker update report: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save report").into_response()
        }
    }
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
