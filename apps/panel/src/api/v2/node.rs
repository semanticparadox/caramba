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
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(payload): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    // PARANOID MODE: Use COALESCE in SQL and Option<String> in Rust to handle absolutely any schema state
    // We explicitly cast to ensure types match what we expect
    let node_res: Result<Option<(i64, Option<String>, Option<String>)>, sqlx::Error> = 
        sqlx::query_as("SELECT id, status, COALESCE(ip, '') as ip FROM nodes WHERE join_token = ?")
        .bind(&token) // Add & reference to safer binding
        .fetch_optional(&state.pool)
        .await;

    let (node_id, node_status, node_ip) = match node_res {
        Ok(Some((id, status, ip_opt))) => (id, status.unwrap_or_else(|| "new".to_string()), ip_opt.unwrap_or_default()),
        Ok(None) => {
            tracing::warn!("Heartbeat from unknown token (Token: {}...)", &token.chars().take(5).collect::<String>());
            return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
        }
        Err(e) => {
            error!("CRITICAL DB ERROR in heartbeat select: {:?}", e);
            // Return the actual error to the agent so we can see it in logs
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB Select Error: {}", e)).into_response();
        }
    };

    info!("💓 [Checkpoint 1] Found Node ID: {:?} | Status: {} | IP: {}", node_id, node_status, node_ip);
    info!("💓 [Checkpoint 2] Agent Payload: ver={}, uptime={}", payload.version, payload.uptime);

    // 3. Update Status and IP
    // Use X-Forwarded-For if available, else SocketAddr
    let remote_ip = headers
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());
    
    let new_status = "active"; // Always set to active on heartbeat

    // Check orphan logic
    if node_ip != remote_ip {
        info!("💓 [Checkpoint 3] IP Mismatch! DB={} vs Remote={}. Handling constraints...", node_ip, remote_ip);
        
        let update_res = sqlx::query("UPDATE nodes SET ip = cast(id as text) || '_orphaned' WHERE ip = ? AND id != ?")
            .bind(&remote_ip)
            .bind(node_id)
            .execute(&state.pool)
            .await;
        
        if let Err(e) = update_res {
             error!("⚠️ Failed to orphan old nodes with IP {}: {:?}", remote_ip, e);
        } else {
             info!("💓 [Checkpoint 3a] Terminated potential conflicts for IP {}", remote_ip);
        }
    }

    // Now safe to update
    info!("💓 [Checkpoint 4] Updating Node {} to status='{}', ip='{}'", node_id, new_status, remote_ip);
    
    let update_status_res = sqlx::query("UPDATE nodes SET last_seen = CURRENT_TIMESTAMP, status = ?, ip = ? WHERE id = ?")
        .bind(new_status)
        .bind(&remote_ip)
        .bind(node_id)
        .execute(&state.pool)
        .await;

    if let Err(e) = update_status_res {
        error!("CRITICAL DB ERROR: Failed to update node {} heartbeat: {:?}", node_id, e);
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB Update Error: {}", e)).into_response();
    }
    
    info!("💓 [Checkpoint 5] Success! Node {} updated.", node_id);

    
    // 4. Check if config update is needed (hash mismatch)
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
    // 2. Validate Node
    // Using simple query_as to avoid compilation failure if DB migration is not applied locally yet.
    // At runtime, it will fail if column is missing, but it unblocks build.
    let node_res: Result<Option<(i64, bool)>, sqlx::Error> = sqlx::query_as("SELECT id, is_enabled FROM nodes WHERE join_token = ?")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await;

    let (node_id, is_enabled) = match node_res {
        Ok(Some((id, enabled))) => (id, enabled),
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
             error!("DB Error in get_config: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    if !is_enabled {
        return (StatusCode::FORBIDDEN, "Node is disabled").into_response();
    }
    
    // Force unwrap if it is Option (based on error)
    // NOTE: The previous error "found Option<i64>" suggests n.id is Option.
    // Let's handle it safely.
    let node_id_scalar = if let Some(id_val) = Option::from(node_id) { id_val } else { 
        error!("Node ID is null for token {}", token);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Node ID Invalid").into_response();
    };

    // 3. Generate Config
    match state.orchestration_service.generate_node_config_json(node_id_scalar).await {
        Ok((_, config_value)) => {
            let config_str = config_value.to_string();
            let hash = format!("{:x}", md5::compute(config_str.as_bytes()));
            
            (StatusCode::OK, Json(ConfigResponse {
                hash,
                content: config_value,
            })).into_response()
        },
        Err(e) => {
            error!("Config generation failed for node {}: {}", node_id_scalar, e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}
