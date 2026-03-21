use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, info};

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>,
    pub relay_country: Option<String>,
}

/// Detect client type from User-Agent header when ?client= is not specified.
fn detect_client_from_ua(headers: &HeaderMap) -> &'static str {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Check sing-box clients FIRST — Hiddify UA contains "ClashMeta" and "v2ray"
    // e.g. "HiddifyNext/4.0.0 (ios) like ClashMeta v2ray sing-box"
    if ua.contains("hiddify") || ua.contains("sing-box") || ua.contains("sfi") || ua.contains("sfa") {
        "singbox"
    } else if ua.contains("clash") || ua.contains("stash") || ua.contains("mihomo") {
        "clash"
    } else if ua.contains("shadowrocket")
        || ua.contains("v2rayn")
        || ua.contains("v2rayng")
        || ua.contains("streisand")
        || ua.contains("fair")
        || ua.contains("nekoray")
        || ua.contains("happ")
    {
        "v2ray"
    } else {
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
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    info!(
        "Subscription request: UUID={}, client={}, ip={}, ua={:?}",
        uuid, client_type, client_ip, user_agent
    );

    // All formats proxied to panel — panel has the authoritative generators
    // for sing-box (geo-based auto-relay, proper naming), v2ray, and clash.
    // Forward client IP and User-Agent so panel can do device tracking and geo filtering.
    proxy_to_panel(&state, &uuid, client_type, &client_ip, user_agent.as_deref(), params.relay_country.as_deref()).await
}

/// Proxy subscription requests to the panel, which has the authoritative
/// generators for all formats (sing-box, v2ray, clash).
async fn proxy_to_panel(
    state: &AppState,
    uuid: &str,
    client_type: &str,
    client_ip: &str,
    user_agent: Option<&str>,
    relay_country: Option<&str>,
) -> Response {
    let mut panel_sub_url = format!(
        "{}/sub/{}?client={}",
        state.config.panel_url, uuid, client_type
    );
    if let Some(rc) = relay_country {
        panel_sub_url.push_str(&format!("&relay_country={}", rc));
    }

    let resp = match state
        .panel_client
        .proxy_subscription(&panel_sub_url, &state.config.domain, client_ip, user_agent)
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

    if status.is_redirection() {
        let location = resp.headers().get("location").and_then(|v| v.to_str().ok());
        error!(
            "Panel returned redirect {} -> {:?} (Host header may not match subscription_domain)",
            status, location
        );
        return (
            StatusCode::BAD_GATEWAY,
            "Panel subscription redirect loop",
        )
            .into_response();
    }

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
