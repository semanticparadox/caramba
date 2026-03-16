use crate::singbox_generator::ConfigGenerator;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, info};

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>, // only "singbox" is supported
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let client_type = params
        .client
        .as_deref()
        .unwrap_or("singbox")
        .to_ascii_lowercase();
    if client_type != "singbox" {
        return (
            StatusCode::BAD_REQUEST,
            "Only sing-box subscriptions are supported",
        )
            .into_response();
    }

    let client_ip = get_client_ip(&headers).unwrap_or_else(|| "0.0.0.0".to_string());
    info!(
        "Subscription request: UUID={}, client_ip={}",
        uuid, client_ip
    );

    let sub = match state.panel_client.get_subscription(&uuid).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to fetch subscription: {}", e);
            return (StatusCode::NOT_FOUND, "Subscription not found").into_response();
        }
    };

    if sub.status != "active" {
        return (StatusCode::FORBIDDEN, "Subscription inactive").into_response();
    }

    let mut exit_nodes = match state.panel_client.get_active_exit_nodes().await {
        Ok(nodes) => nodes,
        Err(e) => {
            error!("Failed to fetch active exit nodes: {}", e);
            return (StatusCode::SERVICE_UNAVAILABLE, "No exit nodes available").into_response();
        }
    };

    if let Some(exit_id) = sub.node_id {
        exit_nodes.retain(|n| n.node.id == exit_id);
        if exit_nodes.is_empty() {
            return (
                StatusCode::NOT_FOUND,
                "Requested exit node is unavailable for this subscription",
            )
                .into_response();
        }
    }

    let relay_nodes = match state.panel_client.get_active_relay_nodes().await {
        Ok(nodes) => nodes,
        Err(e) => {
            error!("Failed to fetch active relay nodes: {}", e);
            return (StatusCode::SERVICE_UNAVAILABLE, "No relay nodes available").into_response();
        }
    };

    let mut nodes = Vec::with_capacity(exit_nodes.len() + relay_nodes.len());
    nodes.extend(exit_nodes);
    nodes.extend(relay_nodes);

    if nodes.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "No nodes available").into_response();
    }

    let user_keys = match state.panel_client.get_user_keys(sub.user_id).await {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to fetch user keys: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Key error").into_response();
        }
    };

    let config_json =
        match ConfigGenerator::generate(nodes, &sub.subscription_uuid, &user_keys, sub.node_id) {
            Ok(cfg) => cfg,
            Err(e) => {
                error!("Failed to generate strict sing-box profile: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to build sing-box profile",
                )
                    .into_response();
            }
        };

    let content = serde_json::to_string_pretty(&config_json).unwrap_or_default();
    let total_traffic_bytes = (sub.traffic_limit_gb as i64) * 1024 * 1024 * 1024;
    let user_info_header = format!(
        "upload=0; download={}; total={}; expire={}",
        sub.used_traffic, total_traffic_bytes, sub.expire_timestamp
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header("profile-title", &sub.plan_name)
        .header("profile-update-interval", "2")
        .header("Subscription-Userinfo", &user_info_header)
        .body(content)
        .unwrap()
        .into_response()
}

fn get_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(ip) = headers.get("cf-connecting-ip") {
        return ip.to_str().ok().map(|s| s.to_string());
    }
    if let Some(ip) = headers.get("x-forwarded-for") {
        return ip
            .to_str()
            .ok()
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string());
    }
    None
}
