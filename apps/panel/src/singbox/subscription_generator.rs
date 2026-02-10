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
    pub inbounds: Vec<crate::models::network::Inbound>,
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
            inbounds: vec![],
        }
    }
}

// Convert from Node + Inbounds
impl NodeInfo {
    pub fn new(node: &crate::models::node::Node, inbounds: Vec<crate::models::network::Inbound>) -> Self {
        Self {
            name: node.name.clone(),
            address: node.ip.clone(),
            reality_port: Some(node.vpn_port as i32),
            reality_sni: node.domain.clone(),
            reality_public_key: node.reality_pub.clone(),
            reality_short_id: node.short_id.clone(),
            hy2_port: None,
            hy2_sni: None,
            inbounds,
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
        // Priority: Use Inbounds if available
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                
                // Parse settings (simplified for now, ideally use strongly typed structs from network.rs)
                // VLESS
                if inbound.protocol == "vless" {
                     let sni = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("server_names"))
                         .and_then(|v| v.as_array())
                         .and_then(|a| a.first())
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_sni.clone().unwrap_or("www.google.com".to_string()))
                     } else {
                        node.reality_sni.clone().unwrap_or("www.google.com".to_string())
                     };

                     let pbk = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("public_key"))
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_public_key.clone().unwrap_or_default())
                     } else {
                        node.reality_public_key.clone().unwrap_or_default()
                     };

                     let sid = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("short_ids"))
                         .and_then(|v| v.as_array())
                         .and_then(|a| a.first())
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_short_id.clone().unwrap_or_default())
                     } else {
                        node.reality_short_id.clone().unwrap_or_default()
                     };

                     proxies.push(json!({
                        "name": format!("{} - {}", node.name, inbound.remark.as_deref().unwrap_or("Auto")),
                        "type": "vless",
                        "server": node.address,
                        "port": inbound.listen_port,
                        "uuid": user_keys.user_uuid,
                        "network": "tcp",
                        "tls": true,
                        "servername": sni,
                        "reality-opts": {
                            "public-key": pbk,
                            "short-id": sid
                        },
                        "client-fingerprint": "chrome"
                    }));
                }
            }
        } 
        // Fallback to legacy fields if no inbounds
        else if node.reality_port.is_some() {
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
        // Priority: Use Inbounds if available
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                
                if inbound.protocol == "vless" {
                     let sni = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("server_names"))
                         .and_then(|v| v.as_array())
                         .and_then(|a| a.first())
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_sni.clone().unwrap_or("www.google.com".to_string()))
                     } else {
                        node.reality_sni.clone().unwrap_or("www.google.com".to_string())
                     };

                     let pbk = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("public_key"))
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_public_key.clone().unwrap_or_default())
                     } else {
                        node.reality_public_key.clone().unwrap_or_default()
                     };

                     let sid = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("short_ids"))
                         .and_then(|v| v.as_array())
                         .and_then(|a| a.first())
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_short_id.clone().unwrap_or_default())
                     } else {
                        node.reality_short_id.clone().unwrap_or_default()
                     };

                    let vless_link = format!(
                        "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
                        user_keys.user_uuid,
                        node.address,
                        inbound.listen_port,
                        sni,
                        pbk,
                        sid,
                        urlencoding::encode(&format!("{} - {}", node.name, inbound.remark.as_deref().unwrap_or("Auto")))
                    );
                    links.push(vless_link);
                }
            }
        }
        // Fallback
        else if let Some(port) = node.reality_port {
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
        if !node.inbounds.is_empty() {
             for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                // Implement VLESS for sing-box
                if inbound.protocol == "vless" {
                     let sni = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("server_names"))
                         .and_then(|v| v.as_array())
                         .and_then(|a| a.first())
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_sni.clone().unwrap_or("www.google.com".to_string()))
                     } else {
                        node.reality_sni.clone().unwrap_or("www.google.com".to_string())
                     };

                     let pbk = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("public_key"))
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_public_key.clone().unwrap_or_default())
                     } else {
                        node.reality_public_key.clone().unwrap_or_default()
                     };

                     let sid = if let Ok(s) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
                        s.get("reality_settings")
                         .and_then(|r| r.get("short_ids"))
                         .and_then(|v| v.as_array())
                         .and_then(|a| a.first())
                         .and_then(|s| s.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(node.reality_short_id.clone().unwrap_or_default())
                     } else {
                        node.reality_short_id.clone().unwrap_or_default()
                     };

                    outbounds.push(json!({
                        "type": "vless",
                        "tag": format!("{}_{}", node.name, inbound.tag),
                        "server": node.address,
                        "server_port": inbound.listen_port,
                        "uuid": user_keys.user_uuid,
                        "flow": "xtls-rprx-vision",
                        "tls": {
                            "enabled": true,
                            "server_name": sni,
                            "reality": {
                                "enabled": true,
                                "public_key": pbk,
                                "short_id": sid
                            }
                        }
                    }));
                }
             }
        }
        else if let Some(port) = node.reality_port {
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
