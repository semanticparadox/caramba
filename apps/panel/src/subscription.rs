use axum::{
    extract::{Path, Query, State, Request},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, warn};

use crate::AppState;
use crate::singbox::subscription_generator::{NodeInfo};

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>, // "clash" | "v2ray" | "singbox"
    pub node_id: Option<i64>,
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    req: Request,
) -> Response {
    // 0. Smart Routing: Redirect if subscription_domain is set and we are not on it
    let sub_domain = state.settings.get_or_default("subscription_domain", "").await;
    if !sub_domain.is_empty() {
        if let Some(host) = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) {
            // Check if we are already on the correct domain to avoid loops
            // We ignore port in host string for comparison if sub_domain doesn't have it
            let host_clean = host.split(':').next().unwrap_or(host);
            let sub_domain_clean = sub_domain.split(':').next().unwrap_or(&sub_domain);
            
            if host_clean != sub_domain_clean {
                let proto = "https"; // Default to https
                let full_url = format!("{}://{}/sub/{}", proto, sub_domain, uuid);
                // Preserve query params if needed, but simple redirect for now
                return axum::response::Redirect::permanent(&full_url).into_response();
            }
        }
    }

    // 0.5 Extract IP and User-Agent for tracking
    let user_agent = req.headers()
        .get(header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
        
    // In a real deployment behind reverse proxy, we need real IP. 
    // For now/local, we might not have it easily without extraction middleware.
    // We'll use a placeholder or best effort.
    let client_ip = "0.0.0.0".to_string(); // TODO: Extract real IP

    // 1. Rate Limit (30 req / min per UUID)
    let rate_key = format!("rate:sub:{}", uuid);
    match state.redis.check_rate_limit(&rate_key, 30, 60).await {
        Ok(allowed) => {
            if !allowed {
                warn!("Rate limit exceeded for subscription {}", uuid);
                return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
            }
        }
        Err(e) => {
             error!("Rate limit check failed: {}", e);
        }
    }

    // 2. Get subscription
    let sub = match state.subscription_service.get_subscription_by_uuid(&uuid).await {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "Subscription not found").into_response();
        }
    };
    
    // 3. Check if active
    if sub.status != "active" {
        return (StatusCode::FORBIDDEN, "Subscription inactive or expired").into_response();
    }
    
    // 4. Update access tracking
    let _ = state.subscription_service.track_access(sub.id, &client_ip, user_agent.as_deref()).await;
    
    // 5. Get user keys
    let user_keys = match state.subscription_service.get_user_keys(&sub).await {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to get user keys for sub {}: {}", uuid, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };
    

    
    // Filter nodes if node_id is provided
    // Note: NodeInfo doesn't strictly have ID, but we mapped it from Node. 
    // Wait, NodeInfo struct in generator.rs does NOT have ID field!
    // We need to check NodeInfo definition again.
    // If it doesn't have ID, we can't filter by ID easily unless we change NodeInfo or filter before conversion.
    // For now, let's skip node filtering or assuming we want all.
    // The previous code filtered `all_nodes` (which were `Node`s from store service) THEN converted.
    
    // Let's re-read subscription_generator.rs to see NodeInfo.
    // It has `name`, `address`, `reality_port`... NO ID.
    // So filtering by `node_id` must happen BEFORE conversion.
    // `get_active_nodes_for_config` returns `NodeInfo`. 
    // I should probably use `store_service.get_active_nodes()` (returns `Node`) 
    // then filter, then convert.
    // OR update `get_active_nodes_for_config` to return `Node` and convert later.
    
    // Let's use `store_service` for fetching nodes (since it's already there and returns `Node`),
    // then filter, then map.
    let nodes_raw = match state.store_service.get_active_nodes().await {
         Ok(nodes) => nodes,
         Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response(),
    };
    
    let filtered_nodes = if let Some(nid) = params.node_id {
        nodes_raw.into_iter().filter(|n| n.id == nid).collect::<Vec<_>>()
    } else {
        nodes_raw
    };
    
    if filtered_nodes.is_empty() {
         return (StatusCode::NOT_FOUND, "Requested server not found").into_response();
    }
    
    let node_infos: Vec<NodeInfo> = filtered_nodes.iter().map(NodeInfo::from).collect();

    // 7. Check Redis Cache & Generate
    let client_type = params.client.as_deref().unwrap_or("singbox");
    let cache_node_id = params.node_id.unwrap_or(0);
    // Include user_uuid in cache key because same sub might yield different configs if we change keys (though keys are tied to sub/user)
    // Actually, uuid is enough.
    let cache_key = format!("sub_config:{}:{}:{}", uuid, client_type, cache_node_id);

    if let Ok(Some(cached_config)) = state.redis.get(&cache_key).await {
         // Return cached...
         // (same logic as before)
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
            match state.subscription_service.generate_clash(&sub, &node_infos, &user_keys) {
                Ok(c) => (c, "application/yaml", "config.yaml"),
                Err(e) => {
                    error!("Clash gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed").into_response();
                }
            }
        }
        "v2ray" => {
             match state.subscription_service.generate_v2ray(&sub, &node_infos, &user_keys) {
                Ok(c) => (c, "text/plain", "config.txt"),
                Err(e) => {
                    error!("V2Ray gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed").into_response();
                }
            }
        }
        _ => {
             match state.subscription_service.generate_singbox(&sub, &node_infos, &user_keys) {
                Ok(c) => (c, "application/json", "config.json"),
                Err(e) => {
                    error!("Singbox gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed").into_response();
                }
            }
        }
    };
    
    // Cache
    let _ = state.redis.set(&cache_key, &content, 300).await; // 5 min cache
    
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, format!("inline; filename={}", filename).as_str()),
        ],
        content
    ).into_response()
}
