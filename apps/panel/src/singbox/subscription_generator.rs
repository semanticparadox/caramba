use crate::models::store::Subscription;
use anyhow::Result;
use serde_json::{json, Value};

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
            address: node.ip.clone(),
            reality_port: Some(node.vpn_port as i32),
            reality_sni: node.domain.clone(),
            reality_public_key: node.reality_pub.clone(),
            reality_short_id: node.short_id.clone(),
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

// ─── Helper: Parse stream_settings JSON ───────────────────────────────────────

/// Parsed transport/security info from an inbound's stream_settings JSON
struct StreamInfo {
    network: String,       // tcp, ws, grpc, xhttp, quic
    security: String,      // reality, tls, none
    sni: String,
    public_key: String,    // Reality only
    short_id: String,      // Reality only
    fingerprint: String,
    ws_path: String,       // WebSocket path
    grpc_service: String,  // gRPC serviceName
    flow: String,          // xtls-rprx-vision (Reality+TCP only)
}

fn parse_stream_settings(raw: &str, node: &NodeInfo) -> StreamInfo {
    let v: Value = serde_json::from_str(raw).unwrap_or(json!({}));

    let network = v.get("network")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp")
        .to_string();

    let security = v.get("security")
        .and_then(|v| v.as_str())
        .unwrap_or("reality")
        .to_string();

    // Reality settings
    let reality = v.get("realitySettings")
        .or_else(|| v.get("reality_settings"));

    let sni = reality
        .and_then(|r| r.get("serverNames").or_else(|| r.get("server_names")))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // TLS SNI fallback
            v.get("tlsSettings")
                .and_then(|t| t.get("serverName"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| node.reality_sni.clone().unwrap_or("www.google.com".to_string()));

    let public_key = reality
        .and_then(|r| r.get("publicKey").or_else(|| r.get("public_key")))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        // Also check privateKey → we need publicKey for client
        .unwrap_or_else(|| node.reality_public_key.clone().unwrap_or_default());

    let short_id = reality
        .and_then(|r| r.get("shortIds").or_else(|| r.get("short_ids")))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| node.reality_short_id.clone().unwrap_or_default());

    let fingerprint = reality
        .and_then(|r| r.get("fingerprint"))
        .and_then(|s| s.as_str())
        .unwrap_or("chrome")
        .to_string();

    // WebSocket settings
    let ws = v.get("wsSettings").or_else(|| v.get("ws_settings"));
    let ws_path = ws
        .and_then(|w| w.get("path"))
        .and_then(|s| s.as_str())
        .unwrap_or("/")
        .to_string();

    // gRPC settings
    let grpc = v.get("grpcSettings").or_else(|| v.get("grpc_settings"));
    let grpc_service = grpc
        .and_then(|g| g.get("serviceName").or_else(|| g.get("service_name")))
        .and_then(|s| s.as_str())
        .unwrap_or("grpc")
        .to_string();

    // Flow: only use xtls-rprx-vision for Reality+TCP VLESS
    let flow = if security == "reality" && network == "tcp" {
        "xtls-rprx-vision".to_string()
    } else {
        String::new()
    };

    StreamInfo { network, security, sni, public_key, short_id, fingerprint, ws_path, grpc_service, flow }
}

// ─── Helper: Parse Shadowsocks method from settings JSON ──────────────────────

fn parse_ss_method(settings_raw: &str) -> String {
    let v: Value = serde_json::from_str(settings_raw).unwrap_or(json!({}));
    v.get("method")
        .and_then(|s| s.as_str())
        .unwrap_or("2022-blake3-aes-128-gcm")
        .to_string()
}

fn parse_ss_password(settings_raw: &str) -> String {
    let v: Value = serde_json::from_str(settings_raw).unwrap_or(json!({}));
    v.get("password")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// V2Ray Link Generation (base64 encoded links)
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate V2Ray base64 config (multi-protocol link format)
pub fn generate_v2ray_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    let mut links = Vec::new();

    for node in nodes {
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                let si = parse_stream_settings(&inbound.stream_settings, node);
                let label_raw = format!("{} - {}", 
                    node.name, inbound.remark.as_deref().unwrap_or("Auto"));
                let label = urlencoding::encode(&label_raw);

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut params = vec![
                            format!("encryption=none"),
                            format!("type={}", si.network),
                            format!("security={}", si.security),
                            format!("sni={}", si.sni),
                            format!("fp={}", si.fingerprint),
                        ];
                        if !si.flow.is_empty() {
                            params.push(format!("flow={}", si.flow));
                        }
                        if si.security == "reality" {
                            params.push(format!("pbk={}", si.public_key));
                            params.push(format!("sid={}", si.short_id));
                        }
                        match si.network.as_str() {
                            "ws" => params.push(format!("path={}", urlencoding::encode(&si.ws_path))),
                            "grpc" => params.push(format!("serviceName={}", si.grpc_service)),
                            _ => {}
                        }
                        links.push(format!("vless://{}@{}:{}?{}#{}",
                            user_keys.user_uuid, node.address, inbound.listen_port,
                            params.join("&"), label));
                    }
                    "vmess" => {
                        // VMess uses JSON-base64 link format
                        let mut vmess_obj = json!({
                            "v": "2",
                            "ps": format!("{} - {}", node.name, inbound.remark.as_deref().unwrap_or("Auto")),
                            "add": node.address,
                            "port": inbound.listen_port.to_string(),
                            "id": user_keys.user_uuid,
                            "aid": "0",
                            "scy": "auto",
                            "net": si.network,
                            "type": "none",
                            "tls": if si.security == "tls" { "tls" } else { "" },
                            "sni": si.sni,
                            "fp": si.fingerprint,
                        });
                        if si.network == "ws" {
                            vmess_obj["path"] = json!(si.ws_path);
                            vmess_obj["host"] = json!(si.sni);
                        }
                        if si.network == "grpc" {
                            vmess_obj["path"] = json!(si.grpc_service);
                        }
                        use base64::Engine;
                        let encoded = base64::engine::general_purpose::STANDARD
                            .encode(serde_json::to_string(&vmess_obj)?);
                        links.push(format!("vmess://{}", encoded));
                    }
                    "trojan" => {
                        let mut params = vec![
                            format!("type={}", si.network),
                            format!("security={}", si.security),
                            format!("sni={}", si.sni),
                            format!("fp={}", si.fingerprint),
                        ];
                        if si.security == "reality" {
                            params.push(format!("pbk={}", si.public_key));
                            params.push(format!("sid={}", si.short_id));
                        }
                        match si.network.as_str() {
                            "ws" => params.push(format!("path={}", urlencoding::encode(&si.ws_path))),
                            "grpc" => params.push(format!("serviceName={}", si.grpc_service)),
                            _ => {}
                        }
                        // Trojan uses user_uuid as password
                        links.push(format!("trojan://{}@{}:{}?{}#{}",
                            user_keys.user_uuid, node.address, inbound.listen_port,
                            params.join("&"), label));
                    }
                    "shadowsocks" | "ss" => {
                        let method = parse_ss_method(&inbound.settings);
                        let password = parse_ss_password(&inbound.settings);
                        // ss://base64(method:password)@host:port#tag
                        use base64::Engine;
                        let userinfo = base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .encode(format!("{}:{}", method, password));
                        links.push(format!("ss://{}@{}:{}#{}",
                            userinfo, node.address, inbound.listen_port, label));
                    }
                    "hysteria2" | "hy2" => {
                        links.push(format!("hysteria2://{}@{}:{}?sni={}&insecure=1#{}",
                            user_keys.hy2_password, node.address, inbound.listen_port,
                            si.sni, label));
                    }
                    _ => {
                        // Unknown protocol, skip
                    }
                }
            }
        }
        // Legacy fallback: VLESS Reality
        else if let Some(port) = node.reality_port {
            let vless_link = format!(
                "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
                user_keys.user_uuid, node.address, port,
                node.reality_sni.as_ref().unwrap_or(&"www.google.com".to_string()),
                node.reality_public_key.as_ref().unwrap_or(&"".to_string()),
                node.reality_short_id.as_ref().unwrap_or(&"".to_string()),
                urlencoding::encode(&format!("{} VLESS", node.name))
            );
            links.push(vless_link);
        }

        // Legacy Hysteria2
        if let Some(port) = node.hy2_port {
            links.push(format!(
                "hysteria2://{}@{}:{}?sni={}&insecure=1#{}",
                user_keys.hy2_password, node.address, port,
                node.hy2_sni.as_ref().unwrap_or(&node.address),
                urlencoding::encode(&format!("{} HY2", node.name))
            ));
        }
    }

    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(links.join("\n")))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Clash YAML Config Generation
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate Clash YAML config (multi-protocol)
pub fn generate_clash_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    let mut proxies = Vec::new();

    for node in nodes {
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                let si = parse_stream_settings(&inbound.stream_settings, node);
                let name = format!("{} - {}", node.name, inbound.remark.as_deref().unwrap_or("Auto"));

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut proxy = json!({
                            "name": name,
                            "type": "vless",
                            "server": node.address,
                            "port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                            "network": si.network,
                            "client-fingerprint": si.fingerprint,
                        });
                        if !si.flow.is_empty() {
                            proxy["flow"] = json!(si.flow);
                        }
                        if si.security == "reality" {
                            proxy["tls"] = json!(true);
                            proxy["servername"] = json!(si.sni);
                            proxy["reality-opts"] = json!({
                                "public-key": si.public_key,
                                "short-id": si.short_id
                            });
                        } else if si.security == "tls" {
                            proxy["tls"] = json!(true);
                            proxy["servername"] = json!(si.sni);
                        }
                        if si.network == "ws" {
                            proxy["ws-opts"] = json!({
                                "path": si.ws_path,
                                "headers": { "Host": si.sni }
                            });
                        }
                        if si.network == "grpc" {
                            proxy["grpc-opts"] = json!({
                                "grpc-service-name": si.grpc_service
                            });
                        }
                        proxies.push(proxy);
                    }
                    "vmess" => {
                        let mut proxy = json!({
                            "name": name,
                            "type": "vmess",
                            "server": node.address,
                            "port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                            "alterId": 0,
                            "cipher": "auto",
                            "network": si.network,
                        });
                        if si.security == "tls" {
                            proxy["tls"] = json!(true);
                            proxy["servername"] = json!(si.sni);
                        }
                        if si.network == "ws" {
                            proxy["ws-opts"] = json!({
                                "path": si.ws_path,
                                "headers": { "Host": si.sni }
                            });
                        }
                        if si.network == "grpc" {
                            proxy["grpc-opts"] = json!({
                                "grpc-service-name": si.grpc_service
                            });
                        }
                        proxies.push(proxy);
                    }
                    "trojan" => {
                        let mut proxy = json!({
                            "name": name,
                            "type": "trojan",
                            "server": node.address,
                            "port": inbound.listen_port,
                            "password": user_keys.user_uuid,
                            "sni": si.sni,
                        });
                        if si.security == "reality" {
                            proxy["reality-opts"] = json!({
                                "public-key": si.public_key,
                                "short-id": si.short_id
                            });
                            proxy["client-fingerprint"] = json!(si.fingerprint);
                        }
                        if si.network == "ws" {
                            proxy["network"] = json!("ws");
                            proxy["ws-opts"] = json!({
                                "path": si.ws_path,
                                "headers": { "Host": si.sni }
                            });
                        }
                        if si.network == "grpc" {
                            proxy["network"] = json!("grpc");
                            proxy["grpc-opts"] = json!({
                                "grpc-service-name": si.grpc_service
                            });
                        }
                        proxies.push(proxy);
                    }
                    "shadowsocks" | "ss" => {
                        let method = parse_ss_method(&inbound.settings);
                        let password = parse_ss_password(&inbound.settings);
                        proxies.push(json!({
                            "name": name,
                            "type": "ss",
                            "server": node.address,
                            "port": inbound.listen_port,
                            "cipher": method,
                            "password": password,
                        }));
                    }
                    "hysteria2" | "hy2" => {
                        proxies.push(json!({
                            "name": name,
                            "type": "hysteria2",
                            "server": node.address,
                            "port": inbound.listen_port,
                            "password": user_keys.hy2_password,
                            "sni": si.sni,
                            "skip-cert-verify": true,
                        }));
                    }
                    _ => {}
                }
            }
        }
        // Legacy fallback
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

        // Legacy Hysteria2
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
        }, {
            "name": "Auto",
            "type": "url-test",
            "proxies": proxy_names,
            "url": "http://www.gstatic.com/generate_204",
            "interval": 300
        }],
        "rules": [
            "MATCH,EXA-ROBOT"
        ]
    });

    Ok(serde_yaml::to_string(&config)?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sing-box JSON Config Generation
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate Sing-box JSON config (multi-protocol)
pub fn generate_singbox_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    let mut outbounds = vec![];
    let mut outbound_tags = vec![];

    for node in nodes {
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                let si = parse_stream_settings(&inbound.stream_settings, node);
                let tag = format!("{}_{}", node.name, inbound.tag);

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut ob = json!({
                            "type": "vless",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                        });
                        if !si.flow.is_empty() {
                            ob["flow"] = json!(si.flow);
                        }
                        // TLS / Reality
                        if si.security == "reality" {
                            ob["tls"] = json!({
                                "enabled": true,
                                "server_name": si.sni,
                                "utls": { "enabled": true, "fingerprint": si.fingerprint },
                                "reality": {
                                    "enabled": true,
                                    "public_key": si.public_key,
                                    "short_id": si.short_id
                                }
                            });
                        } else if si.security == "tls" {
                            ob["tls"] = json!({
                                "enabled": true,
                                "server_name": si.sni,
                                "utls": { "enabled": true, "fingerprint": si.fingerprint }
                            });
                        }
                        // Transport
                        match si.network.as_str() {
                            "ws" => {
                                ob["transport"] = json!({
                                    "type": "ws",
                                    "path": si.ws_path,
                                    "headers": { "Host": si.sni }
                                });
                            }
                            "grpc" => {
                                ob["transport"] = json!({
                                    "type": "grpc",
                                    "service_name": si.grpc_service
                                });
                            }
                            "xhttp" | "splithttp" => {
                                ob["transport"] = json!({
                                    "type": "httpupgrade",
                                    "host": si.sni,
                                    "path": si.ws_path
                                });
                            }
                            _ => {} // tcp = no transport block
                        }
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "vmess" => {
                        let mut ob = json!({
                            "type": "vmess",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                            "alter_id": 0,
                            "security": "auto",
                        });
                        if si.security == "tls" {
                            ob["tls"] = json!({
                                "enabled": true,
                                "server_name": si.sni,
                                "utls": { "enabled": true, "fingerprint": si.fingerprint }
                            });
                        }
                        match si.network.as_str() {
                            "ws" => {
                                ob["transport"] = json!({
                                    "type": "ws",
                                    "path": si.ws_path,
                                    "headers": { "Host": si.sni }
                                });
                            }
                            "grpc" => {
                                ob["transport"] = json!({
                                    "type": "grpc",
                                    "service_name": si.grpc_service
                                });
                            }
                            _ => {}
                        }
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "trojan" => {
                        let mut ob = json!({
                            "type": "trojan",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "password": user_keys.user_uuid,
                        });
                        if si.security == "tls" || si.security == "reality" {
                            let mut tls = json!({
                                "enabled": true,
                                "server_name": si.sni,
                                "utls": { "enabled": true, "fingerprint": si.fingerprint }
                            });
                            if si.security == "reality" {
                                tls["reality"] = json!({
                                    "enabled": true,
                                    "public_key": si.public_key,
                                    "short_id": si.short_id
                                });
                            }
                            ob["tls"] = tls;
                        }
                        match si.network.as_str() {
                            "ws" => {
                                ob["transport"] = json!({
                                    "type": "ws",
                                    "path": si.ws_path,
                                    "headers": { "Host": si.sni }
                                });
                            }
                            "grpc" => {
                                ob["transport"] = json!({
                                    "type": "grpc",
                                    "service_name": si.grpc_service
                                });
                            }
                            _ => {}
                        }
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "shadowsocks" | "ss" => {
                        let method = parse_ss_method(&inbound.settings);
                        let password = parse_ss_password(&inbound.settings);
                        let ob = json!({
                            "type": "shadowsocks",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "method": method,
                            "password": password,
                        });
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "hysteria2" | "hy2" => {
                        let ob = json!({
                            "type": "hysteria2",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "password": user_keys.hy2_password,
                            "tls": {
                                "enabled": true,
                                "server_name": si.sni,
                                "insecure": true
                            }
                        });
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    _ => {}
                }
            }
        }
        // Legacy fallback
        else if let Some(port) = node.reality_port {
            let tag = format!("{}_vless", node.name);
            outbounds.push(json!({
                "type": "vless",
                "tag": &tag,
                "server": node.address,
                "server_port": port,
                "uuid": user_keys.user_uuid,
                "flow": "xtls-rprx-vision",
                "tls": {
                    "enabled": true,
                    "server_name": node.reality_sni.as_ref().unwrap_or(&"www.google.com".to_string()),
                    "utls": { "enabled": true, "fingerprint": "chrome" },
                    "reality": {
                        "enabled": true,
                        "public_key": node.reality_public_key.as_ref().unwrap_or(&"".to_string()),
                        "short_id": node.reality_short_id.as_ref().unwrap_or(&"".to_string())
                    }
                }
            }));
            outbound_tags.push(tag);
        }
    }

    // Add selector and urltest outbounds
    let mut all_outbounds = vec![
        json!({
            "type": "selector",
            "tag": "proxy",
            "outbounds": outbound_tags,
            "default": outbound_tags.first().unwrap_or(&"direct".to_string()),
        }),
        json!({
            "type": "urltest",
            "tag": "auto",
            "outbounds": outbound_tags,
            "url": "http://www.gstatic.com/generate_204",
            "interval": "3m"
        }),
        json!({ "type": "direct", "tag": "direct" }),
        json!({ "type": "block", "tag": "block" }),
        json!({ "type": "dns", "tag": "dns-out" }),
    ];
    all_outbounds.extend(outbounds);

    let config = json!({
        "log": { "level": "info" },
        "dns": {
            "servers": [
                { "tag": "google", "address": "tls://8.8.8.8" },
                { "tag": "local", "address": "local", "detour": "direct" }
            ]
        },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 2080,
            "sniff": true
        }],
        "outbounds": all_outbounds,
        "route": {
            "rules": [
                { "protocol": "dns", "outbound": "dns-out" }
            ],
            "final": "proxy",
            "auto_detect_interface": true
        }
    });

    Ok(serde_json::to_string_pretty(&config)?)
}
