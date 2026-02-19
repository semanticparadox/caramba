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
    pub client: Option<String>, // "clash" | "v2ray" | "singbox"
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // 0. Extract Client IP
    let client_ip = get_client_ip(&headers).unwrap_or("0.0.0.0".to_string());

    // 1. GeoIP Lookup
    let geo_data = state.geo_service.get_location(&client_ip).await;
    let country_code = geo_data
        .as_ref()
        .map(|d| d.country_code.as_str())
        .unwrap_or("XX");

    info!(
        "Subscription request: UUID={}, client={:?}, IP={}, Country={}",
        uuid, params.client, client_ip, country_code
    );

    // 2. Get Subscription
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

    // 3. Get Nodes (InternalNode structure)
    let nodes = match state.panel_client.get_active_nodes().await {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to fetch nodes: {}", e);
            return (StatusCode::SERVICE_UNAVAILABLE, "No nodes available").into_response();
        }
    };

    // 4. Get User Keys
    let user_keys = match state.panel_client.get_user_keys(sub.user_id).await {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to fetch user keys: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Key error").into_response();
        }
    };

    // 5. Generate Config
    let client_type = params.client.as_deref().unwrap_or("singbox");

    let (content, content_type, filename) = match client_type {
        "clash" => {
            // Legacy Clash Gen
            let simple_nodes: Vec<&crate::panel_client::Node> =
                nodes.iter().map(|n| &n.node).collect();
            match generate_clash_config(&simple_nodes, &user_keys) {
                Ok(c) => (c, "application/yaml", "config.yaml"),
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            }
        }
        "v2ray" => {
            // Legacy V2Ray Gen
            let simple_nodes: Vec<&crate::panel_client::Node> =
                nodes.iter().map(|n| &n.node).collect();
            match generate_v2ray_config(&simple_nodes, &user_keys) {
                Ok(c) => (c, "text/plain", "config.txt"),
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            }
        }
        _ => {
            // Smart Sing-box Gen
            // Pass the FULL nodes (with inbounds) and the detected region
            let config_json = ConfigGenerator::generate(nodes, &user_keys, country_code);
            let json_str = serde_json::to_string_pretty(&config_json).unwrap_or_default();
            (json_str, "application/json", "config.json")
        }
    };

    // 6. Return with proper headers
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .header("X-Profile-Update-Interval", "24")
        .header(
            "Subscription-Userinfo",
            format!("upload=0; download={}; total=0; expire=0", sub.used_traffic),
        )
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

// Config generators (simplified versions from main panel)
fn generate_clash_config(
    nodes: &[&crate::panel_client::Node],
    keys: &crate::panel_client::UserKeys,
) -> anyhow::Result<String> {
    use serde_json::json;

    let mut proxies = Vec::new();

    for node in nodes {
        proxies.push(json!({
            "name": format!("{} VLESS", node.name),
            "type": "vless",
            "server": node.ip,
            "port": node.vpn_port,
            "uuid": keys.user_uuid,
            "network": "tcp",
            "tls": true,
            "servername": node.domain.as_ref().unwrap_or(&"www.google.com".to_string()),
            "reality-opts": {
                "public-key": node.reality_pub.as_ref().unwrap_or(&"".to_string()),
                "short-id": node.short_id.as_ref().unwrap_or(&"".to_string())
            },
            "client-fingerprint": "chrome"
        }));
    }

    let proxy_names: Vec<String> = proxies
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();

    let config = json!({
        "proxies": proxies,
        "proxy-groups": [{
            "name": "CARAMBA",
            "type": "select",
            "proxies": proxy_names
        }],
        "rules": ["MATCH,CARAMBA"]
    });

    Ok(serde_yaml::to_string(&config)?)
}

fn generate_v2ray_config(
    nodes: &[&crate::panel_client::Node],
    keys: &crate::panel_client::UserKeys,
) -> anyhow::Result<String> {
    let mut links = Vec::new();

    for node in nodes {
        let vless_link = format!(
            "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
            keys.user_uuid,
            node.ip,
            node.vpn_port,
            node.domain.as_ref().unwrap_or(&"www.google.com".to_string()),
            node.reality_pub.as_ref().unwrap_or(&"".to_string()),
            node.short_id.as_ref().unwrap_or(&"".to_string()),
            urlencoding::encode(&format!("{} VLESS", node.name))
        );
        links.push(vless_link);
    }

    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(links.join("\n")))
}
