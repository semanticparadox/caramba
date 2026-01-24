use axum::{
    extract::State,
    response::{IntoResponse, Json},
    http::StatusCode,
};
use tracing::{info, error};
use crate::AppState;
use exarobot_shared::api::{HeartbeatRequest, HeartbeatResponse, AgentAction};
use exarobot_shared::config::ConfigResponse;

/// Handle agent heartbeat
/// POST /api/v2/node/heartbeat
pub async fn heartbeat(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    let node_res = sqlx::query!("SELECT id, status FROM nodes WHERE join_token = ?", token)
        .fetch_optional(&state.pool)
        .await;

    let node = match node_res {
        Ok(Some(n)) => n,
        Ok(None) => {
            tracing::warn!("Heartbeat from unknown token: {}", token);
            return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
        }
        Err(e) => {
            error!("DB Error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    info!("💓 Heartbeat from Node {}: ver={}, uptime={}", node.id, payload.version, payload.uptime);

    // 3. Update Status
    let new_status = if node.status == "new" || node.status == "installing" { "active" } else { "active" }; // Force active on heartbeat
    
    let db_res = sqlx::query!(
        "UPDATE nodes SET last_seen = CURRENT_TIMESTAMP, status = ?, version = ? WHERE id = ?",
        new_status,
        payload.version,
        node.id
    )
    .execute(&state.pool)
    .await;
    
    // 4. Check if config update is needed (hash mismatch)
    // We need to fetch the stored hash or generate it? 
    // Agent sends payload.config_hash. We compare it with what we think it should be.
    // For now, let's just trust request. If agent has hash X, and we think it's Y, return UpdateConfig.
    // Simple logic: Always return None unless we implement hash cache. 
    // Better: Compare payload.config_hash with DB hash? We don't store config hash in DB yet.
    // Let's implement robust hash check in next step, for now success.
    
    (StatusCode::OK, Json(HeartbeatResponse {
        success: true,
        action: AgentAction::None,
    })).into_response()
}

/// Get Node Configuration
/// GET /api/v2/node/config
pub async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    let node_res = sqlx::query!("SELECT id FROM nodes WHERE join_token = ?", token)
        .fetch_optional(&state.pool)
        .await;

    let node_id = match node_res {
        Ok(Some(n)) => n.id,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response(),
    };

    // 3. Generate Config
    // If keys are missing, generate them now? 
    // OrchestrationService::generate_node_config_json should handle logic.
    match state.orchestration_service.generate_node_config_json(node_id).await {
        Ok((_, config_value)) => {
            let config_str = config_value.to_string();
            let hash = format!("{:x}", md5::compute(config_str.as_bytes()));
            
            (StatusCode::OK, Json(ConfigResponse {
                hash,
                content: config_value,
            })).into_response()
        },
        Err(e) => {
            error!("Config generation failed for node {}: {}", node_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}
