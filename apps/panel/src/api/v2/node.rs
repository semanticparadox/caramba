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
    Json(payload): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    info!("💓 Heartbeat: ver={}, uptime={}", payload.version, payload.uptime);
    
    // TODO: Extract token from header, validate, update DB
    
    (StatusCode::OK, Json(HeartbeatResponse {
        success: true,
        action: AgentAction::None,
    }))
}

/// Get Node Configuration
/// GET /api/v2/node/config
pub async fn get_config(
    State(state): State<AppState>,
) -> impl IntoResponse {
    // TODO: Extract token from header Authorization: Bearer <token>
    // For now, mock with node_id = 1
    let node_id = 1i64;
    
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
            error!("Config generation failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}
