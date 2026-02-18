use axum::{
    extract::State,
    response::{IntoResponse, Json},
    http::StatusCode,
};
use tracing::{info, warn, error};
use crate::AppState;
use caramba_shared::api::{HeartbeatRequest, HeartbeatResponse, AgentAction};
use caramba_shared::config::ConfigResponse;
use serde::{Deserialize, Serialize};
use chrono::Utc;

#[derive(Deserialize)]
struct IpApiResponse {
    #[serde(rename = "countryCode")]
    country_code: String,
    country: String,
    city: String,
    lat: f64,
    lon: f64,
}

fn country_code_to_flag(code: &str) -> String {
    let code = code.to_uppercase();
    if code.len() != 2 { return "🌐".to_string(); }
    let offset = 127397u32;
    let first = code.chars().next().unwrap() as u32 + offset;
    let second = code.chars().nth(1).unwrap() as u32 + offset;
    format!("{}{}", char::from_u32(first).unwrap_or('🌐'), char::from_u32(second).unwrap_or(' '))
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
    let node_res: Result<Option<(i64, Option<String>, Option<String>)>, sqlx::Error> = sqlx::query_as("SELECT id, country_code, country FROM nodes WHERE join_token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await;

    let (node_id, node_country_code, node_country) = match node_res {
        Ok(Some((id, cc, c))) => (id, cc, c),
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
             error!("DB Error in heartbeat: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    // 3. Update Telemetry & Status & IP & Version
    if let Some(lat) = req.latency {
        // Fix: Also update IP here, because if a node sends stats, we still want to fix its IP if it's pending.
        let _ = sqlx::query("UPDATE nodes SET last_latency = $1, last_cpu = $2, last_ram = $3, current_speed_mbps = $4, max_ram = COALESCE($5, max_ram), cpu_cores = COALESCE($6, cpu_cores), cpu_model = COALESCE($7, cpu_model), active_connections = COALESCE($8, active_connections), last_seen = CURRENT_TIMESTAMP, status = CASE WHEN status = 'disabled' THEN 'disabled' ELSE 'active' END, ip = CASE WHEN ip LIKE 'pending-%' OR ip = '0.0.0.0' THEN $9 ELSE ip END, version = $10 WHERE id = $11")
            .bind(lat)
            .bind(req.cpu_usage.unwrap_or(0.0))
            .bind(req.memory_usage.unwrap_or(0.0))
            .bind(req.speed_mbps.unwrap_or(0))
            .bind(req.max_ram.map(|v| v as i64)) // Cast u64 to i64 (BIGINT)
            .bind(req.cpu_cores)
            .bind(req.cpu_model)
            .bind(req.active_connections.map(|v| v as i32))
            .bind(&remote_ip)
            .bind(&req.version)
            .bind(node_id)
            .execute(&state.pool)
            .await;
    } else {
        // Just update last_seen if no telemetry (or older agent)
        let _ = sqlx::query("UPDATE nodes SET last_seen = CURRENT_TIMESTAMP, status = CASE WHEN status = 'disabled' THEN 'disabled' ELSE 'active' END, ip = CASE WHEN ip LIKE 'pending-%' OR ip = '0.0.0.0' THEN $1 ELSE ip END, version = $2 WHERE id = $3")
            .bind(&remote_ip)
            .bind(&req.version)
            .bind(node_id)
            .execute(&state.pool)
            .await;
    }

    // GeoIP Check (Async) — trigger if country_code OR country/city/flag are missing
    if node_country_code.is_none() || node_country.is_none() {
        let pool = state.pool.clone();
        let ip_target = remote_ip.clone();
        tokio::spawn(async move {
            let url = format!("http://ip-api.com/json/{}?fields=countryCode,country,city,lat,lon", ip_target);
            match reqwest::get(&url).await {
                Ok(resp) => {
                     if let Ok(json) = resp.json::<IpApiResponse>().await {
                         let flag = country_code_to_flag(&json.country_code);
                         let _ = sqlx::query("UPDATE nodes SET country_code = $1, country = $2, city = $3, flag = $4, latitude = $5, longitude = $6 WHERE id = $7")
                             .bind(&json.country_code)
                             .bind(&json.country)
                             .bind(&json.city)
                             .bind(&flag)
                             .bind(json.lat)
                             .bind(json.lon)
                             .bind(node_id)
                             .execute(&pool)
                             .await;
                         info!("🗺️ [GeoIP] Detected location {} {}, {} ({}, {}) for node {}", flag, json.city, json.country, json.lat, json.lon, node_id);
                     }
                },
                Err(e) => error!("GeoIP failed: {}", e)
            }
        });
    }

    
    // 4. Process Per-User Traffic Usage
    if let Some(usage_map) = req.user_usage {
        let mut relay_legacy_usage_bytes: u64 = 0;
        for (tag, bytes) in usage_map {
            if tag.starts_with("user_") {
                if let Ok(sub_id) = tag[5..].parse::<i64>() {
                    // Increment used_traffic and update timestamp
                    let _ = sqlx::query("UPDATE subscriptions SET used_traffic = used_traffic + $1, traffic_updated_at = CURRENT_TIMESTAMP WHERE id = $2")
                        .bind(bytes as i64)
                        .bind(sub_id)
                        .execute(&state.pool)
                        .await;
                }
            }

            if tag.starts_with("relay_") && tag.ends_with("_legacy") {
                relay_legacy_usage_bytes = relay_legacy_usage_bytes.saturating_add(bytes);
            }
        }

        // Record last observed legacy relay traffic, used by relay auth guardrail.
        if relay_legacy_usage_bytes > 0 {
            let _ = state
                .settings
                .set("relay_legacy_usage_last_seen_at", &Utc::now().to_rfc3339())
                .await;
            let _ = state
                .settings
                .set(
                    "relay_legacy_usage_last_seen_bytes",
                    &relay_legacy_usage_bytes.to_string(),
                )
                .await;
        }
    }

    // 6. Process Telemetry (Phase 3)
    // Run in background to not block heartbeat response
    let telemetry_svc = state.telemetry_service.clone();
    let active_conns = req.active_connections;
    let traffic_up = req.traffic_up;
    let traffic_down = req.traffic_down;
    let speed = req.speed_mbps;
    let discoveries = req.discovered_snis;
    let uptime = req.uptime;
    
    tokio::spawn(async move {
        if let Err(e) = telemetry_svc.process_heartbeat(node_id, active_conns, traffic_up, traffic_down, speed, discoveries, uptime).await {
            error!("Telemetry processing failed for node {}: {}", node_id, e);
        }
    });

    // 5. Agent Update Logic (Phase 67)
    let auto_update_agents: bool = state.settings.get_or_default("auto_update_agents", "true").await.parse().unwrap_or(true);
    let latest_version: String = state.settings.get_or_default("agent_latest_version", "0.0.0").await;
    
    let target_version = if auto_update_agents {
        // If auto-update is ON, force latest version
        Some(latest_version)
    } else {
        // If auto-update is OFF, check if specific target version is set for this node
        let stored_target: Option<String> = sqlx::query_scalar("SELECT target_version FROM nodes WHERE id = $1")
            .bind(node_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);
        
        stored_target
    };

    // 6. Action Trigger (Log Collection, etc.)
    let mut action = AgentAction::None;
    let pending_logs: bool = sqlx::query_scalar("SELECT pending_log_collection FROM nodes WHERE id = $1")
        .bind(node_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if pending_logs {
        action = AgentAction::CollectLogs;
    }

    (StatusCode::OK, Json(HeartbeatResponse {
        success: true,
        action,
        latest_version: target_version,
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
    let valid: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM nodes WHERE join_token = $1")
        .bind(&token)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !valid {
         return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
    }

    // 3. Fetch Update Info from Settings
    let version = state.settings.get_or_default("agent_latest_version", "0.0.0").await;
    let mut url = state.settings.get_or_default("agent_update_url", "").await;
    let hash = state.settings.get_or_default("agent_update_hash", "").await;

    // Resolve relative URL
    if url.starts_with('/') {
        if let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) {
            let protocol = if host.contains("localhost") || host.contains("127.0.0.1") { "http" } else { "https" };
            url = format!("{}://{}{}", protocol, host, url);
        }
    }

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
    let node_res: Result<Option<(i64, bool)>, sqlx::Error> = sqlx::query_as("SELECT id, is_enabled FROM nodes WHERE join_token = $1")
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
            let config_str: String = config_value.to_string();
            let hash = format!("{:x}", md5::compute(config_str.as_bytes()));
            
            // Update last_synced_at
            let _ = sqlx::query("UPDATE nodes SET last_synced_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(node_id_scalar)
                .execute(&state.pool)
                .await;

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
    let node_res: Result<Option<(i64, Option<String>)>, sqlx::Error> = sqlx::query_as("SELECT id, reality_sni FROM nodes WHERE join_token = $1")
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
    let next_sni: String = match state.security_service.get_next_sni(&current_sni, 1, false).await {
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
    if let Err(e) = sqlx::query("UPDATE nodes SET reality_sni = $1 WHERE id = $2")
        .bind(&next_sni)
        .bind(node_id)
        .execute(&state.pool)
        .await 
    {
        error!("Failed to update node SNI: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "DB Update Failed").into_response();
    }

    // 5. Log Rotation
    let rotation_id = match state.security_service.log_sni_rotation(node_id, &current_sni, &next_sni, reason).await {
        Ok(log) => log.id,
        Err(e) => {
            warn!("Failed to log SNI rotation: {}", e);
            0 // Continue even if logging fails
        }
    };

    // 6. Notify Affected Users (async, non-blocking)
    if let Some(bot) = state.bot_manager.get_bot().await.ok().map(|b| b as teloxide::Bot) {
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
    let node_res: Result<Option<i64>, sqlx::Error> = sqlx::query_scalar("SELECT id FROM nodes WHERE join_token = $1")
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
    let valid: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM nodes WHERE join_token = $1")
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

#[derive(Deserialize)]
pub struct RegisterNodeRequest {
    pub enrollment_key: String,
    pub hostname: String,
    pub ip: Option<String>,
}

#[derive(Serialize)]
pub struct RegisterNodeResponse {
    pub node_id: i64,
    pub join_token: String,
}

/// Register a new node using an Enrollment Key
/// POST /api/v2/node/register
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterNodeRequest>,
) -> impl IntoResponse {
    // 1. Validate API Key
    let api_key_res: Result<Option<caramba_db::models::api_key::ApiKey>, _> = sqlx::query_as("SELECT * FROM api_keys WHERE key = $1 AND is_active = TRUE")
    .bind(&payload.enrollment_key)
    .fetch_optional(&state.pool)
    .await;

    let api_key = match api_key_res {
        Ok(Some(k)) => k,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid API Key").into_response(),
        Err(e) => {
             error!("DB Error checking API key: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    if api_key.key_type != "enrollment" {
        return (StatusCode::FORBIDDEN, "Invalid Key Type").into_response();
    }

    if let Some(max) = api_key.max_uses {
        if api_key.current_uses >= max {
             return (StatusCode::FORBIDDEN, "Key Usage Limit Reached").into_response();
        }
    }

    // 2. Increment Usage
    let _ = sqlx::query("UPDATE api_keys SET current_uses = current_uses + 1 WHERE id = $1")
        .bind(api_key.id)
        .execute(&state.pool)
        .await;

    // 3. Create Node
    let join_token = uuid::Uuid::new_v4().to_string();
    // Default to pending IP to ensure it's updated later. OR use 0.0.0.0.
    // Use "pending-" prefix so our heartbeat logic picks it up!
    let ip = payload.ip.unwrap_or_else(|| format!("pending-{}", &join_token[0..8])); 

    let node_id_res = sqlx::query("INSERT INTO nodes (name, ip, join_token, status, is_enabled) VALUES ($1, $2, $3, 'new', TRUE) RETURNING id")
        .bind(&payload.hostname)
        .bind(&ip)
        .bind(&join_token)
        .fetch_one(&state.pool)
        .await;

     match node_id_res {
        Ok(row) => {
             use sqlx::Row;
             let node_id: i64 = row.get("id");
             info!("✅ Node registered via API Key {}: {} (ID: {})", api_key.name, payload.hostname, node_id);
             
             (StatusCode::OK, Json(RegisterNodeResponse {
                 node_id,
                 join_token,
             })).into_response()
        },
        Err(e) => {
            error!("Failed to create node: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create node").into_response()
        }
    }
}

/// Report Logs from Agent
/// POST /api/v2/node/logs
pub async fn report_node_logs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<caramba_shared::api::LogResponse>,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    let node_id: i64 = match sqlx::query_scalar("SELECT id FROM nodes WHERE join_token = $1")
        .bind(&token)
        .fetch_one(&state.pool)
        .await 
    {
        Ok(id) => id,
        Err(_) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
    };

    // 3. Store Logs (In Redis for quick retrieval, and reset pending flag)
    let logs_json = serde_json::to_string(&req.logs).unwrap_or_default();
    let _ = state.redis.set(&format!("node_logs:{}", node_id), &logs_json, 300).await; // Store for 5 mins

    let _ = sqlx::query("UPDATE nodes SET pending_log_collection = FALSE WHERE id = $1")
        .bind(node_id)
        .execute(&state.pool)
        .await;

    // 4. Notify UI via PubSub
    let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "logs_ready").await;

    info!("✅ Logs received and stored for node {}", node_id);
    StatusCode::OK.into_response()
}
