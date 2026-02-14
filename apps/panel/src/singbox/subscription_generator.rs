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
    pub frontend_url: Option<String>, 
    pub inbounds: Vec<crate::models::network::Inbound>,
    pub relay_info: Option<Box<NodeInfo>>, // Chaining support (Phase 8)
    
    // Policies (Phase 11)
    pub config_block_ads: bool,
    pub config_block_porn: bool,
    pub config_block_torrent: bool,
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
            frontend_url: None, 
            inbounds: vec![],
            relay_info: None,
            config_block_ads: node.config_block_ads,
            config_block_porn: node.config_block_porn,
            config_block_torrent: node.config_block_torrent,
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
            frontend_url: None, 
            inbounds,
            relay_info: None,
            config_block_ads: node.config_block_ads,
            config_block_porn: node.config_block_porn,
            config_block_torrent: node.config_block_torrent,
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
    
    // XHTTP / Advanced settings
    packet_encoding: Option<String>, // packet-up / packetaddr
    x_padding_bytes: Option<String>, // 500-1200
    xmux: Option<Value>,             // JSON object for mux settings
    
    // Hysteria 2
    hy2_ports: Option<String>,       // Port hopping range e.g. "20000-50000"
    hy2_obfs: Option<String>,        // Obfs password
    
    // TUIC v5
    tuic_congestion_control: Option<String>,
    tuic_zero_rtt_handshake: Option<bool>,
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
        .unwrap_or("chrome_random")
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
    // Note: Protocol check is done at the caller level usually, but we can't see it here.
    // However, we should be careful. Sing-box only wants 'flow' on VLESS inbounds.
    let flow = if security == "reality" && network == "tcp" {
        "xtls-rprx-vision".to_string()
    } else {
        String::new()
    };

    // XHTTP / Advanced Parsing
    let packet_encoding = v.get("packet_encoding").or_else(|| v.get("packetEncoding"))
        .and_then(|s| s.as_str()).map(|s| s.to_string());

    let x_padding_bytes = v.get("x_padding_bytes").or_else(|| v.get("xPaddingBytes"))
        .and_then(|s| s.as_str()).map(|s| s.to_string());

    let xmux = v.get("xmux").cloned();

    // Hysteria 2 Specifics
    let hy2_settings = v.get("hysteria2Settings").or_else(|| v.get("hysteria2_settings"));
    let hy2_ports = hy2_settings.and_then(|h| h.get("ports").or_else(|| h.get("server_ports")))
        .and_then(|s| s.as_str()).map(|s| s.to_string());
    let hy2_obfs = hy2_settings.and_then(|h| h.get("obfs_password").or_else(|| h.get("obfsPassword")))
        .and_then(|s| s.as_str()).map(|s| s.to_string());

    // TUIC Specifics
    let tuic_settings = v.get("tuicSettings").or_else(|| v.get("tuic_settings"));
    let tuic_congestion_control = tuic_settings.and_then(|t| t.get("congestion_control")).and_then(|v| v.as_str()).map(|s| s.to_string());
    let tuic_zero_rtt_handshake = tuic_settings.and_then(|t| t.get("zero_rtt_handshake")).and_then(|v| v.as_bool());

    StreamInfo { 
        network, security, sni, public_key, short_id, fingerprint, 
        ws_path, grpc_service, flow,
        packet_encoding, x_padding_bytes, xmux,
        hy2_ports, hy2_obfs,
        tuic_congestion_control,
        tuic_zero_rtt_handshake,
    }
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
                        
                        // XHTTP & Mux
                        if let Some(pe) = &si.packet_encoding {
                             params.push(format!("packetEncoding={}", pe));
                        }
                        
                        // Randomize padding if not set but recommended (500-1200)
                        // Note: For VLESS links, usually 'xPaddingBytes' isn't standard in all clients, 
                        // but widely supported in Xray/Sing-box via query params if using XHTTP
                        if let Some(pad) = &si.x_padding_bytes {
                             params.push(format!("xPaddingBytes={}", pad));
                        } else if si.network == "xhttp" || si.network == "httpupgrade" {
                             // Default randomization
                             use rand::Rng;
                             let mut rng = rand::rng();
                             let pad_len = rng.random_range(500..=1200);
                             params.push(format!("xPaddingBytes={}", pad_len));
                        }

                        match si.network.as_str() {
                            "ws" => params.push(format!("path={}", urlencoding::encode(&si.ws_path))),
                            "grpc" => params.push(format!("serviceName={}", si.grpc_service)),
                            "xhttp" | "httpupgrade" => {
                                params.push(format!("path={}", urlencoding::encode(&si.ws_path)));
                                params.push(format!("mode=auto")); 
                            }
                            _ => {}
                        }
                        // Frontend Masquerading Logic
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        // If masquerading, we MUST use the real SNI in the header/TLS config
                        // which is already handled by `si.sni` (streams settings)
                        // But the connection address (host/ip) in the link should be the frontend.
                        
                        links.push(format!("vless://{}@{}:{}?{}#{}",
                            user_keys.user_uuid, host, inbound.listen_port, // Use host (frontend or node IP)
                            params.join("&"), label));
                    }
                    "vmess" => {
                        // VMess uses JSON-base64 link format
                        let mut vmess_obj = json!({
                            "v": "2",
                            "ps": format!("{} - {}", node.name, inbound.remark.as_deref().unwrap_or("Auto")),
                            "add": node.frontend_url.as_deref().unwrap_or(&node.address), // Masquerading
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
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        links.push(format!("trojan://{}@{}:{}?{}#{}",
                            user_keys.user_uuid, host, inbound.listen_port, // Masquerading
                            params.join("&"), label));
                    }
                    "shadowsocks" | "ss" => {
                        let method = parse_ss_method(&inbound.settings);
                        // Phase 46: Use per-user password (consistent with orchestration_service)
                        let password = if method.contains("2022") {
                            user_keys.user_uuid.replace("-", "")
                        } else {
                            parse_ss_password(&inbound.settings)
                        };
                        
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        
                        // ss://base64(method:password)@host:port#tag
                        use base64::Engine;
                        let userinfo = base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .encode(format!("{}:{}", method, password));
                        links.push(format!("ss://{}@{}:{}#{}",
                            userinfo, host, inbound.listen_port, label));
                    }
                    "hysteria2" | "hy2" => {
                        let mut params = vec![
                           format!("sni={}", si.sni),
                           format!("insecure=1"),
                        ];
                        if let Some(ports) = &si.hy2_ports {
                            params.push(format!("mport={}", ports));
                        }
                        if let Some(obfs) = &si.hy2_obfs {
                            params.push(format!("obfs=salamander"));
                            params.push(format!("obfs-password={}", obfs));
                        }
                        
                        
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        links.push(format!("hysteria2://{}@{}:{}?{}#{}",
                            user_keys.hy2_password, host, inbound.listen_port, // Masquerading
                            params.join("&"), label));
                    }
                    "amneziawg" => {
                        let client_id = user_keys.hy2_password.split(':').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                        let local_address = format!("10.10.0.{}/32", (client_id % 250) + 2);
                        
                        // wireguard://private_key@server:port?public_key=...&preshared_key=...#label
                        // Note: Some clients use 'address' param for local address
                        let mut params = vec![
                            format!("public_key={}", si.public_key),
                            format!("address={}", urlencoding::encode(&local_address)),
                        ];
                        
                        // Add AmneziaWG obfuscation as non-standard params (supported by some clients/converted by users)
                        if let Ok(awg_obj) = serde_json::from_str::<serde_json::Value>(&inbound.settings) {
                            for field in ["jc", "jmin", "jmax", "s1", "s2", "h1", "h2", "h3", "h4"] {
                                if let Some(v) = awg_obj.get(field) {
                                    params.push(format!("{}={}", field, v));
                                }
                            }
                        }

                        links.push(format!("wireguard://{}@{}:{}?{}#{}",
                            user_keys._awg_private_key.clone().unwrap_or_default(),
                            node.address, inbound.listen_port,
                            params.join("&"), label));
                    }
                    "naive" => {
                         let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                         links.push(format!("naive+https://{}:{}@{}:{}?sni={}#{}",
                            user_keys.user_uuid, user_keys.hy2_password,
                            host, inbound.listen_port,
                            si.sni, label));
                    }
                    "tuic" => {
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let params = vec![
                            format!("sni={}", si.sni),
                            format!("congestion_control={}", si.tuic_congestion_control.as_deref().unwrap_or("bbr")),
                            format!("alpn=h3"),
                        ];
                        links.push(format!("tuic://{}:{}@{}:{}?{}#{}",
                            user_keys.user_uuid, user_keys.hy2_password,
                            host, inbound.listen_port,
                            params.join("&"), label));
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
                    "amneziawg" => {
                        let client_id = user_keys.hy2_password.split(':').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                        let local_address = format!("10.10.0.{}/32", (client_id % 250) + 2);
                        
                        let mut proxy = json!({
                            "name": name,
                            "type": "wireguard",
                            "server": node.address,
                            "port": inbound.listen_port,
                            "ip": local_address,
                            "private-key": user_keys._awg_private_key.clone().unwrap_or_default(),
                            "public-key": si.public_key,
                            "udp": true,
                            "mtu": 1280,
                        });
                        
                        // Clash Meta amnezia-wg opts
                        if let Ok(awg_obj) = serde_json::from_str::<serde_json::Value>(&inbound.settings) {
                             let mut opts = json!({});
                             for field in ["jc", "jmin", "jmax", "s1", "s2", "h1", "h2", "h3", "h4"] {
                                 if let Some(v) = awg_obj.get(field) {
                                     opts[field] = v.clone();
                                 }
                             }
                             proxy["amnezia-wg"] = opts;
                        }
                        proxies.push(proxy);
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
    let mut generated_relays: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for node in nodes {
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                
                let mut detour_tag: Option<String> = None;

                // ─── Relay Chaining (Phase 8) ────────────────────────────────
                if let Some(relay) = &node.relay_info {
                    let relay_key = format!("relay_{}", relay.address);
                    if let Some(existing_tag) = generated_relays.get(&relay_key) {
                        detour_tag = Some(existing_tag.clone());
                    } else if let Some(ri) = relay.inbounds.iter().find(|i| i.enable) {
                        let r_tag = format!("relay_{}", relay.name);
                        let r_si = parse_stream_settings(&ri.stream_settings, relay);
                        
                        let r_ob = match ri.protocol.as_str() {
                            "shadowsocks" | "ss" => {
                                let method = parse_ss_method(&ri.settings);
                                let password = parse_ss_password(&ri.settings);
                                Some(json!({
                                    "type": "shadowsocks",
                                    "tag": &r_tag,
                                    "server": relay.address,
                                    "server_port": ri.listen_port,
                                    "method": method,
                                    "password": password,
                                }))
                            },
                            "hysteria2" | "hy2" => {
                                Some(json!({
                                    "type": "hysteria2",
                                    "tag": &r_tag,
                                    "server": relay.address,
                                    "server_port": ri.listen_port,
                                    "password": user_keys.hy2_password,
                                    "tls": { 
                                        "enabled": true, 
                                        "server_name": r_si.sni, 
                                        "insecure": true, 
                                        "alpn": ["h3"] 
                                    }
                                }))
                            },
                            _ => None
                        };

                        if let Some(r_ob) = r_ob {
                            outbounds.push(r_ob);
                            generated_relays.insert(relay_key, r_tag.clone());
                            detour_tag = Some(r_tag);
                        }
                    }
                }
                // ─────────────────────────────────────────────────────────────

                let si = parse_stream_settings(&inbound.stream_settings, node);
                
                // Human-readable tag for clients (e.g. "France - VLESS Reality XHTTP")
                let display_name = if let Some(remark) = &inbound.remark {
                    if remark.starts_with("Template: ") {
                        remark.strip_prefix("Template: ").unwrap_or(remark).to_string()
                    } else {
                        remark.clone()
                    }
                } else {
                    inbound.tag.clone()
                };
                let tag = format!("{} - {}", node.name, display_name);

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut ob = json!({
                            "type": "vless",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                        });
                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }
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
                            "xhttp" | "splithttp" | "httpupgrade" => {
                                ob["transport"] = json!({
                                    "type": "httpupgrade",
                                    "host": si.sni,
                                    "path": si.ws_path
                                });
                                
                                // Packet Encoding (xudp / packetaddr)
                                if let Some(pe) = &si.packet_encoding {
                                     ob["transport"]["packet_encoding"] = json!(pe);
                                } else {
                                     ob["transport"]["packet_encoding"] = json!("xudp");
                                }
                                
                                // Multiplexing for HTTPUpgrade
                                let mut mux = si.xmux.clone().unwrap_or(json!({}));
                                if !mux.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) {
                                    mux["enabled"] = json!(true);
                                }
                                if mux.get("max_connections").is_none() {
                                    mux["max_connections"] = json!(4);
                                }
                                if mux.get("min_streams").is_none() {
                                    mux["min_streams"] = json!(2);
                                }
                                if mux.get("padding").is_none() {
                                    mux["padding"] = json!(true);
                                }
                                ob["multiplex"] = mux;
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
                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }
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
                        let mut ob = json!({
                            "type": "shadowsocks",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "method": method,
                            "password": password,
                        });
                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "hysteria2" | "hy2" => {
                        let mut ob = json!({
                            "type": "hysteria2",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "password": user_keys.hy2_password,
                            "tls": {
                                "enabled": true,
                                "server_name": si.sni,
                                "insecure": true, // User request/Policy: insecure for HY2 often default in these environments
                                "alpn": ["h3"]
                            }
                        });
                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }

                        // Add Obfuscation
                        if let Some(obfs_pass) = &si.hy2_obfs {
                            ob["obfs"] = json!({
                                "type": "salamander",
                                "password": obfs_pass
                            });
                        }

                        // Port Hopping
                        if let Some(ports) = &si.hy2_ports {
                             ob["server_ports"] = json!(ports);
                        }
                        
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "tuic" => {
                        let mut ob = json!({
                            "type": "tuic",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                            "password": user_keys.hy2_password, // Reusing hy2_password as general 'proxy_pass'
                            "congestion_control": si.tuic_congestion_control.as_deref().unwrap_or("bbr"),
                            "zero_rtt_handshake": si.tuic_zero_rtt_handshake.unwrap_or(true),
                            "tls": {
                                "enabled": true,
                                "server_name": si.sni,
                                "alpn": ["h3"]
                            }
                        });
                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "amneziawg" => {
                        let client_id = user_keys.hy2_password.split(':').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                        let local_address = format!("10.10.0.{}/32", (client_id % 250) + 2);

                        let mut ob = json!({
                            "type": "wireguard",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "local_address": local_address, // String per Hiddify error
                            "private_key": user_keys._awg_private_key.clone().unwrap_or_default(),
                            "peer_public_key": "", // Placeholder, will be replaced below
                        });
                        
                        // Fetch server's public key from inbound settings
                        if let Ok(awg_obj) = serde_json::from_str::<serde_json::Value>(&inbound.settings) {
                            if let Some(pub_key) = awg_obj.get("public_key").and_then(|v| v.as_str()) {
                                ob["peer_public_key"] = json!(pub_key);
                            }
                            // Add AmneziaWG specific fields
                            for field in ["jc", "jmin", "jmax", "s1", "s2", "h1", "h2", "h3", "h4"] {
                                if let Some(v) = awg_obj.get(field) {
                                    ob[field] = v.clone();
                                }
                            }
                        }

                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }
                        outbound_tags.push(tag);
                        outbounds.push(ob);
                    }
                    "naive" => {
                        let parts: Vec<&str> = user_keys.hy2_password.split(':').collect();
                        let username = parts.get(0).copied().unwrap_or("0");
                        let password = parts.get(1).copied().unwrap_or("");
                        
                        let mut ob = json!({
                            "type": "http",
                            "tag": tag,
                            "server": node.address,
                            "server_port": inbound.listen_port,
                            "username": username,
                            "password": password,
                            "tls": {
                                "enabled": true,
                                "server_name": si.sni,
                                "utls": { "enabled": true, "fingerprint": "chrome" }
                            }
                        });
                        if let Some(dt) = &detour_tag {
                            ob["detour"] = json!(dt);
                        }
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
            "url": "https://www.google.com/generate_204",
            "interval": "3m",
            "tolerance": 50
        }),
        json!({ "type": "direct", "tag": "direct" }),
        json!({ "type": "block", "tag": "block" }),
        json!({ "type": "dns", "tag": "dns-out" }),
    ];
    all_outbounds.extend(outbounds);

    // ─── Dynamic Rules Generation ────────────────────────────────────────────
    let mut rules_list = vec![
        json!({ "protocol": "dns", "outbound": "dns-out" }),
    ];

    // Apply Policies (Phase 11)
    // We use the policy of the first available node as the global policy for this subscription profile.
    if let Some(node) = nodes.first() {
        if node.config_block_ads {
            rules_list.push(json!({ "geosite": "category-ads-all", "outbound": "block" }));
        }
        if node.config_block_porn {
            rules_list.push(json!({ "geosite": "category-porn", "outbound": "block" }));
        }
        if node.config_block_torrent {
             rules_list.push(json!({ "protocol": "bittorrent", "outbound": "block" }));
        }
    }

    // Default Geo-routing
    rules_list.push(json!({ "geosite": ["ru", "category-gov-ru", "yandex", "vk"], "outbound": "direct" }));
    rules_list.push(json!({ "geoip": ["ru"], "outbound": "direct" }));

    let config = json!({
        "log": { "level": "info" },
        "dns": {
            "servers": [
                { "tag": "google", "address": "8.8.8.8", "strategy": "ipv4_only" },
                { "tag": "local", "address": "local", "strategy": "ipv4_only" }
            ],
            "rules": [
                { "outbound": "any", "server": "google" }
            ],
            "strategy": "ipv4_only"
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
            "rules": rules_list,
            "final": "proxy",
            "auto_detect_interface": true
        }
    });

    Ok(serde_json::to_string_pretty(&config)?)
}
