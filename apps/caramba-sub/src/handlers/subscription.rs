use crate::AppState;
use crate::singbox_generator::ConfigGenerator;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, info, warn};

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>,
}

/// Detect client type from User-Agent header when ?client= is not specified.
fn detect_client_from_ua(headers: &HeaderMap) -> &'static str {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ua.contains("clash") || ua.contains("stash") || ua.contains("mihomo") {
        "clash"
    } else if ua.contains("shadowrocket")
        || ua.contains("v2rayn")
        || ua.contains("v2rayng")
        || ua.contains("streisand")
        || ua.contains("fair")
        || ua.contains("nekoray")
    {
        "v2ray"
    } else {
        // Default: sing-box format (works for Hiddify, SFI, SFA, NekoBox)
        "singbox"
    }
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let client_type = match params.client.as_deref() {
        Some(c) => match c.to_ascii_lowercase().as_str() {
            "hiddify" | "singbox" | "sing-box" | "sfi" | "sfa" | "nekobox" => "singbox",
            "clash" | "clashmeta" | "mihomo" | "stash" => "clash",
            "v2ray" | "v2rayn" | "v2rayng" | "xray" | "shadowrocket" | "streisand" => "v2ray",
            _ => detect_client_from_ua(&headers),
        },
        None => detect_client_from_ua(&headers),
    };

    let client_ip = get_client_ip(&headers).unwrap_or_else(|| "0.0.0.0".to_string());
    info!(
        "Subscription request: UUID={}, client={}, ip={}",
        uuid, client_type, client_ip
    );

    // For V2Ray and Clash formats, proxy to panel which has full generators
    if client_type == "v2ray" || client_type == "clash" {
        return proxy_to_panel(&state, &uuid, client_type).await;
    }

    // Sing-box format — generate locally with strict variant control
    // Retry once on connection errors (reqwest may reuse a stale connection)
    let sub = match state.panel_client.get_subscription(&uuid).await {
        Ok(s) => s,
        Err(e) => {
            warn!("First attempt failed for {}: {}. Retrying...", uuid, e);
            match state.panel_client.get_subscription(&uuid).await {
                Ok(s) => s,
                Err(e2) => {
                    error!("Failed to fetch subscription after retry: {}", e2);
                    return (StatusCode::NOT_FOUND, "Subscription not found").into_response();
                }
            }
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

    let mut preferred_exit_id = sub.node_id;
    if let Some(exit_id) = preferred_exit_id {
        let exit_available = exit_nodes.iter().any(|n| n.node.id == exit_id);
        if exit_available {
            exit_nodes.retain(|n| n.node.id == exit_id);
        } else {
            warn!(
                "Requested exit node {} is unavailable for subscription {}. Falling back to default active exit node.",
                exit_id, sub.subscription_uuid
            );
            preferred_exit_id = None;
        }
    }

    if exit_nodes.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "No exit nodes available").into_response();
    }

    let relay_nodes = match state.panel_client.get_active_relay_nodes().await {
        Ok(nodes) => nodes,
        Err(e) => {
            warn!(
                "Failed to fetch active relay nodes: {}. Falling back to direct-only variants.",
                e
            );
            Vec::new()
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

    let config_json = match ConfigGenerator::generate(
        nodes,
        &sub.subscription_uuid,
        &user_keys,
        preferred_exit_id,
    ) {
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
    let user_info_header = if sub.traffic_limit_gb == 0 {
        format!("upload=0; download={}; expire={}", sub.used_traffic, sub.expire_timestamp)
    } else {
        let total_traffic_bytes = (sub.traffic_limit_gb as i64) * 1024 * 1024 * 1024;
        format!(
            "upload=0; download={}; total={}; expire={}",
            sub.used_traffic, total_traffic_bytes, sub.expire_timestamp
        )
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
        .header(header::PRAGMA, "no-cache")
        .header("profile-title", &sub.plan_name)
        .header("profile-update-interval", "2")
        .header("Subscription-Userinfo", &user_info_header)
        .body(content)
        .unwrap()
        .into_response()
}

/// Proxy subscription requests for V2Ray/Clash formats to the panel,
/// which has full generators for these formats.
async fn proxy_to_panel(state: &AppState, uuid: &str, client_type: &str) -> Response {
    let panel_sub_url = format!(
        "{}/api/internal/subscription/{}?client={}",
        state.config.panel_url, uuid, client_type
    );

    let resp = match state
        .panel_client
        .raw_get(&panel_sub_url)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to proxy subscription to panel: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                "Panel subscription endpoint unavailable",
            )
                .into_response();
        }
    };

    let status = resp.status();
    let mut builder = Response::builder().status(status.as_u16());

    // Forward relevant headers from panel response
    for key in &[
        "content-type",
        "profile-title",
        "profile-update-interval",
        "subscription-userinfo",
    ] {
        if let Some(val) = resp.headers().get(*key) {
            if let Ok(v) = val.to_str() {
                builder = builder.header(*key, v);
            }
        }
    }

    builder = builder
        .header(header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
        .header(header::PRAGMA, "no-cache");

    let body = resp.text().await.unwrap_or_default();
    builder.body(body).unwrap().into_response()
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
