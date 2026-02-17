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
            reality_sni: node.reality_sni.clone().or(node.domain.clone()),
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
            reality_sni: node.reality_sni.clone().or(node.domain.clone()),
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
    // 1. Parse into strongly-typed struct for robust alias handling (SNI, Settings, etc.)
    let settings: crate::models::network::StreamSettings = serde_json::from_str(raw).unwrap_or_default();
    
    // 2. Parse into generic Value for fields not yet in StreamSettings struct (e.g. fingerprint, tuic/hy2 extras)
    let v: Value = serde_json::from_str(raw).unwrap_or(json!({}));

    let network = settings.network.clone().unwrap_or_else(|| "tcp".to_string());
    let security = settings.security.clone().unwrap_or_else(|| "reality".to_string());

    // SNI Extraction (Priority: Reality -> TLS -> Node Fallback)
    // SNI Extraction (Priority: Node Override -> Reality -> TLS -> Default)
    let sni = if let Some(override_sni) = &node.reality_sni {
        if !override_sni.is_empty() {
            override_sni.clone()
        } else {
             // Fallback if empty string
             extract_sni_from_settings(&settings).unwrap_or("www.google.com".to_string())
        }
    } else {
        extract_sni_from_settings(&settings).unwrap_or("www.google.com".to_string())
    };

    // Reality Keys
    let public_key = settings.reality_settings.as_ref()
        .and_then(|r| r.public_key.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| node.reality_public_key.clone().unwrap_or_default());

    let short_id = settings.reality_settings.as_ref()
        .and_then(|r| r.short_ids.first().cloned())
        .unwrap_or_else(|| node.reality_short_id.clone().unwrap_or_default());

    // Fingerprint (Not in StreamSettings yet, extract manually)
    let fingerprint = v.get("realitySettings").or_else(|| v.get("reality_settings"))
        .and_then(|r| r.get("fingerprint"))
        .and_then(|s| s.as_str())
        .unwrap_or("chrome")
        .to_string();

    // WebSocket settings
    let ws_path = settings.ws_settings.as_ref()
        .map(|w| w.path.clone())
        .unwrap_or_else(|| "/".to_string());

    // gRPC settings (Not in StreamSettings struct fully? Let's check manual parsing fallback)
    // StreamSettings has no grpc_settings field defined in models/network.rs snippet I saw? 
    // Wait, let me check models/network.rs again. It had ws_settings, http_upgrade...
    // It did NOT have grpc_settings in the snippet I read. 
    // So I must rely on `v` for grpc.
    let grpc = v.get("grpcSettings").or_else(|| v.get("grpc_settings"));
    let grpc_service = grpc
        .and_then(|g| g.get("serviceName").or_else(|| g.get("service_name")))
        .and_then(|s| s.as_str())
        .unwrap_or("grpc")
        .to_string();

    // Flow
    let explicit_flow = v.get("flow").and_then(|f| f.as_str()).unwrap_or("");
    let flow = if !explicit_flow.is_empty() {
        explicit_flow.to_string()
    } else if (security == "reality" || security == "tls") && network == "tcp" {
        "xtls-rprx-vision".to_string()
    } else {
        String::new()
    };

    // XHTTP / Advanced Parsing
    let packet_encoding = settings.packet_encoding.clone();
    
    // x_padding_bytes not in StreamSettings?
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

fn extract_sni_from_settings(settings: &crate::models::network::StreamSettings) -> Option<String> {
    if let Some(reality) = &settings.reality_settings {
        // Priority: server_names[0] -> server_name (singular)
        reality.server_names.first().cloned()
            .or_else(|| reality.server_name.clone())
            .filter(|s| !s.is_empty())
    } else if let Some(tls) = &settings.tls_settings {
        if !tls.server_name.is_empty() {
            Some(tls.server_name.clone())
        } else {
            None
        }
    } else {
        None
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

fn parse_ss_password(settings_raw: &str, user_uuid: &str) -> String {
    let v: Value = serde_json::from_str(settings_raw).unwrap_or(json!({}));
    
    // 1. Try to find in `users` list matching UUID
    if let Some(users) = v.get("users").and_then(|u| u.as_array()) {
        for user in users {
            // Check username/name against UUID
            if let Some(username) = user.get("username").or(user.get("name")).and_then(|u| u.as_str()) {
                if username == user_uuid {
                    return user.get("password").and_then(|p| p.as_str()).unwrap_or("").to_string();
                }
            }
        }
        // Fallback: if list has 1 item and we didn't match (maybe single user mode but ID mismatch?), use it.
        if users.len() == 1 {
            return users[0].get("password").and_then(|p| p.as_str()).unwrap_or("").to_string();
        }
    }

    // 2. Fallback to top-level password
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
                        let password = parse_ss_password(&inbound.settings, &user_keys.user_uuid);
                        
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
                        let password = parse_ss_password(&inbound.settings, &user_keys.user_uuid);
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

/// Generate Sing-box JSON config (multi-protocol) with smart routing
pub fn generate_singbox_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
) -> Result<String> {
    let mut outbounds = vec![];
    let mut outbound_tags = vec![];
    let mut generated_relays: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // 1. Generate Proxy Outbounds
    for node in nodes {
        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable { continue; }
                
                let mut detour_tag: Option<String> = None;

                // ─── Relay Chaining Support ──────────────────────────────────
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
                                let password = parse_ss_password(&ri.settings, &user_keys.user_uuid);
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
                
                // Human-readable tag
                let display_name = if let Some(remark) = &inbound.remark {
                    if remark.starts_with("Template: ") {
                        remark.strip_prefix("Template: ").unwrap_or(remark).trim().to_string()
                    } else {
                        format!("{} - {}", node.name, remark)
                    }
                } else {
                    format!("{} - Auto", node.name)
                }
                .replace("Template: ", "");

                let mut outbound = json!({
                    "tag": display_name,
                });

                match inbound.protocol.as_str() {
                    "vless" => {
                        outbound["type"] = json!("vless");
                        outbound["server"] = json!(node.address);
                        outbound["server_port"] = json!(inbound.listen_port);
                        outbound["uuid"] = json!(user_keys.user_uuid);
                        outbound["flow"] = if !si.flow.is_empty() { json!(si.flow) } else { json!("") };
                        
                        let mut tls = json!({ "enabled": false });
                        if si.security == "reality" {
                            tls["enabled"] = json!(true);
                            tls["server_name"] = json!(si.sni);
                            tls["reality"] = json!({
                                "enabled": true,
                                "public_key": si.public_key,
                                "short_id": si.short_id
                            });
                            tls["utls"] = json!({ "enabled": true, "fingerprint": si.fingerprint });
                        } else if si.security == "tls" {
                            tls["enabled"] = json!(true);
                            tls["server_name"] = json!(si.sni);
                            tls["utls"] = json!({ "enabled": true, "fingerprint": si.fingerprint });
                        }
                        outbound["tls"] = tls;

                        if si.network == "ws" {
                            outbound["transport"] = json!({
                                "type": "ws",
                                "path": si.ws_path,
                                "headers": { "Host": si.sni }
                            });
                        } else if si.network == "grpc" {
                            outbound["transport"] = json!({
                                "type": "grpc",
                                "service_name": si.grpc_service
                            });
                        } else if matches!(si.network.as_str(), "xhttp" | "splithttp" | "httpupgrade") {
                             outbound["transport"] = json!({
                                "type": "httpupgrade",
                                "path": si.ws_path,
                                "host": if si.sni.is_empty() { Value::Null } else { json!([si.sni]) }
                             });
                             
                            // Packet Encoding
                            if let Some(pe) = &si.packet_encoding {
                                outbound["packet_encoding"] = json!(pe);
                            } else {
                                outbound["packet_encoding"] = json!("xudp");
                            }

                            // Multiplexing
                            if let Some(mux) = &si.xmux {
                                outbound["multiplex"] = mux.clone();
                            }
                        }
                    },
                    "hysteria2" | "hy2" => {
                        outbound["type"] = json!("hysteria2");
                        outbound["server"] = json!(node.address);
                        outbound["server_port"] = json!(inbound.listen_port);
                        outbound["password"] = json!(user_keys.hy2_password);
                        
                        let tls = json!({
                            "enabled": true,
                            "server_name": si.sni,
                            "insecure": true, 
                            "alpn": ["h3"]
                        });
                        outbound["tls"] = tls;
                        
                        if let Some(obfs) = &si.hy2_obfs {
                            outbound["obfs"] = json!({
                                "type": "salamander",
                                "password": obfs
                            });
                        }
                    },
                    "tuic" => {
                        outbound["type"] = json!("tuic");
                        outbound["server"] = json!(node.address);
                        outbound["server_port"] = json!(inbound.listen_port);
                        outbound["uuid"] = json!(user_keys.user_uuid);
                        outbound["password"] = json!(user_keys.hy2_password); 
                        outbound["congestion_control"] = json!(si.tuic_congestion_control.as_deref().unwrap_or("bbr"));
                        outbound["zero_rtt_handshake"] = json!(si.tuic_zero_rtt_handshake.unwrap_or(false));
                        
                        outbound["tls"] = json!({
                            "enabled": true,
                            "server_name": si.sni,
                            "alpn": ["h3"],
                             "insecure": true 
                        });
                    },
                    "trojan" => {
                        outbound["type"] = json!("trojan");
                        outbound["server"] = json!(node.address);
                        outbound["server_port"] = json!(inbound.listen_port);
                        outbound["password"] = json!(user_keys.user_uuid);
                        
                        let mut tls = json!({ "enabled": true, "server_name": si.sni });
                        if si.security == "reality" {
                             tls["reality"] = json!({
                                "enabled": true,
                                "public_key": si.public_key,
                                "short_id": si.short_id
                            });
                            tls["utls"] = json!({ "enabled": true, "fingerprint": si.fingerprint });
                        }
                         outbound["tls"] = tls;
                         
                        if si.network == "ws" {
                            outbound["transport"] = json!({
                                "type": "ws",
                                "path": si.ws_path,
                                "headers": { "Host": si.sni }
                            });
                        }
                    },
                    "shadowsocks" | "ss" => {
                        outbound["type"] = json!("shadowsocks");
                        outbound["server"] = json!(node.address);
                        outbound["server_port"] = json!(inbound.listen_port);
                        outbound["method"] = json!(parse_ss_method(&inbound.settings));
                        outbound["password"] = json!(parse_ss_password(&inbound.settings, &user_keys.user_uuid));
                    },
                    "naive" => {
                         outbound["type"] = json!("naive"); // Assuming naive plugin/support
                         outbound["server"] = json!(node.address);
                         outbound["server_port"] = json!(inbound.listen_port);
                         outbound["username"] = json!(user_keys.user_uuid);
                         outbound["password"] = json!(user_keys.hy2_password);
                         outbound["tls"] = json!({
                            "enabled": true,
                            "server_name": si.sni,
                            "alpn": ["h2", "http/1.1"]
                         });
                    },
                    "amneziawg" => {
                         let client_id = user_keys.hy2_password.split(':').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                         let local_address = format!("10.10.0.{}/32", (client_id % 250) + 2);
                         
                         outbound["type"] = json!("wireguard");
                         outbound["server"] = json!(node.address);
                         outbound["server_port"] = json!(inbound.listen_port);
                         outbound["local_address"] = json!([local_address]);
                         outbound["private_key"] = json!(user_keys._awg_private_key.clone().unwrap_or_default());
                         outbound["peer_public_key"] = json!(si.public_key);
                         outbound["mtu"] = json!(1280);
                         
                         if let Ok(awg_obj) = serde_json::from_str::<serde_json::Value>(&inbound.settings) {
                             if let Some(jc) = awg_obj.get("jc") { 
                                 outbound["reserved"] = json!([jc.as_u64().unwrap_or(0), awg_obj["jmin"].as_u64().unwrap_or(0), awg_obj["jmax"].as_u64().unwrap_or(0)]); 
                             }
                         }
                    }
                    _ => continue,
                }

                if let Some(tag) = detour_tag {
                    outbound["detour"] = json!(tag);
                }
                
                outbound_tags.push(display_name.clone());
                outbounds.push(outbound);
            }
        }
    }

    if outbound_tags.is_empty() {
        return Ok(json!({}).to_string());
    }

    // 2. Wrap into Selectors/URLTest
    let mut final_outbounds = Vec::new();

    // 2.1 Proxy Selector (Main Group)
    let mut proxy_group_tags = vec!["auto".to_string()];
    proxy_group_tags.extend(outbound_tags.clone()); // Add all proxies to selector
    // proxy_group_tags.push("direct".to_string()); // Optional: allow manual direct

    final_outbounds.push(json!({
        "type": "selector",
        "tag": "proxy",
        "outbounds": proxy_group_tags,
        "default": "auto"
    }));

    // 2.2 Auto URLTest Group
    final_outbounds.push(json!({
        "type": "urltest",
        "tag": "auto",
        "outbounds": outbound_tags,
        "url": "https://www.gstatic.com/generate_204",
        "interval": "3m",
        "tolerance": 50
    }));

    // 2.3 Add DIRECT and BLOCK
    final_outbounds.push(json!({ "type": "direct", "tag": "direct" }));
    final_outbounds.push(json!({ "type": "block", "tag": "block" }));
    final_outbounds.push(json!({ "type": "dns", "tag": "dns-out" }));

    // 2.4 Add Generated Proxies
    final_outbounds.extend(outbounds);


    // Aggregated Policies
    let block_ads = nodes.iter().any(|n| n.config_block_ads);
    let block_porn = nodes.iter().any(|n| n.config_block_porn);
    // let block_torrent = nodes.iter().any(|n| n.config_block_torrent); // Client-side P2P blocking is harder to enforce reliably via geosite alone, but we can try.

    // 3. DNS Configuration
    let mut dns_rules = vec![
             json!({ "outbound": ["any"], "server": "local" }), // Default local? No, usually reverse
             json!({ "clash_mode": "direct", "server": "local" }),
             json!({ "clash_mode": "global", "server": "google" }),
             // Domain based rules
             json!({ "geosite": "cn", "server": "local" }),
    ];

    if block_ads {
        dns_rules.push(json!({ "geosite": "category-ads-all", "server": "block" }));
    }
    if block_porn {
        dns_rules.push(json!({ "geosite": "category-porn", "server": "block" }));
    }

    let dns_config = json!({
        "servers": [
            { "tag": "google", "address": "8.8.8.8", "detour": "proxy" }, // Route DNS through proxy to avoid leaks/poisoning
            { "tag": "local", "address": "local", "detour": "direct" }
        ],
        "rules": dns_rules,
        "final": "google",
        "strategy": "ipv4_only" // Safer for most
    });

    // 4. Route Rules
    let mut route_rules = vec![
            json!({ "protocol": "dns", "outbound": "dns-out" }),
    ];

    if block_ads {
        route_rules.push(json!({ "geosite": ["category-ads-all"], "outbound": "block" }));
    }
    if block_porn {
        route_rules.push(json!({ "geosite": ["category-porn"], "outbound": "block" }));
    }

    route_rules.extend(vec![
            json!({ "geosite": ["cn", "private"], "outbound": "direct" }),
            json!({ "geoip": ["cn", "private"], "outbound": "direct" }),
            // Add RU/UA specifics if needed, for now standard
            json!({ "geosite": ["ru"], "outbound": "direct" }),
            json!({ "geoip": ["ru"], "outbound": "direct" })
    ]);

    let route_config = json!({
        "auto_detect_interface": true,
        "final": "proxy",
        "rules": route_rules
    });
    
    // 5. Inbounds (Mixed Port for Client)
    let inbounds_config = vec![
        json!({
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 2080,
            "sniff": true,
            "sniff_override_destination": true
        })
    ];

    // 6. Final Assembly
    let config = json!({
        "log": {
            "level": "info",
            "timestamp": true
        },
        "dns": dns_config,
        "inbounds": inbounds_config,
        "outbounds": final_outbounds,
        "route": route_config, 
        "experimental": {
            "cache_file": {
                "enabled": true,
                "store_fakeip": true
            },
            "clash_api": {
                "external_controller": "127.0.0.1:9090",
                "external_ui": "ui",
                "external_ui_download_url": "https://github.com/MetaCubeX/Yacd-meta/archive/gh-pages.zip",
                "external_ui_download_detour": "proxy",
                "default_mode": "rule"
            }
        }
    });

    Ok(serde_json::to_string_pretty(&config)?)
}
