use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{info, error};
use crate::AppState;
use crate::singbox::subscription_generator::{
    generate_clash_config,
    generate_v2ray_config,
    generate_singbox_config,
    UserKeys,
};

#[derive(Deserialize)]
pub struct SubParams {
    client: Option<String>, // "clash" | "v2ray" | "singbox"
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    headers: header::HeaderMap,
) -> Response {
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string());

    info!("Subscription request: UUID={}, IP={}, client={:?}", uuid, client_ip, params.client);
    
    // 1. Get subscription by UUID
    let sub = match state.store_service.get_subscription_by_uuid(&uuid).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            info!("Subscription not found: {}", uuid);
            return (StatusCode::NOT_FOUND, "Subscription not found").into_response()
        }
        Err(e) => {
            error!("DB error fetching subscription: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
        }
    };
    
    // 2. Check if active
    if sub.status != "active" {
        info!("Subscription {} is inactive (status: {})", uuid, sub.status);
        return (StatusCode::FORBIDDEN, "Subscription inactive or expired").into_response();
    }
    
    // 3. Update last_sub_access
    let _ = sqlx::query("UPDATE subscriptions SET last_sub_access = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(sub.id)
        .execute(&state.pool)
        .await;
    
    // 4. Get user keys
    let user_keys = match sqlx::query_as::<_, (String, String)>(
        "SELECT user_uuid, hy2_password FROM users WHERE id = ?"
    )
    .bind(sub.user_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some((user_uuid, hy2_password))) => UserKeys {
            user_uuid,
            hy2_password,
            awg_private_key: None, // Client generates this
        },
        _ => {
            error!("User keys not found for user_id={}", sub.user_id);
            return (StatusCode::INTERNAL_SERVER_ERROR, "User not found").into_response()
        }
    };
    
    // 5. Get and Filter Active Nodes (Smart Routing)
    let all_nodes = match state.store_service.get_active_nodes().await {
        Ok(nodes) if !nodes.is_empty() => nodes,
        _ => {
            error!("No active nodes available");
            return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response()
        }
    };

    // Geo-Aware Filtering
    let client_loc = state.geo_service.get_location(&client_ip).await;
    let client_country = client_loc.as_ref().map(|l| l.country_code.as_str()).unwrap_or("XX");
    
    let nodes: Vec<_> = if client_country == "RU" {
        // User in Russia: Prefer RU nodes or Relays
        let filtered: Vec<_> = all_nodes.iter().filter(|n| {
            n.country_code.as_deref() == Some("RU") || n.is_relay
        }).cloned().collect();
        
        if filtered.is_empty() {
             info!("User in RU but no RU/Relay nodes found. Fallback to all nodes.");
             all_nodes
        } else {
             filtered
        }
    } else {
        // User World: Prefer Non-RU nodes
        let filtered: Vec<_> = all_nodes.iter().filter(|n| {
            n.country_code.as_deref() != Some("RU")
        }).cloned().collect();

        if filtered.is_empty() {
            info!("User in World but no Non-RU nodes found. Fallback to all nodes.");
            all_nodes
        } else {
            filtered
        }
    };
    
    // 6. Generate config based on client type
    let client_type = params.client.as_deref().unwrap_or("singbox");
    let (content, content_type, filename) = match client_type {
        "clash" => {
            match generate_clash_config(&sub, &nodes, &user_keys) {
                Ok(yaml) => (yaml, "application/yaml", "config.yaml"),
                Err(e) => {
                    error!("Failed to generate Clash config: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Config generation failed").into_response()
                }
            }
        }
        "v2ray" => {
            match generate_v2ray_config(&sub, &nodes, &user_keys) {
                Ok(b64) => (b64, "text/plain", "config.txt"),
                Err(e) => {
                    error!("Failed to generate V2Ray config: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Config generation failed").into_response()
                }
            }
        }
        "singbox" | _ => {
            match generate_singbox_config(&sub, &nodes, &user_keys) {
                Ok(json) => (json, "application/json", "config.json"),
                Err(e) => {
                    error!("Failed to generate Sing-box config: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Config generation failed").into_response()
                }
            }
        }
    };
    
    info!("Generated {} config for subscription {} ({} bytes)", client_type, uuid, content.len());
    
    // 7. Return with proper headers
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, &format!("inline; filename={}", filename)),
            ("profile-update-interval", "24"), // Update every 24 hours
            ("subscription-userinfo", &format!("upload=0; download={}; total={}", 
                sub.used_traffic, 
                10 * 1024 * 1024 * 1024_i64 // TODO: Get actual traffic limit from plan
            )),
        ],
        content
    ).into_response()
}
