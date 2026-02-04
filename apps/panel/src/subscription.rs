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
    NodeInfo,
};

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>, // "clash" | "v2ray" | "singbox"
    pub node_id: Option<i64>,
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>
) -> Response {
    // 0. Rate Limit (30 req / min per UUID)
    let rate_key = format!("rate:sub:{}", uuid);
    match state.redis.check_rate_limit(&rate_key, 30, 60).await {
        Ok(allowed) => {
            if !allowed {
                info!("Rate limit exceeded for subscription {}", uuid);
                return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
            }
        }
        Err(e) => {
             error!("Rate limit check failed: {}", e);
             // Fail open or closed? Fail open to not block users if Redis hiccups, but log error.
        }
    }

    info!("Subscription request: UUID={}, client={:?}, node_id={:?}", uuid, params.client, params.node_id);
    
    // 1. Get subscription by UUID
    let sub = match sqlx::query_as::<_, crate::models::store::Subscription>(
        "SELECT * FROM subscriptions WHERE subscription_uuid = ?"
    )
    .bind(&uuid)
    .fetch_optional(&state.pool)
    .await
    {
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
            _awg_private_key: None, // Client generates this
        },
        _ => {
            error!("User keys not found for user_id={}", sub.user_id);
            return (StatusCode::INTERNAL_SERVER_ERROR, "User not found").into_response()
        }
    };
    
    // 5. Get active nodes
    let all_nodes = match state.store_service.get_active_nodes().await {
        Ok(nodes) if !nodes.is_empty() => nodes,
        _ => {
            error!("No active nodes available");
            return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response()
        }
    };
    
    // Filter nodes if node_id is provided
    let nodes: Vec<_> = if let Some(node_id) = params.node_id {
        all_nodes.into_iter().filter(|n| n.id == node_id).collect()
    } else {
        all_nodes
    };
    
    if nodes.is_empty() {
        return (StatusCode::NOT_FOUND, "Requested server not found or inactive").into_response();
    }
    
    // Convert to NodeInfo
    let node_infos: Vec<NodeInfo> = nodes.iter().map(NodeInfo::from).collect();
    
    // 6. Check Redis Cache & Generate
    let client_type = params.client.as_deref().unwrap_or("singbox");
    let cache_key = format!("sub_config:{}:{}:{}", uuid, client_type, params.node_id.unwrap_or(0));

    if let Ok(Some(cached_config)) = state.redis.get(&cache_key).await {
        info!("Hit Redis cache for subscription {}", uuid);
        let filename = match client_type {
            "clash" => "config.yaml",
            "v2ray" => "config.txt",
            _ => "config.json",
        };
        let content_type = match client_type {
            "clash" => "application/yaml",
            "v2ray" => "text/plain",
            _ => "application/json",
        };
        
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONTENT_DISPOSITION, format!("inline; filename={}", filename).as_str()),
            ],
            cached_config
        ).into_response();
    }
    let (content, content_type, filename) = match client_type {
        "clash" => {
            match generate_clash_config(&sub, &node_infos, &user_keys) {
                Ok(yaml) => (yaml, "application/yaml", "config.yaml"),
                Err(e) => {
                    error!("Failed to generate Clash config: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Config generation failed").into_response()
                }
            }
        }
        "v2ray" => {
            match generate_v2ray_config(&sub, &node_infos, &user_keys) {
                Ok(b64) => (b64, "text/plain", "config.txt"),
                Err(e) => {
                    error!("Failed to generate V2Ray config: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Config generation failed").into_response()
                }
            }
        }
        "singbox" | _ => {
            match generate_singbox_config(&sub, &node_infos, &user_keys) {
                Ok(json) => (json, "application/json", "config.json"),
                Err(e) => {
                    error!("Failed to generate Sing-box config: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Config generation failed").into_response()
                }
            }
        }
    };
    
    info!("Generated {} config for subscription {} ({} bytes)", client_type, uuid, content.len());
    
    // Cache the result
    if let Err(e) = state.redis.set(&cache_key, &content, 3600).await {
        error!("Failed to cache config in Redis: {}", e);
    }
    
    // 7. Return with proper headers
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, format!("inline; filename={}", filename).as_str()),
        ],
        content
    ).into_response()
}
