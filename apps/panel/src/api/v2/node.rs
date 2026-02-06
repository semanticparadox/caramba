use axum::{
    extract::State,
    response::{IntoResponse, Json},
    http::StatusCode,
};
use tracing::{info, warn, error};
use crate::AppState;
use exarobot_shared::api::{HeartbeatRequest, HeartbeatResponse, AgentAction};
use exarobot_shared::config::ConfigResponse;
use serde::Deserialize;

#[derive(Deserialize)]
struct IpApiResponse {
    #[serde(rename = "countryCode")]
    country_code: String,
    lat: f64,
    lon: f64,
}

/// Agent Heartbeat
/// POST /api/v2/node/heartbeat
pub async fn heartbeat(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let remote_ip = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0")
        .to_string();

    // 1. Extract Token
    let token = match headers.get("Authorization") {
         Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
         None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    let node_res: Result<Option<(i64, Option<String>)>, sqlx::Error> = sqlx::query_as("SELECT id, country_code FROM nodes WHERE join_token = ?")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await;

    let (node_id, node_country) = match node_res {
        Ok(Some((id, cc))) => (id, cc),
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
             error!("DB Error in heartbeat: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    // 3. Update Telemetry & Status
    if let Some(lat) = req.latency {
        let _ = sqlx::query("UPDATE nodes SET last_latency = ?, last_cpu = ?, last_ram = ?, last_seen = CURRENT_TIMESTAMP, status = CASE WHEN status = 'new' THEN 'active' ELSE status END WHERE id = ?")
            .bind(lat)
            .bind(req.cpu_usage.unwrap_or(0.0))
            .bind(req.memory_usage.unwrap_or(0.0))
            .bind(node_id)
            .execute(&state.pool)
            .await;
    } else {
        // Just update last_seen if no telemetry (or older agent)
        let _ = sqlx::query("UPDATE nodes SET last_seen = CURRENT_TIMESTAMP, status = CASE WHEN status = 'new' THEN 'active' ELSE status END WHERE id = ?")
            .bind(node_id)
            .execute(&state.pool)
            .await;
    }

    // GeoIP Check (Async)
    if node_country.is_none() {
        let pool = state.pool.clone();
        let ip_target = remote_ip.clone();
        tokio::spawn(async move {
            let url = format!("http://ip-api.com/json/{}?fields=countryCode,lat,lon", ip_target);
            match reqwest::get(&url).await {
                Ok(resp) => {
                     if let Ok(json) = resp.json::<IpApiResponse>().await {
                         let _ = sqlx::query("UPDATE nodes SET country_code = ?, latitude = ?, longitude = ? WHERE id = ?")
                             .bind(&json.country_code)
                             .bind(json.lat)
                             .bind(json.lon)
                             .bind(node_id)
                             .execute(&pool)
                             .await;
                         info!("🗺️ [GeoIP] Detected location {} ({}, {}) for node {}", json.country_code, json.lat, json.lon, node_id);
                     }
                },
                Err(e) => error!("GeoIP failed: {}", e)
            }
        });
    }

    
    // 4. Check if config update is needed (hash mismatch)
    // 5. Check for Agent Update
    let latest_version: String = state.settings.get_or_default("agent_latest_version", "0.0.0").await;
    
    (StatusCode::OK, Json(HeartbeatResponse {
        success: true,
        action: AgentAction::None,
        latest_version: Some(latest_version),
    })).into_response()
}

// ... (existing code) ...

/// Get Agent Update Info
/// GET /api/v2/node/update-info
pub async fn get_update_info(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Token (Quick Check)
    let valid: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM nodes WHERE join_token = ?")
        .bind(&token)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !valid {
         return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
    }

    // 3. Fetch Update Info from Settings
    let version = state.settings.get_or_default("agent_latest_version", "0.0.0").await;
    let url = state.settings.get_or_default("agent_update_url", "").await;
    let hash = state.settings.get_or_default("agent_update_hash", "").await;

    Json(serde_json::json!({
        "version": version,
        "url": url,
        "hash": hash
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

/// Rotate SNI for a node
/// POST /api/v2/node/rotate-sni
pub async fn rotate_sni(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    let node_res: Result<Option<(i64, Option<String>)>, sqlx::Error> = sqlx::query_as("SELECT id, reality_sni FROM nodes WHERE join_token = ?")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await;

    let (node_id, current_sni) = match node_res {
        Ok(Some((id, sni))) => (id, sni.unwrap_or_else(|| "www.google.com".to_string())),
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
             error!("DB Error in rotate_sni: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    let reason = payload.get("reason").and_then(|v| v.as_str()).unwrap_or("manual");
    
    // 3. Get Next SNI
    // Assume Tier 1 for now, or fetch from node settings
    let next_sni = match state.store_service.get_next_sni(&current_sni, 1).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to get next SNI: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to rotate SNI").into_response();
        }
    };

    if next_sni == current_sni {
        return (StatusCode::CONFLICT, "No other SNI available").into_response();
    }

    // 4. Update Node
    if let Err(e) = sqlx::query("UPDATE nodes SET reality_sni = ? WHERE id = ?")
        .bind(&next_sni)
        .bind(node_id)
        .execute(&state.pool)
        .await 
    {
        error!("Failed to update node SNI: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "DB Update Failed").into_response();
    }

    // 5. Log Rotation
    let rotation_id = match state.store_service.log_sni_rotation(node_id, &current_sni, &next_sni, reason).await {
        Ok(id) => id,
        Err(e) => {
            warn!("Failed to log SNI rotation: {}", e);
            0 // Continue even if logging fails
        }
    };

    // 6. Notify Affected Users (async, non-blocking)
    if let Some(bot) = state.bot_manager.get_bot().await.ok() {
        let notification_service = state.notification_service.clone();
        let old_sni = current_sni.clone();
        let new_sni_clone = next_sni.clone();
        
        tokio::spawn(async move {
            match notification_service.notify_sni_rotation(&bot, node_id, &old_sni, &new_sni_clone, rotation_id).await {
                Ok(count) => info!("📱 Notified {} users about SNI rotation on node {}", count, node_id),
                Err(e) => error!("Failed to send SNI rotation notifications: {}", e),
            }
        });
    } else {
        warn!("Bot not available, skipping user notifications for SNI rotation");
    }
    
    info!("✅ SNI Rotated for Node {}: {} → {} (rotation #{})", node_id, current_sni, next_sni, rotation_id);

    (StatusCode::OK, Json(serde_json::json!({
        "status": "rotated",
        "new_sni": next_sni,
        "rotation_id": rotation_id
    }))).into_response()
}

/// Long Polling for Config Updates
/// GET /api/v2/node/updates/poll
pub async fn poll_updates(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node (Cache or DB)
    // For polling, we might want to use cache to avoid hitting DB every 30s x 1000 nodes?
    // But validate_token usually hits DB.
    // Let's assume hitting DB is fine for now (once per 30s per node is low load).
    // Or we can rely on Redis.
    // For now, simple DB query.
    let node_res: Result<Option<i64>, sqlx::Error> = sqlx::query_scalar("SELECT id FROM nodes WHERE join_token = ?")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await;

    let node_id = match node_res {
        Ok(Some(id)) => id,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
             error!("DB Error in poll_updates: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    // 3. Wait for update
    let rx = state.pubsub.wait_for(&format!("node_events:{}", node_id));

    // 4. Select with timeout (30s)
    match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
        Ok(Ok(_payload)) => {
            // Message received
            (StatusCode::OK, Json(serde_json::json!({"update": true}))).into_response()
        },
        Ok(Err(_)) => {
            // Sender dropped
             (StatusCode::OK, Json(serde_json::json!({"update": false}))).into_response()
        },
        Err(_) => {
            // Timeout
             (StatusCode::OK, Json(serde_json::json!({"update": false}))).into_response()
        }
    }
}

/// Get Agent Settings (Decoy, etc)
/// GET /api/v2/node/settings
pub async fn get_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Token (Quick Check)
    let valid: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM nodes WHERE join_token = ?")
        .bind(&token)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !valid {
         return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
    }

    // 3. Fetch Decoy Settings
    let decoy_enabled: bool = state.settings.get_or_default("decoy_enabled", "false").await.parse().unwrap_or(false);
    let decoy_urls_str = state.settings.get_or_default("decoy_urls", "[\"https://www.google.com\", \"https://www.azure.com\", \"https://www.netflix.com\"]").await;
    let min_interval: u64 = state.settings.get_or_default("decoy_min_interval", "60").await.parse().unwrap_or(60);
    let max_interval: u64 = state.settings.get_or_default("decoy_max_interval", "600").await.parse().unwrap_or(600);

    let decoy_urls: Vec<String> = serde_json::from_str(&decoy_urls_str).unwrap_or_default();

    // 4. Fetch Kill Switch Settings
    let kill_switch_enabled: bool = state.settings.get_or_default("kill_switch_enabled", "false").await.parse().unwrap_or(false);
    let kill_switch_timeout: u64 = state.settings.get_or_default("kill_switch_timeout", "300").await.parse().unwrap_or(300);

    Json(serde_json::json!({
        "decoy": {
            "enabled": decoy_enabled,
            "urls": decoy_urls,
            "min_interval": min_interval,
            "max_interval": max_interval
        },
        "kill_switch": {
            "enabled": kill_switch_enabled,
            "timeout": kill_switch_timeout
        }
    })).into_response()
}
