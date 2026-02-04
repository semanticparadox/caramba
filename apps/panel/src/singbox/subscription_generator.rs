use crate::models::store::Subscription;
use anyhow::Result;
use serde_json::json;

/// User keys for generating client configs
pub struct UserKeys {
    pub user_uuid: String,
    pub hy2_password: String,
    pub _awg_private_key: Option<String>,
}

/// Simplified node struct for subscription generation
#[derive(Clone)]
pub struct NodeInfo {
    pub name: String,
    pub address: String,
    pub reality_port: Option<i32>,
    pub reality_sni: Option<String>,
    pub reality_public_key: Option<String>,
    pub reality_short_id: Option<String>,
    pub hy2_port: Option<i32>,
    pub hy2_sni: Option<String>,
}

// Convert from actual Node model
impl From<&crate::models::node::Node> for NodeInfo {
    fn from(node: &crate::models::node::Node) -> Self {
        Self {
            name: node.name.clone(),
            address: node.ip.clone(), // IP field is the address
            reality_port: Some(node.vpn_port as i32), // vpn_port is the Reality port
            reality_sni: node.domain.clone(), // Domain is used as SNI
            reality_public_key: node.reality_pub.clone(),
            reality_short_id: node.short_id.clone(),
            // TODO: Add HY2 fields when they're added to Node model
            hy2_port: None,
            hy2_sni: None,
        }
    }
}

/// Generate Clash YAML config
pub fn generate_clash_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    let mut proxies = Vec::new();
    
    for node in nodes {
        // VLESS + Reality
        if node.reality_port.is_some() {
            proxies.push(json!({
                "name": format!("{} VLESS", node.name),
                "type": "vless",
                "server": node.address,
                "port": node.reality_port.unwrap(),
                "uuid": user_keys.user_uuid,
                "network": "tcp",
                "tls": true,
                "servername": node.reality_sni.as_ref().unwrap_or(&"www.google.com".to_string()),
                "reality-opts": {
                    "public-key": node.reality_public_key.as_ref().unwrap_or(&"".to_string()),
                    "short-id": node.reality_short_id.as_ref().unwrap_or(&"".to_string())
                },
                "client-fingerprint": "chrome"
            }));
        }
        
        // Hysteria2
        if node.hy2_port.is_some() {
            proxies.push(json!({
                "name": format!("{} HY2", node.name),
                "type": "hysteria2",
                "server": node.address,
                "port": node.hy2_port.unwrap(),
                "password": user_keys.hy2_password,
                "sni": node.hy2_sni.as_ref().unwrap_or(&node.address),
                "skip-cert-verify": true
            }));
        }
    }
    
    let proxy_names: Vec<String> = proxies.iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    
    let config = json!({
        "proxies": proxies,
        "proxy-groups": [{
            "name": "EXA-ROBOT",
            "type": "select",
            "proxies": proxy_names
        }],
        "rules": [
            "MATCH,EXA-ROBOT"
        ]
    });
    
    Ok(serde_yaml::to_string(&config)?)
}

/// Generate V2Ray base64 config (legacy VMess link format)
pub fn generate_v2ray_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    let mut links = Vec::new();
    
    for node in nodes {
        // VLESS Reality link
        if let Some(port) = node.reality_port {
            let vless_link = format!(
                "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
                user_keys.user_uuid,
                node.address,
                port,
                node.reality_sni.as_ref().unwrap_or(&"www.google.com".to_string()),
                node.reality_public_key.as_ref().unwrap_or(&"".to_string()),
                node.reality_short_id.as_ref().unwrap_or(&"".to_string()),
                urlencoding::encode(&format!("{} VLESS", node.name))
            );
            links.push(vless_link);
        }
        
        // Hysteria2 link
        if let Some(port) = node.hy2_port {
            let hy2_link = format!(
                "hysteria2://{}@{}:{}?sni={}&insecure=1#{}",
                user_keys.hy2_password,
                node.address,
                port,
                node.hy2_sni.as_ref().unwrap_or(&node.address),
                urlencoding::encode(&format!("{} HY2", node.name))
            );
            links.push(hy2_link);
        }
    }
    
    // Base64 encode all links joined by newlines
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(links.join("\n")))
}

/// Generate Sing-box JSON config (simplified for now)
pub fn generate_singbox_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    // TODO: Implement full sing-box config generation
    // For now, return minimal working config
    let mut outbounds = vec![];
    
    for node in nodes {
        if let Some(port) = node.reality_port {
            outbounds.push(json!({
                "type": "vless",
                "tag": format!("{}_vless", node.name),
                "server": node.address,
                "server_port": port,
                "uuid": user_keys.user_uuid,
                "flow": "xtls-rprx-vision",
                "tls": {
                    "enabled": true,
                    "server_name": node.reality_sni.as_ref().unwrap_or(&"www.google.com".to_string()),
                    "reality": {
                        "enabled": true,
                        "public_key": node.reality_public_key.as_ref().unwrap_or(&"".to_string()),
                        "short_id": node.reality_short_id.as_ref().unwrap_or(&"".to_string())
                    }
                }
            }));
        }
    }
    
    let config = json!({
        "log": { "level": "info" },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 2080
        }],
        "outbounds": outbounds,
        "route": {
            "rules": [],
            "final": outbounds.get(0).and_then(|o| o["tag"].as_str()).unwrap_or("direct")
        }
    });
    
    Ok(serde_json::to_string_pretty(&config)?)
}
