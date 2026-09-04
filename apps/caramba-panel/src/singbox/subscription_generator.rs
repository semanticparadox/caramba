use anyhow::Result;
use caramba_db::models::store::Subscription;
use serde_json::{Value, json};

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
    pub inbounds: Vec<caramba_db::models::network::Inbound>,
    pub relay_info: Option<Box<NodeInfo>>, // Chaining support

    // Geo / display
    pub country_code: Option<String>, // e.g. "DE", "NL", "RU"
    pub is_relay: bool,               // true = this node is infrastructure relay

    // Policies
    pub config_block_ads: bool,
    pub config_block_porn: bool,
    pub config_block_torrent: bool,
}

// Convert from actual Node model
impl From<&caramba_db::models::node::Node> for NodeInfo {
    fn from(node: &caramba_db::models::node::Node) -> Self {
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
            country_code: node.country_code.clone(),
            is_relay: node.is_relay,
            config_block_ads: node.config_block_ads,
            config_block_porn: node.config_block_porn,
            config_block_torrent: node.config_block_torrent,
        }
    }
}

// Convert from Node + Inbounds
impl NodeInfo {
    pub fn new(
        node: &caramba_db::models::node::Node,
        inbounds: Vec<caramba_db::models::network::Inbound>,
    ) -> Self {
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
            country_code: node.country_code.clone(),
            is_relay: node.is_relay,
            config_block_ads: node.config_block_ads,
            config_block_porn: node.config_block_porn,
            config_block_torrent: node.config_block_torrent,
        }
    }
}

// ─── Helper: Parse stream_settings JSON ───────────────────────────────────────

/// Parsed transport/security info from an inbound's stream_settings JSON
pub(crate) struct StreamInfo {
    pub(crate) network: String,  // tcp, ws, grpc, xhttp, quic
    pub(crate) security: String, // reality, tls, none
    pub(crate) sni: String,
    pub(crate) public_key: String, // Reality only
    pub(crate) short_id: String,   // Reality only
    pub(crate) fingerprint: String,
    pub(crate) ws_path: String,      // WebSocket path
    pub(crate) grpc_service: String, // gRPC serviceName
    pub(crate) flow: String,         // xtls-rprx-vision (Reality+TCP only)

    // XHTTP / Advanced settings
    pub(crate) packet_encoding: Option<String>, // packet-up / packetaddr
    pub(crate) x_padding_bytes: Option<String>, // 500-1200
    #[allow(dead_code)] // WIP: xmux/mux settings not yet emitted into configs
    pub(crate) xmux: Option<Value>, // JSON object for mux settings

    // Hysteria 2
    pub(crate) hy2_ports: Option<String>, // Port hopping range e.g. "20000-50000"
    pub(crate) hy2_obfs: Option<String>,  // Obfs password

    // TUIC v5
    pub(crate) tuic_congestion_control: Option<String>,
    pub(crate) tuic_zero_rtt_handshake: Option<bool>,
}

fn is_placeholder_sni(sni: &str) -> bool {
    let sni = sni.trim().to_ascii_lowercase();
    sni.is_empty() || sni == "www.google.com" || sni == "google.com" || sni == "drive.google.com"
}

// ─── Label & display helpers ──────────────────────────────────────────────────

/// Return a flag emoji for a 2-letter ISO country code
fn country_flag(code: Option<&str>) -> &'static str {
    match code.map(|s| s.trim()).unwrap_or("") {
        "RU" | "ru" => "🇷🇺",
        "DE" | "de" => "🇩🇪",
        "NL" | "nl" => "🇳🇱",
        "FI" | "fi" => "🇫🇮",
        "FR" | "fr" => "🇫🇷",
        "US" | "us" => "🇺🇸",
        "GB" | "gb" | "UK" | "uk" => "🇬🇧",
        "TR" | "tr" => "🇹🇷",
        "SE" | "se" => "🇸🇪",
        "LV" | "lv" => "🇱🇻",
        "LT" | "lt" => "🇱🇹",
        "EE" | "ee" => "🇪🇪",
        "KZ" | "kz" => "🇰🇿",
        "UA" | "ua" => "🇺🇦",
        "PL" | "pl" => "🇵🇱",
        "AT" | "at" => "🇦🇹",
        "CH" | "ch" => "🇨🇭",
        "JP" | "jp" => "🇯🇵",
        "SG" | "sg" => "🇸🇬",
        "HK" | "hk" => "🇭🇰",
        "AU" | "au" => "🇦🇺",
        "CA" | "ca" => "🇨🇦",
        "CZ" | "cz" => "🇨🇿",
        "MD" | "md" => "🇲🇩",
        _ => "🌐",
    }
}

/// Country flag emoji only — node name omitted since flag is sufficient
pub(crate) fn format_node_label(node: &NodeInfo) -> String {
    country_flag(node.country_code.as_deref()).to_string()
}

/// Compact protocol + transport label — user-friendly names instead of
/// protocol jargon.
pub(crate) fn format_proto_label(protocol: &str, si: &StreamInfo) -> String {
    match protocol.to_ascii_lowercase().as_str() {
        "vless" => match si.network.as_str() {
            "tcp" => match si.security.as_str() {
                "reality" => "Stealth",
                "tls" => "Secure",
                _ => "TCP",
            },
            "ws" => "WebSocket",
            "grpc" => "Stream",
            "xhttp" | "splithttp" => "XHTTP",
            "httpupgrade" => "HTTP",
            other => other,
        }
        .to_string(),
        "vmess" => "VMess".to_string(),
        "trojan" => match si.security.as_str() {
            "reality" => "Trojan·Stealth".to_string(),
            _ => "Trojan".to_string(),
        },
        "hysteria2" | "hy2" => {
            if si.hy2_obfs.is_some() {
                "Speed+".to_string()
            } else {
                "Speed".to_string()
            }
        }
        "tuic" => "TUIC".to_string(),
        "shadowsocks" | "ss" => "Shadow".to_string(),
        "amneziawg" => "Amnezia".to_string(),
        "naive" => "Naive".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

// ─── Sing-box outbound builder ────────────────────────────────────────────────

/// Build a single sing-box outbound JSON object for the given inbound/protocol.
/// `detour` – if Some, the connection goes through that relay outbound tag first.
/// Returns None for unknown protocols.
fn build_singbox_outbound(
    tag: &str,
    inbound: &caramba_db::models::network::Inbound,
    endpoint: &str,
    si: &StreamInfo,
    user_keys: &UserKeys,
    detour: Option<&str>,
) -> Option<serde_json::Value> {
    // Skip transports not reliably supported by sing-box:
    // - XHTTP/SplitHTTP: proprietary Xray-core transport
    // - gRPC: limited/broken support in sing-box 1.8+
    // Both still appear in V2Ray base64 and Clash configs.
    if matches!(si.network.as_str(), "xhttp" | "splithttp") {
        return None;
    }
    // AmneziaWG is hidden unless explicitly enabled (no client/server support yet).
    if inbound.protocol.eq_ignore_ascii_case("amneziawg") && !crate::utils::amneziawg_enabled() {
        return None;
    }

    let mut ob = json!({ "tag": tag });

    match inbound.protocol.as_str() {
        "vless" => {
            ob["type"] = json!("vless");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["uuid"] = json!(user_keys.user_uuid);
            // flow (xtls-rprx-vision) is only valid for Reality+TCP connections.
            // Setting it on non-Reality or non-TCP outbounds breaks some clients (Happ).
            // Omitting the field entirely (not even empty string) is safest.
            if !si.flow.is_empty() && si.security == "reality" && si.network == "tcp" {
                ob["flow"] = json!(si.flow);
            }

            let mut tls = json!({ "enabled": false });
            if si.security == "reality" {
                tls = json!({
                    "enabled": true,
                    "server_name": si.sni,
                    "reality": {
                        "enabled": true,
                        "public_key": si.public_key,
                        "short_id": si.short_id
                    },
                    "utls": { "enabled": true, "fingerprint": si.fingerprint }
                });
                // TLS fragment removed — Hiddify 4.0 doesn't support fragment as object in tls block.
                // sing-box native fragment is configured at outbound level, not inside tls.
                if false {
                    // Placeholder — re-enable when Hiddify supports it
                    tls["fragment"] = json!({
                        "enabled": true,
                        "size": "1-500",
                        "sleep": "0-5"
                    });
                }
            } else if si.security == "tls" {
                // Сервер использует самоподписанный сертификат — клиент должен пропустить проверку
                tls = json!({
                    "enabled": true,
                    "server_name": si.sni,
                    "insecure": true,
                    "utls": { "enabled": true, "fingerprint": si.fingerprint }
                });
            }
            ob["tls"] = tls;

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
                "httpupgrade" => {
                    // Standard HTTP Upgrade — fully supported by sing-box.
                    // Multiplex removed for broader client compatibility (Happ doesn't support smux).
                    ob["transport"] = json!({
                        "type": "httpupgrade",
                        "path": si.ws_path,
                        "host": si.sni
                    });
                }
                "xhttp" | "splithttp" => {
                    // Unreachable: the early guard at the top of this function already
                    // returns None for xhttp/splithttp before we get here.
                    // Kept as an explicit no-op match arm to document the intent.
                }
                _ => {}
            }
        }
        "vmess" => {
            ob["type"] = json!("vmess");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["uuid"] = json!(user_keys.user_uuid);
            ob["security"] = json!("auto");
            ob["alter_id"] = json!(0);
            if si.security == "tls" {
                // Server uses a self-signed certificate for plain-TLS inbounds, so
                // the client must skip verification or the handshake is rejected.
                ob["tls"] = json!({
                    "enabled": true,
                    "server_name": si.sni,
                    "insecure": true,
                    "utls": { "enabled": true, "fingerprint": si.fingerprint }
                });
            }
            if si.network == "ws" {
                ob["transport"] = json!({
                    "type": "ws",
                    "path": si.ws_path,
                    "headers": { "Host": si.sni }
                });
            } else if si.network == "grpc" {
                ob["transport"] = json!({ "type": "grpc", "service_name": si.grpc_service });
            }
        }
        "trojan" => {
            ob["type"] = json!("trojan");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["password"] = json!(user_keys.user_uuid);
            let mut tls = json!({ "enabled": true, "server_name": si.sni });
            if si.security == "reality" {
                tls["reality"] = json!({
                    "enabled": true,
                    "public_key": si.public_key,
                    "short_id": si.short_id
                });
                tls["utls"] = json!({ "enabled": true, "fingerprint": si.fingerprint });
            } else {
                // Plain TLS: server presents a self-signed certificate, so the
                // client must skip verification (Reality does its own auth above).
                tls["insecure"] = json!(true);
                tls["utls"] = json!({ "enabled": true, "fingerprint": si.fingerprint });
            }
            ob["tls"] = tls;
            if si.network == "ws" {
                ob["transport"] = json!({
                    "type": "ws",
                    "path": si.ws_path,
                    "headers": { "Host": si.sni }
                });
            } else if si.network == "grpc" {
                ob["transport"] = json!({ "type": "grpc", "service_name": si.grpc_service });
            }
        }
        "hysteria2" | "hy2" => {
            ob["type"] = json!("hysteria2");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["password"] = json!(user_keys.hy2_password);
            ob["tls"] = json!({
                "enabled": true,
                "server_name": si.sni,
                "insecure": true,
                "alpn": ["h3"]
            });
            if let Some(obfs) = &si.hy2_obfs {
                ob["obfs"] = json!({ "type": "salamander", "password": obfs });
            }
            if let Some(ports) = &si.hy2_ports {
                // server_ports must be a JSON array of port-range strings (sing-box 1.11+).
                // Skip if it's just the same single port as server_port.
                let single = inbound.listen_port.to_string();
                if ports != &single {
                    // Convert comma/space separated ranges to array
                    let ranges: Vec<&str> = ports
                        .split([',', ' '])
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect();
                    ob["server_ports"] = json!(ranges);
                }
            }
        }
        "tuic" => {
            ob["type"] = json!("tuic");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["uuid"] = json!(user_keys.user_uuid);
            ob["password"] = json!(user_keys.hy2_password);
            ob["congestion_control"] =
                json!(si.tuic_congestion_control.as_deref().unwrap_or("bbr"));
            ob["zero_rtt_handshake"] = json!(si.tuic_zero_rtt_handshake.unwrap_or(false));
            ob["tls"] = json!({
                "enabled": true,
                "server_name": si.sni,
                "alpn": ["h3"],
                "insecure": true
            });
        }
        "shadowsocks" | "ss" => {
            ob["type"] = json!("shadowsocks");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["method"] = json!(parse_ss_method(&inbound.settings));
            ob["password"] = json!(parse_ss_password(&inbound.settings, &user_keys.user_uuid));
        }
        "naive" => {
            ob["type"] = json!("naive");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["username"] = json!(user_keys.user_uuid);
            ob["password"] = json!(user_keys.hy2_password);
            // NaiveProxy TLS only supports: server_name, certificate, certificate_path, ech.
            // alpn, utls, insecure etc. are NOT supported and cause parse errors.
            ob["tls"] = json!({
                "enabled": true,
                "server_name": si.sni
            });
        }
        "amneziawg" => {
            let client_id = user_keys
                .hy2_password
                .split(':')
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let local_address = format!("10.10.0.{}/32", (client_id % 250) + 2);
            ob["type"] = json!("wireguard");
            ob["server"] = json!(endpoint);
            ob["server_port"] = json!(inbound.listen_port);
            ob["local_address"] = json!([local_address]);
            ob["private_key"] = json!(user_keys._awg_private_key.clone().unwrap_or_default());
            ob["peer_public_key"] = json!(si.public_key);
            ob["mtu"] = json!(1280);
            if let Ok(awg) = serde_json::from_str::<serde_json::Value>(&inbound.settings)
                && let Some(jc) = awg.get("jc")
            {
                ob["reserved"] = json!([
                    jc.as_u64().unwrap_or(0),
                    awg["jmin"].as_u64().unwrap_or(0),
                    awg["jmax"].as_u64().unwrap_or(0)
                ]);
            }
        }
        _ => return None,
    }

    if let Some(d) = detour {
        ob["detour"] = json!(d);
    }

    Some(ob)
}

// ─── Relay outbound factory ───────────────────────────────────────────────────

/// Ensure a relay outbound exists in `outbounds` for the given relay node.
/// Uses a cache keyed by relay node address to avoid duplicate entries.
/// Picks the best available relay inbound: prefers SS2022, then Hy2, then any.
/// Returns the tag of the relay outbound, or None if no suitable inbound found.
fn ensure_relay_outbound(
    relay: &NodeInfo,
    user_keys: &UserKeys,
    outbounds: &mut Vec<serde_json::Value>,
    cache: &mut std::collections::HashMap<String, String>,
) -> Option<String> {
    let cache_key = relay.address.clone();
    if let Some(existing) = cache.get(&cache_key) {
        return Some(existing.clone());
    }

    // Priority order for relay transport: ss/ss2022 > hysteria2 > vless > any
    let priority_order = ["shadowsocks", "ss", "hysteria2", "hy2", "vless", "trojan"];
    let pick = priority_order
        .iter()
        .find_map(|proto| {
            relay
                .inbounds
                .iter()
                .find(|ib| ib.enable && ib.protocol == *proto)
        })
        .or_else(|| relay.inbounds.iter().find(|ib| ib.enable));

    let ri = pick?;
    let r_si = parse_stream_settings(&ri.stream_settings, relay);
    let r_tag = format!("relay {}", country_flag(relay.country_code.as_deref()));

    let ob = build_singbox_outbound(&r_tag, ri, &relay.address, &r_si, user_keys, None)?;

    outbounds.push(ob);
    cache.insert(cache_key, r_tag.clone());
    Some(r_tag)
}

pub(crate) fn parse_stream_settings(raw: &str, node: &NodeInfo) -> StreamInfo {
    // 1. Parse into strongly-typed struct for robust alias handling (SNI, Settings, etc.)
    let settings: caramba_db::models::network::StreamSettings =
        serde_json::from_str(raw).unwrap_or_default();

    // 2. Parse into generic Value for fields not yet in StreamSettings struct (e.g. fingerprint, tuic/hy2 extras)
    let v: Value = serde_json::from_str(raw).unwrap_or(json!({}));

    let network = settings
        .network
        .clone()
        .or_else(|| {
            v.get("network")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "tcp".to_string());
    // Определяем security: берём из stream_settings, если не задано — определяем по наличию
    // realitySettings/tlsSettings, иначе "none".
    // ВАЖНО: дефолт НЕ может быть "reality" — это ломает ws/grpc/httpupgrade инбаунды.
    let security_raw = settings.security.clone().or_else(|| {
        v.get("security")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
    });
    let security = security_raw.unwrap_or_else(|| {
        // Автоопределение: reality → tls → none
        if settings.reality_settings.is_some()
            || v.get("realitySettings").is_some()
            || v.get("reality_settings").is_some()
        {
            "reality".to_string()
        } else if settings.tls_settings.is_some() || v.get("tlsSettings").is_some() {
            "tls".to_string()
        } else {
            "none".to_string()
        }
    });

    let inbound_sni = extract_sni_from_settings(&settings)
        .or_else(|| extract_sni_from_raw(&v))
        .filter(|s| !is_placeholder_sni(s));
    let node_sni = node
        .reality_sni
        .as_ref()
        .filter(|s| !is_placeholder_sni(s))
        .cloned();

    let is_reality = security.eq_ignore_ascii_case("reality")
        || settings.reality_settings.is_some()
        || v.get("realitySettings").is_some()
        || v.get("reality_settings").is_some();

    // For Reality we must prefer node-level SNI (effective runtime value on the node),
    // otherwise subscriptions can drift from active node config and fail handshake.
    let sni = if is_reality {
        node_sni
            .clone()
            .or(inbound_sni.clone())
            .unwrap_or_else(|| "www.google.com".to_string())
    } else {
        inbound_sni
            .or(node_sni)
            .unwrap_or_else(|| "www.google.com".to_string())
    };

    // Reality Keys
    let public_key = settings
        .reality_settings
        .as_ref()
        .and_then(|r| r.public_key.clone())
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| node.reality_public_key.clone().unwrap_or_default());

    let short_id = settings
        .reality_settings
        .as_ref()
        .and_then(|r| r.short_ids.first().cloned())
        .unwrap_or_else(|| node.reality_short_id.clone().unwrap_or_default());

    // Fingerprint (Not in StreamSettings yet, extract manually)
    let fingerprint = v
        .get("realitySettings")
        .or_else(|| v.get("reality_settings"))
        .and_then(|r| r.get("fingerprint"))
        .and_then(|s| s.as_str())
        .unwrap_or("chrome")
        .to_string();

    // WebSocket settings
    let ws_path = settings
        .ws_settings
        .as_ref()
        .map(|w| w.path.clone())
        .or_else(|| settings.xhttp_settings.as_ref().map(|x| x.path.clone()))
        .or_else(|| {
            settings
                .http_upgrade_settings
                .as_ref()
                .map(|h| h.path.clone())
        })
        .or_else(|| {
            v.get("wsSettings")
                .or_else(|| v.get("ws_settings"))
                .and_then(|w| w.get("path"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            v.get("xhttpSettings")
                .or_else(|| v.get("xhttp_settings"))
                .and_then(|x| x.get("path"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            v.get("httpUpgradeSettings")
                .or_else(|| v.get("http_upgrade_settings"))
                .and_then(|h| h.get("path"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
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
    } else if security == "reality" && network == "tcp" {
        // xtls-rprx-vision is only emitted by the server for Reality+TCP. Inferring
        // it for plain TLS (server sets no flow there) makes the client send a flow
        // the server doesn't expect -> auth mismatch / connection refused, and it
        // also leaks into the vless:// URI and Clash proxy outputs.
        "xtls-rprx-vision".to_string()
    } else {
        String::new()
    };

    // XHTTP / Advanced Parsing
    let packet_encoding = settings.packet_encoding.clone().or_else(|| {
        v.get("packet_encoding")
            .or_else(|| v.get("packetEncoding"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
    });

    // x_padding_bytes not in StreamSettings?
    let x_padding_bytes = v
        .get("x_padding_bytes")
        .or_else(|| v.get("xPaddingBytes"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    let xmux = v.get("xmux").cloned();

    // Hysteria 2 Specifics
    let hy2_settings = v
        .get("hysteria2Settings")
        .or_else(|| v.get("hysteria2_settings"));
    let hy2_ports = hy2_settings
        .and_then(|h| h.get("ports").or_else(|| h.get("server_ports")))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let hy2_obfs = hy2_settings
        .and_then(|h| h.get("obfs_password").or_else(|| h.get("obfsPassword")))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    // TUIC Specifics
    let tuic_settings = v.get("tuicSettings").or_else(|| v.get("tuic_settings"));
    let tuic_congestion_control = tuic_settings
        .and_then(|t| t.get("congestion_control"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let tuic_zero_rtt_handshake = tuic_settings
        .and_then(|t| t.get("zero_rtt_handshake"))
        .and_then(|v| v.as_bool());

    StreamInfo {
        network,
        security,
        sni,
        public_key,
        short_id,
        fingerprint,
        ws_path,
        grpc_service,
        flow,
        packet_encoding,
        x_padding_bytes,
        xmux,
        hy2_ports,
        hy2_obfs,
        tuic_congestion_control,
        tuic_zero_rtt_handshake,
    }
}

fn extract_sni_from_settings(
    settings: &caramba_db::models::network::StreamSettings,
) -> Option<String> {
    if let Some(reality) = &settings.reality_settings {
        // Priority: server_names[0] -> server_name (singular)
        reality
            .server_names
            .first()
            .cloned()
            .or_else(|| reality.server_name.clone())
            .filter(|s: &String| !s.is_empty())
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

fn extract_sni_from_raw(v: &Value) -> Option<String> {
    let reality_sni = v
        .get("realitySettings")
        .or_else(|| v.get("reality_settings"))
        .and_then(|r| {
            r.get("serverNames")
                .and_then(|arr| arr.as_array())
                .and_then(|arr| arr.first())
                .and_then(|s| s.as_str())
                .or_else(|| r.get("serverName").and_then(|s| s.as_str()))
        })
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let tls_sni = v
        .get("tlsSettings")
        .or_else(|| v.get("tls_settings"))
        .and_then(|t| {
            t.get("serverName")
                .or_else(|| t.get("server_name"))
                .and_then(|s| s.as_str())
        })
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    reality_sni.or(tls_sni)
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
            if let Some(username) = user
                .get("username")
                .or(user.get("name"))
                .and_then(|u| u.as_str())
                && username == user_uuid
            {
                return user
                    .get("password")
                    .and_then(|p| p.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
        // Fallback: if list has 1 item and we didn't match (maybe single user mode but ID mismatch?), use it.
        if users.len() == 1 {
            return users[0]
                .get("password")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
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
    _relay_nodes: &[NodeInfo],
) -> Result<String> {
    let mut links = Vec::new();

    for node in nodes {
        // Skip pure relay infrastructure nodes.
        if node.is_relay {
            continue;
        }

        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable {
                    continue;
                }
                if inbound.protocol.eq_ignore_ascii_case("amneziawg")
                    && !crate::utils::amneziawg_enabled()
                {
                    continue;
                }
                let si = parse_stream_settings(&inbound.stream_settings, node);

                // XHTTP / SplitHTTP IS valid in V2Ray base64 links because those
                // links are consumed by Xray-core clients (v2rayN, NekoRay w/ Xray
                // backend, etc.) that natively support it.  We deliberately include
                // it here and exclude it from sing-box/Clash generators.
                // No `continue` needed — fall through to match below.

                let proto_label = format_proto_label(&inbound.protocol, &si);
                let node_label = format_node_label(node);
                // Relay suffix so users know this path goes via a Russian relay.
                let relay_suffix = if node.relay_info.is_some() {
                    " ↪"
                } else {
                    ""
                };
                let label_raw = format!("{} {}{}", node_label, proto_label, relay_suffix);
                let label = urlencoding::encode(&label_raw);

                // XHTTP / SplitHTTP links are Xray-core only — skip for sing-box
                // but generate them here because V2Ray base64 is consumed by Xray
                // clients (v2rayN, NekoRay/Xray backend, etc.) which support it.
                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut params = vec![
                            "encryption=none".to_string(),
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
                            "ws" => {
                                params.push(format!("path={}", urlencoding::encode(&si.ws_path)))
                            }
                            "grpc" => params.push(format!("serviceName={}", si.grpc_service)),
                            "httpupgrade" => {
                                params.push(format!("path={}", urlencoding::encode(&si.ws_path)));
                                // no mode= for httpupgrade; it is not an xhttp parameter
                            }
                            "xhttp" | "splithttp" => {
                                params.push(format!("path={}", urlencoding::encode(&si.ws_path)));
                                // mode=auto tells Xray to pick the best split strategy
                                params.push("mode=auto".to_string());
                                // xPaddingBytes randomises request size to defeat traffic
                                // analysis — recommended for XHTTP.
                                if let Some(pad) = &si.x_padding_bytes {
                                    params.push(format!("xPaddingBytes={}", pad));
                                } else {
                                    use rand::Rng;
                                    let pad = rand::rng().random_range(500u32..=1200);
                                    params.push(format!("xPaddingBytes={}", pad));
                                }
                            }
                            _ => {}
                        }
                        // Frontend Masquerading Logic
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        // If masquerading, we MUST use the real SNI in the header/TLS config
                        // which is already handled by `si.sni` (streams settings)
                        // But the connection address (host/ip) in the link should be the frontend.

                        links.push(format!(
                            "vless://{}@{}:{}?{}#{}",
                            user_keys.user_uuid,
                            host,
                            inbound.listen_port, // Use host (frontend or node IP)
                            params.join("&"),
                            label
                        ));
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
                            "ws" => {
                                params.push(format!("path={}", urlencoding::encode(&si.ws_path)))
                            }
                            "grpc" => params.push(format!("serviceName={}", si.grpc_service)),
                            _ => {}
                        }
                        // Trojan uses user_uuid as password
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        links.push(format!(
                            "trojan://{}@{}:{}?{}#{}",
                            user_keys.user_uuid,
                            host,
                            inbound.listen_port, // Masquerading
                            params.join("&"),
                            label
                        ));
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
                        links.push(format!(
                            "ss://{}@{}:{}#{}",
                            userinfo, host, inbound.listen_port, label
                        ));
                    }
                    "hysteria2" | "hy2" => {
                        let mut params = vec![format!("sni={}", si.sni), "insecure=1".to_string()];
                        if let Some(ports) = &si.hy2_ports {
                            params.push(format!("mport={}", ports));
                        }
                        if let Some(obfs) = &si.hy2_obfs {
                            params.push("obfs=salamander".to_string());
                            params.push(format!("obfs-password={}", obfs));
                        }

                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        links.push(format!(
                            "hysteria2://{}@{}:{}?{}#{}",
                            user_keys.hy2_password,
                            host,
                            inbound.listen_port, // Masquerading
                            params.join("&"),
                            label
                        ));
                    }
                    "amneziawg" => {
                        let client_id = user_keys
                            .hy2_password
                            .split(':')
                            .next()
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
                        let local_address = format!("10.10.0.{}/32", (client_id % 250) + 2);

                        // wireguard://private_key@server:port?public_key=...&preshared_key=...#label
                        // Note: Some clients use 'address' param for local address
                        let mut params = vec![
                            format!("public_key={}", si.public_key),
                            format!("address={}", urlencoding::encode(&local_address)),
                        ];

                        // Add AmneziaWG obfuscation as non-standard params (supported by some clients/converted by users)
                        if let Ok(awg_obj) =
                            serde_json::from_str::<serde_json::Value>(&inbound.settings)
                        {
                            for field in ["jc", "jmin", "jmax", "s1", "s2", "h1", "h2", "h3", "h4"]
                            {
                                if let Some(v) = awg_obj.get(field) {
                                    params.push(format!("{}={}", field, v));
                                }
                            }
                        }

                        links.push(format!(
                            "wireguard://{}@{}:{}?{}#{}",
                            user_keys._awg_private_key.clone().unwrap_or_default(),
                            node.address,
                            inbound.listen_port,
                            params.join("&"),
                            label
                        ));
                    }
                    "naive" => {
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        links.push(format!(
                            "naive+https://{}:{}@{}:{}?sni={}#{}",
                            user_keys.user_uuid,
                            user_keys.hy2_password,
                            host,
                            inbound.listen_port,
                            si.sni,
                            label
                        ));
                    }
                    "tuic" => {
                        let host = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let params = [
                            format!("sni={}", si.sni),
                            format!(
                                "congestion_control={}",
                                si.tuic_congestion_control.as_deref().unwrap_or("bbr")
                            ),
                            "alpn=h3".to_string(),
                        ];
                        links.push(format!(
                            "tuic://{}:{}@{}:{}?{}#{}",
                            user_keys.user_uuid,
                            user_keys.hy2_password,
                            host,
                            inbound.listen_port,
                            params.join("&"),
                            label
                        ));
                    }
                    _ => {
                        // Unknown protocol, skip
                    }
                }
            }
        }
        // Legacy fallback: VLESS Reality
        else if let Some(port) = node.reality_port {
            let host = node.frontend_url.as_deref().unwrap_or(&node.address);
            let vless_link = format!(
                "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
                user_keys.user_uuid,
                host,
                port,
                node.reality_sni
                    .as_ref()
                    .unwrap_or(&"www.google.com".to_string()),
                node.reality_public_key.as_ref().unwrap_or(&"".to_string()),
                node.reality_short_id.as_ref().unwrap_or(&"".to_string()),
                urlencoding::encode(&format!("{} VLESS", node.name))
            );
            links.push(vless_link);
            // Legacy Hysteria2
            if let Some(port) = node.hy2_port {
                links.push(format!(
                    "hysteria2://{}@{}:{}?sni={}&insecure=1#{}",
                    user_keys.hy2_password,
                    host,
                    port,
                    node.hy2_sni.as_deref().unwrap_or(host),
                    urlencoding::encode(&format!("{} HY2", node.name))
                ));
            }
        }
    }

    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(links.join("\n")))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Clash YAML Config Generation
// ═══════════════════════════════════════════════════════════════════════════════

/// Пин сертификата узла для mihomo (`fingerprint`). Пока всегда `None`.
///
/// Панели неоткуда взять отпечаток: сертификат генерирует САМ узел
/// (`caramba-node`, `ensure_self_signed_cert`, rcgen), в heartbeat уезжает
/// `CertificateStatus { sni, valid, expires_at, error }` — отпечатка там нет,
/// а колонка `nodes.certificates_status` объявлена в init-миграции и не
/// пишется ни одной строкой кода (в проде NULL у всех трёх узлов).
///
/// Что должно приехать, чтобы функция начала возвращать `Some` (порядок важен):
///  1. `caramba_shared::api::CertificateStatus` получает
///     `#[serde(default)] pub sha256: Option<String>` — узел считает
///     `sha256(cert.Raw)` там же, где сейчас пишет `cert.pem`;
///  2. панель складывает отпечаток в отдельную колонку `nodes` (миграция),
///     `NodeInfo` протаскивает его сюда;
///  3. ЖИЗНЕННЫЙ ЦИКЛ. Узел перевыпускает сертификат при КАЖДОЙ смене набора
///     `server_name` (`cert_covers_domains` → `ensure_self_signed_cert`), а
///     ротация SNI — штатная защита от блокировок. mihomo принимает РОВНО ОДИН
///     отпечаток (`NewFingerprintVerifier`, 32 байта), окна перекрытия нет:
///     сразу после ротации всякая закешированная подписка со старым пином
///     ломается наглухо, пока клиент её не перескачает. Поэтому пин требует
///     либо долгоживущего сертификата, чьи SAN покрывают весь пул SNI, либо
///     собственного CA на узел (тогда пинится корень, а не лист).
///
/// Без пункта 3 пин строго хуже текущего поведения: он превращает деградацию
/// в отказ именно на том механизме, который существует ради выживания.
fn node_cert_pin(_node: &NodeInfo) -> Option<&str> {
    None
}

/// Кладёт в clash-прокси гарантию доверия к TLS-сертификату узла.
///
/// Узлы предъявляют САМОПОДПИСАННЫЙ сертификат (`CN == SNI`, issuer == subject,
/// проверено на 85.215.196.151 и 158.69.213.88). Без одной из этих двух опций
/// mihomo валит рукопожатие проверкой цепочки, и прокси мёртв у любого
/// clash-клиента — у нашего, у Happ, у Hiddify.
///
/// `fingerprint` (пин SHA-256 листа) строго сильнее: `ca.GetTLSConfig` ставит
/// `InsecureSkipVerify = true` И вешает `VerifyConnection`, сверяющий
/// `sha256(cert.Raw)` — то есть чужой сертификат не пройдёт. `skip-cert-verify`
/// принимает любой. Ветка с пином не мёртвая: она покрыта тестом и включится,
/// как только `node_cert_pin` научится возвращать значение.
///
/// Reality сюда не попадает: он аутентифицирует сервер по X25519-ключу, TLS-цепочка
/// в нём вообще не проверяется.
fn apply_clash_tls_trust(proxy: &mut Value, cert_pin: Option<&str>) {
    match cert_pin {
        Some(fp) => {
            // mihomo терпит двоеточия и сам их вырезает (component/ca/fingerprint.go).
            proxy["fingerprint"] = json!(fp);
        }
        None => {
            proxy["skip-cert-verify"] = json!(true);
        }
    }
}

/// Generate Clash YAML config (multi-protocol)
pub fn generate_clash_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
    _relay_nodes: &[NodeInfo],
) -> Result<String> {
    let mut proxies = Vec::new();

    // Единственный источник правды о составе групп — `proxies`. Здесь копится
    // только ПРИЗНАК пути (идёт ли выход через релей) для имён, которые
    // генератор рассматривал. Само членство в группах строится ниже фильтром
    // по фактически выпущенным прокси, поэтому имя, под которым прокси не
    // выпустился, физически не может попасть ни в одну группу.
    let mut relay_path_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for node in nodes {
        // Skip pure infrastructure relay nodes.
        if node.is_relay {
            continue;
        }

        // Отпечаток общий на узел: сертификат один на все его plain-TLS порты
        // (`/etc/sing-box/certs/cert.pem`), SNI перечислены в нём как SAN.
        let cert_pin = node_cert_pin(node);

        if !node.inbounds.is_empty() {
            for inbound in &node.inbounds {
                if !inbound.enable {
                    continue;
                }
                let si = parse_stream_settings(&inbound.stream_settings, node);
                let proto_label = format_proto_label(&inbound.protocol, &si);
                let node_label = format_node_label(node);
                let is_relay_path = node.relay_info.is_some();
                let relay_suffix = if is_relay_path { " ↪" } else { "" };
                let name = format!("{} {}{}", node_label, proto_label, relay_suffix);

                // Clash/mihomo path: the mihomo client speaks AmneziaWG natively, so
                // this emission is gated on the CLIENT flag, independent of the strict
                // sing-box `amneziawg_enabled()` gate that protects node configs. This
                // only adds a `wireguard` proxy to the subscription; it never writes an
                // AmneziaWG inbound to a sing-box node. The proxy is inert unless a real
                // AmneziaWG server runs on the node (see AmneziaWG helper notes).
                if inbound.protocol.eq_ignore_ascii_case("amneziawg")
                    && !crate::utils::amneziawg_client_enabled()
                {
                    continue;
                }

                // XHTTP / SplitHTTP is a proprietary Xray-core transport that
                // Clash Meta does not support.  Skip it here; it still appears
                // in the V2Ray base64 output for Xray-native clients.
                // NOTE: xhttp/splithttp is a *transport* (si.network), not a protocol —
                // inbound.protocol will always be "vless"/"vmess"/etc., so we must
                // check si.network here, not inbound.protocol.
                if matches!(si.network.as_str(), "xhttp" | "splithttp") {
                    continue;
                }

                // Это признак пути, а не запись в группу: выпустит ли match
                // ниже прокси с этим именем, здесь ещё неизвестно.
                if is_relay_path {
                    relay_path_names.insert(name.clone());
                }

                match inbound.protocol.as_str() {
                    "vless" => {
                        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let mut proxy = json!({
                            "name": name,
                            "type": "vless",
                            "server": endpoint,
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
                            apply_clash_tls_trust(&mut proxy, cert_pin);
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
                        if si.network == "httpupgrade" {
                            proxy["http-upgrade-opts"] = json!({
                                "path": si.ws_path,
                                "host": si.sni
                            });
                        }
                        proxies.push(proxy);
                    }
                    "vmess" => {
                        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let mut proxy = json!({
                            "name": name,
                            "type": "vmess",
                            "server": endpoint,
                            "port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                            "alterId": 0,
                            "cipher": "auto",
                            "network": si.network,
                        });
                        if si.security == "tls" {
                            proxy["tls"] = json!(true);
                            proxy["servername"] = json!(si.sni);
                            apply_clash_tls_trust(&mut proxy, cert_pin);
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
                        if si.network == "httpupgrade" {
                            proxy["http-upgrade-opts"] = json!({
                                "path": si.ws_path,
                                "host": si.sni
                            });
                        }
                        proxies.push(proxy);
                    }
                    "trojan" => {
                        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let mut proxy = json!({
                            "name": name,
                            "type": "trojan",
                            "server": endpoint,
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
                        } else {
                            // У mihomo trojan всегда поверх TLS, `security` тут
                            // различает только Reality и обычный TLS.
                            apply_clash_tls_trust(&mut proxy, cert_pin);
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
                        if si.network == "httpupgrade" {
                            proxy["network"] = json!("httpupgrade");
                            proxy["http-upgrade-opts"] = json!({
                                "path": si.ws_path,
                                "host": si.sni
                            });
                        }
                        proxies.push(proxy);
                    }
                    "shadowsocks" | "ss" => {
                        let method = parse_ss_method(&inbound.settings);
                        let password = parse_ss_password(&inbound.settings, &user_keys.user_uuid);
                        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);
                        proxies.push(json!({
                            "name": name,
                            "type": "ss",
                            "server": endpoint,
                            "port": inbound.listen_port,
                            "cipher": method,
                            "password": password,
                        }));
                    }
                    "hysteria2" | "hy2" => {
                        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let mut hy2_proxy = json!({
                            "name": name,
                            "type": "hysteria2",
                            "server": endpoint,
                            "port": inbound.listen_port,
                            "password": user_keys.hy2_password,
                            "sni": si.sni,
                        });
                        apply_clash_tls_trust(&mut hy2_proxy, cert_pin);
                        if let Some(obfs) = &si.hy2_obfs {
                            hy2_proxy["obfs"] = json!("salamander");
                            hy2_proxy["obfs-password"] = json!(obfs);
                        }
                        proxies.push(hy2_proxy);
                    }
                    "tuic" => {
                        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);
                        let mut tuic_proxy = json!({
                            "name": name,
                            "type": "tuic",
                            "server": endpoint,
                            "port": inbound.listen_port,
                            "uuid": user_keys.user_uuid,
                            "password": user_keys.hy2_password,
                            "congestion-controller": si.tuic_congestion_control.as_deref().unwrap_or("bbr"),
                            "alpn": ["h3"]
                        });
                        apply_clash_tls_trust(&mut tuic_proxy, cert_pin);
                        proxies.push(tuic_proxy);
                    }
                    "amneziawg" => {
                        let client_id = user_keys
                            .hy2_password
                            .split(':')
                            .next()
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
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

                        // mihomo (Clash.Meta) reads the AmneziaWG obfuscation under the
                        // nested `amnezia-wg-option` key (adapter/outbound/wireguard.go:
                        // `AmneziaWGOption *AmneziaWGOption proxy:"amnezia-wg-option"`).
                        // Inside it jc/jmin/jmax/s1..s4 are ints, but h1..h4 are STRINGS.
                        // The node stores h1..h4 as JSON numbers (config.rs h1: u32), so we
                        // stringify them here, otherwise mihomo's `proxy:` decoder rejects
                        // the proxy and the obfuscation silently degrades to plain WireGuard.
                        if let Ok(awg_obj) =
                            serde_json::from_str::<serde_json::Value>(&inbound.settings)
                        {
                            let mut opts = json!({});
                            for field in ["jc", "jmin", "jmax", "s1", "s2", "s3", "s4"] {
                                if let Some(v) = awg_obj.get(field)
                                    && !v.is_null()
                                {
                                    opts[field] = v.clone();
                                }
                            }
                            for field in ["h1", "h2", "h3", "h4"] {
                                if let Some(v) = awg_obj.get(field)
                                    && !v.is_null()
                                {
                                    // Force string form for mihomo (numbers fail to decode).
                                    let s = match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    };
                                    opts[field] = json!(s);
                                }
                            }
                            if opts.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                                proxy["amnezia-wg-option"] = opts;
                            }
                        }
                        proxies.push(proxy);
                    }
                    // mihomo не знает исходящего `naive`: разбор `type:` в
                    // adapter/parser.go перечисляет ss/ssr/socks5/http/vmess/
                    // vless/snell/trojan/hysteria/hysteria2/wireguard/tuic/
                    // shadowquic/gost-relay/ssh/mieru/anytls/… — ветки `naive`
                    // среди них нет. Подменить его на `http` тоже нельзя:
                    // adapter/outbound/http.go шлёт CONNECT строкой
                    // "HTTP/1.1", без ALPN h2 и без набивки, а naive-сервер
                    // ждёт именно HTTP/2 CONNECT. Поэтому инбаунд опускается —
                    // ровно как его опускает каталог CSM (csm/catalog_store.rs,
                    // NAIVE_NOT_IN_CLASH; 02-SPEC.md 4.4.1 требует, чтобы тело
                    // Clash и каталог описывали один и тот же набор прокси).
                    "naive" => {
                        tracing::debug!(
                            node = %node.name,
                            inbound = inbound.id,
                            "clash: naive пропущен, у mihomo нет такого исходящего"
                        );
                    }
                    other => {
                        tracing::debug!(
                            node = %node.name,
                            inbound = inbound.id,
                            protocol = %other,
                            "clash: протокол вне словаря генератора, прокси не выпущен"
                        );
                    }
                }
            }
        }
        // Legacy fallback
        else if node.reality_port.is_some() {
            let legacy_name = format!("{} Reality", format_node_label(node));
            proxies.push(json!({
                "name": legacy_name,
                "type": "vless",
                "server": node.frontend_url.as_deref().unwrap_or(&node.address),
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
        if let Some(hy2_port) = node.hy2_port.filter(|_| !node.is_relay) {
            let hy2_legacy_name = format!("{} hy2", format_node_label(node));
            proxies.push(json!({
                "name": hy2_legacy_name,
                "type": "hysteria2",
                "server": node.frontend_url.as_deref().unwrap_or(&node.address),
                "port": hy2_port,
                "password": user_keys.hy2_password,
                "sni": node.hy2_sni.as_ref().unwrap_or(&node.address),
                "skip-cert-verify": true
            }));
        }
    }

    let all_names: Vec<String> = proxies
        .iter()
        .filter_map(|p| p["name"].as_str().map(|s| s.to_string()))
        .collect();

    // Членство в группах выводится ИЗ `all_names`, то есть из уже выпущенных
    // прокси. Раньше здесь стояли два параллельных списка, которые
    // наполнялись ДО match по протоколу: `naive` попадал в Auto-Relay, не
    // существуя в `proxies:`, а mihomo такую группу отвергает целиком. Теперь
    // оба множества — подмножества `all_names`, и разойтись с `proxies:` они
    // не могут.
    //
    // Ключ — имя прокси, а уникальности имён генератор не гарантирует
    // (02-SPEC.md 4.5: уникализатора, как в sing-box `unique_tag`, тут нет).
    // При совпадении имён обе копии попадут в одну корзину; для mihomo
    // дубликат имени и так фатален, так что чинить это надо уникализатором,
    // а не здесь.
    let relay_names: Vec<String> = all_names
        .iter()
        .filter(|n| relay_path_names.contains(*n))
        .cloned()
        .collect();
    let direct_names: Vec<String> = all_names
        .iter()
        .filter(|n| !relay_path_names.contains(*n))
        .cloned()
        .collect();

    // Build proxy groups.
    let has_relay = !relay_names.is_empty();
    let has_direct = !direct_names.is_empty();

    let mut selector_list = vec!["Auto-All".to_string()];
    if has_relay {
        selector_list.push("Auto-Relay".to_string());
    }
    if has_direct {
        selector_list.push("Auto-Direct".to_string());
    }
    selector_list.extend(all_names.clone());

    let mut proxy_groups: Vec<serde_json::Value> = vec![
        json!({
            "name": "CARAMBA",
            "type": "select",
            "proxies": selector_list
        }),
        json!({
            "name": "Auto-All",
            "type": "url-test",
            "proxies": all_names,
            "url": "https://www.gstatic.com/generate_204",
            "interval": 180,
            "tolerance": 50
        }),
    ];

    if has_relay {
        proxy_groups.push(json!({
            "name": "Auto-Relay",
            "type": "url-test",
            "proxies": relay_names,
            "url": "https://www.gstatic.com/generate_204",
            "interval": 180,
            "tolerance": 50
        }));
    }

    if has_direct {
        proxy_groups.push(json!({
            "name": "Auto-Direct",
            "type": "url-test",
            "proxies": direct_names,
            "url": "https://www.gstatic.com/generate_204",
            "interval": 180,
            "tolerance": 50
        }));
    }

    let config = json!({
        "mixed-port": 7890,
        "allow-lan": false,
        "mode": "rule",
        "log-level": "info",
        "ipv6": false,
        "proxies": proxies,
        "proxy-groups": proxy_groups,
        "rules": [
            "GEOIP,CN,DIRECT",
            "GEOSITE,CN,DIRECT",
            "MATCH,CARAMBA"
        ]
    });

    Ok(serde_yaml::to_string(&config)?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sing-box JSON Config Generation
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate a full Sing-box client JSON config with:
///
/// - **Direct** outbounds for every enabled inbound on every node.
/// - **Relay-chained** outbounds (same inbounds, but routed through the Russian
///   relay node first) when `node.relay_info` is set – essential for whitelist
///   bypass scenarios in Russia.
/// - Smart proxy groups: `proxy` (selector) → `auto-all` / `auto-relay` /
///   `auto-direct` (URL-test groups).  The client's auto-test picks the fastest
///   working path, so both direct and relay live in the same subscription and
///   the right one wins automatically.
/// - Modern rule-set-based routing (SRS binary format, no deprecated geosite/
///   geoip fields).
/// - TLS fragment on direct TCP+Reality connections to help bypass DPI.
pub fn generate_singbox_config(
    _sub: &Subscription,
    nodes: &[NodeInfo],
    user_keys: &UserKeys,
    relay_nodes: &[NodeInfo],
) -> Result<String> {
    // Tags split by whether they go through a relay or not.
    let mut direct_tags: Vec<String> = vec![];
    let mut relay_tags: Vec<String> = vec![];
    // Actual outbound JSON objects (relay infrastructure + proxy outbounds).
    let mut proxy_outbounds: Vec<Value> = vec![];
    // Cache: relay_node_address -> relay outbound tag (avoid duplicates).
    let mut relay_cache: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // Track used tags to ensure uniqueness.
    let mut used_tags: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Helper: make a tag unique by appending " 2", " 3", etc. if needed.
    let unique_tag = |base: String, used: &mut std::collections::HashSet<String>| -> String {
        if used.insert(base.clone()) {
            return base;
        }
        for i in 2..100 {
            let candidate = format!("{} {}", base, i);
            if used.insert(candidate.clone()) {
                return candidate;
            }
        }
        base // should never reach here
    };

    // ─── Build outbounds ──────────────────────────────────────────────────────
    for node in nodes {
        // Skip pure infrastructure relay nodes – users should not connect to
        // them directly as a destination.
        if node.is_relay {
            continue;
        }

        let enabled: Vec<_> = node.inbounds.iter().filter(|ib| ib.enable).collect();
        let node_label = format_node_label(node);
        let endpoint = node.frontend_url.as_deref().unwrap_or(&node.address);

        for inbound in &enabled {
            let si = parse_stream_settings(&inbound.stream_settings, node);
            let proto_label = format_proto_label(&inbound.protocol, &si);

            // ── Direct outbound ───────────────────────────────────────────────
            let direct_tag = unique_tag(format!("{} {}", node_label, proto_label), &mut used_tags);
            if let Some(ob) =
                build_singbox_outbound(&direct_tag, inbound, endpoint, &si, user_keys, None)
            {
                proxy_outbounds.push(ob);
                direct_tags.push(direct_tag);
            }

            // ── Relay-chained outbounds (auto-matched) ─────────────────────
            for relay in relay_nodes {
                if let Some(relay_ob_tag) =
                    ensure_relay_outbound(relay, user_keys, &mut proxy_outbounds, &mut relay_cache)
                {
                    let relay_label = format_node_label(relay);
                    let via_tag = unique_tag(
                        format!("{} {} via {}", node_label, proto_label, relay_label),
                        &mut used_tags,
                    );
                    if let Some(ob) = build_singbox_outbound(
                        &via_tag,
                        inbound,
                        endpoint,
                        &si,
                        user_keys,
                        Some(&relay_ob_tag),
                    ) {
                        proxy_outbounds.push(ob);
                        relay_tags.push(via_tag);
                    }
                }
            }
        }

        // ── Legacy fallback: node has no inbounds stored ──────────────────────
        if enabled.is_empty()
            && let Some(port) = node.reality_port
        {
            let tag = unique_tag(format!("{} Stealth", node_label), &mut used_tags);
            let ob = json!({
                "type": "vless",
                "tag": &tag,
                "server": endpoint,
                "server_port": port,
                "uuid": user_keys.user_uuid,
                "flow": "xtls-rprx-vision",
                "tls": {
                    "enabled": true,
                    "server_name": node.reality_sni.as_deref().unwrap_or("www.google.com"),
                    "reality": {
                        "enabled": true,
                        "public_key": node.reality_public_key.as_deref().unwrap_or(""),
                        "short_id": node.reality_short_id.as_deref().unwrap_or("")
                    },
                    "utls": { "enabled": true, "fingerprint": "chrome" }
                },
                "_remark": format!("{} Reality", node_label)
            });
            proxy_outbounds.push(ob);
            direct_tags.push(tag);
        }
    }

    let has_relay = !relay_tags.is_empty();
    let all_tags: Vec<String> = direct_tags
        .iter()
        .chain(relay_tags.iter())
        .cloned()
        .collect();

    if all_tags.is_empty() {
        // Все ноды были пропущены (нет активных inbounds или все is_relay=true).
        // Возвращаем ошибку чтобы вызывающий код мог залогировать и вернуть 500/503.
        return Err(anyhow::anyhow!(
            "No proxy outbounds generated: nodes either have no enabled inbounds or all nodes are relay-only infrastructure"
        ));
    }

    // ─── Proxy groups ─────────────────────────────────────────────────────────

    // Selector list: smart groups first, then individual proxies.
    let mut selector_outbounds = vec!["auto-all".to_string()];
    if has_relay {
        selector_outbounds.push("auto-relay".to_string());
    }
    selector_outbounds.push("auto-direct".to_string());
    selector_outbounds.extend(all_tags.clone());

    let mut final_outbounds: Vec<Value> = vec![
        // Main selector – user can override to any specific proxy.
        json!({
            "type": "selector",
            "tag": "proxy",
            "outbounds": selector_outbounds,
            "default": "auto-all"
        }),
        // Auto-all: fastest across everything (direct + relay).
        json!({
            "type": "urltest",
            "tag": "auto-all",
            "outbounds": all_tags,
            "url": "https://www.gstatic.com/generate_204",
            "interval": "3m",
            "tolerance": 50
        }),
    ];

    // Auto-relay group: only relay-chained paths.
    // Ideal when RKN whitelist is active and direct foreign routes are cut.
    if has_relay {
        final_outbounds.push(json!({
            "type": "urltest",
            "tag": "auto-relay",
            "outbounds": relay_tags,
            "url": "https://www.gstatic.com/generate_204",
            "interval": "3m",
            "tolerance": 50
        }));
    }

    // Auto-direct group: direct connections only (lowest latency when unblocked).
    if !direct_tags.is_empty() {
        final_outbounds.push(json!({
            "type": "urltest",
            "tag": "auto-direct",
            "outbounds": direct_tags,
            "url": "https://www.gstatic.com/generate_204",
            "interval": "3m",
            "tolerance": 50
        }));
    }

    // System outbounds.
    final_outbounds.push(json!({ "type": "direct", "tag": "direct" }));
    final_outbounds.push(json!({ "type": "block",  "tag": "block"  }));
    final_outbounds.push(json!({ "type": "dns",    "tag": "dns-out" }));

    // Actual proxy outbounds last.
    final_outbounds.extend(proxy_outbounds);

    // ─── Content-filtering policies ───────────────────────────────────────────
    let block_ads = nodes.iter().any(|n| n.config_block_ads);
    let block_porn = nodes.iter().any(|n| n.config_block_porn);

    // ─── DNS ──────────────────────────────────────────────────────────────────
    let mut dns_rules: Vec<Value> = vec![
        // DNS-over-HTTPS queries themselves must not loop.
        json!({ "outbound": ["any"], "server": "local-plain" }),
        // Clash API mode overrides.
        json!({ "clash_mode": "direct", "server": "local" }),
        json!({ "clash_mode": "global", "server": "remote" }),
        // Domestic Chinese domains resolve locally.
        json!({ "rule_set": ["geosite-cn"], "server": "local" }),
        // Russian domestic domains resolve locally (don't proxy their DNS).
        json!({ "rule_set": ["geosite-ru"], "server": "local" }),
    ];
    if block_ads {
        dns_rules.push(json!({ "rule_set": ["geosite-ads"], "server": "block" }));
    }
    if block_porn {
        dns_rules.push(json!({ "rule_set": ["geosite-porn"], "server": "block" }));
    }

    let dns_config = json!({
        "servers": [
            {
                "tag": "remote",
                "address": "https://8.8.8.8/dns-query",
                "address_resolver": "local-plain",
                "detour": "proxy"
            },
            {
                "tag": "local",
                "address": "https://223.5.5.5/dns-query",
                "address_resolver": "local-plain",
                "detour": "direct"
            },
            // Plain UDP – only used to bootstrap DoH address resolution.
            {
                "tag": "local-plain",
                "address": "223.5.5.5",
                "detour": "direct"
            },
            { "tag": "block", "address": "rcode://success" }
        ],
        "rules": dns_rules,
        "final": "remote",
        "strategy": "prefer_ipv4",
        "independent_cache": true
    });

    // ─── Route rules ──────────────────────────────────────────────────────────
    let mut route_rules: Vec<Value> = vec![
        json!({ "protocol": "dns", "outbound": "dns-out" }),
        json!({ "ip_is_private": true, "outbound": "direct" }),
    ];

    if block_ads {
        route_rules.push(json!({ "rule_set": ["geosite-ads"], "outbound": "block" }));
    }
    if block_porn {
        route_rules.push(json!({ "rule_set": ["geosite-porn"], "outbound": "block" }));
    }

    // Chinese and Russian domestic traffic goes direct.
    // Note: relay traffic to the *relay node* itself is handled transparently by
    // sing-box's detour mechanism – it doesn't match these rules because the
    // proxy outbound establishes the connection, not the routing layer.
    route_rules.push(
        json!({ "rule_set": ["geoip-private", "geosite-cn", "geoip-cn"], "outbound": "direct" }),
    );
    route_rules.push(json!({ "rule_set": ["geosite-ru", "geoip-ru"], "outbound": "direct" }));

    // Everything else → proxy (auto-all picks the best path).

    // ─── Rule sets (SRS binary format, modern sing-box ≥ 1.8) ────────────────
    let mut rule_sets: Vec<Value> = vec![
        json!({
            "type": "remote", "tag": "geoip-private", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-private.srs",
            "download_detour": "direct", "update_interval": "7d"
        }),
        json!({
            "type": "remote", "tag": "geosite-cn", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
            "download_detour": "proxy", "update_interval": "3d"
        }),
        json!({
            "type": "remote", "tag": "geoip-cn", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs",
            "download_detour": "proxy", "update_interval": "3d"
        }),
        json!({
            "type": "remote", "tag": "geosite-ru", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-ru.srs",
            "download_detour": "proxy", "update_interval": "3d"
        }),
        json!({
            "type": "remote", "tag": "geoip-ru", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ru.srs",
            "download_detour": "proxy", "update_interval": "3d"
        }),
    ];

    if block_ads {
        rule_sets.push(json!({
            "type": "remote", "tag": "geosite-ads", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs",
            "download_detour": "proxy", "update_interval": "1d"
        }));
    }
    if block_porn {
        rule_sets.push(json!({
            "type": "remote", "tag": "geosite-porn", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-porn.srs",
            "download_detour": "proxy", "update_interval": "1d"
        }));
    }

    let route_config = json!({
        "auto_detect_interface": true,
        "final": "proxy",
        "rules": route_rules,
        "rule_set": rule_sets
    });

    // ─── Client inbounds (local mixed proxy port) ─────────────────────────────
    let inbounds_config = vec![
        json!({
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 2080,
            "sniff": true,
            "sniff_override_destination": true,
            "set_system_proxy": false
        }),
        // TUN inbound for transparent proxy mode (optional, enabled by clients
        // that support it such as Hiddify / sing-box desktop).
        json!({
            "type": "tun",
            "tag": "tun-in",
            "inet4_address": "172.19.0.1/30",
            "auto_route": true,
            "strict_route": true,
            "sniff": true,
            "sniff_override_destination": true,
            "stack": "mixed"
        }),
    ];

    // ─── Final config assembly ────────────────────────────────────────────────
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
                "path": "cache.db",
                "store_fakeip": false
            },
            "clash_api": {
                "external_controller": "127.0.0.1:9090",
                "external_ui": "ui",
                "external_ui_download_url": "https://github.com/MetaCubeX/Yacd-meta/archive/gh-pages.zip",
                "external_ui_download_detour": "proxy",
                "default_mode": "rule",
                "secret": ""
            }
        }
    });

    // Strip internal _remark fields before serializing — Hiddify 4.0+ rejects unknown fields.
    let mut config = config;
    if let Some(outbounds) = config.get_mut("outbounds").and_then(|v| v.as_array_mut()) {
        for ob in outbounds {
            if let Some(obj) = ob.as_object_mut() {
                obj.remove("_remark");
            }
        }
    }

    Ok(serde_json::to_string_pretty(&config)?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod clash_group_tests {
    use super::*;
    use std::collections::HashSet;

    fn inbound(
        id: i64,
        protocol: &str,
        port: i64,
        stream: &str,
    ) -> caramba_db::models::network::Inbound {
        caramba_db::models::network::Inbound {
            id,
            node_id: 1,
            tag: format!("in{id}"),
            protocol: protocol.into(),
            listen_port: port,
            listen_ip: "::".into(),
            settings: "{}".into(),
            stream_settings: stream.into(),
            remark: None,
            enable: true,
            renew_interval_mins: 0,
            port_range_start: 0,
            port_range_end: 0,
            last_rotated_at: None,
            created_at: None,
        }
    }

    fn node(name: &str, cc: &str, inbounds: Vec<caramba_db::models::network::Inbound>) -> NodeInfo {
        NodeInfo {
            name: name.into(),
            address: "198.51.100.7".into(),
            reality_port: None,
            reality_sni: None,
            reality_public_key: None,
            reality_short_id: None,
            hy2_port: None,
            hy2_sni: None,
            frontend_url: None,
            inbounds,
            relay_info: None,
            country_code: Some(cc.into()),
            is_relay: false,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        }
    }

    fn keys() -> UserKeys {
        UserKeys {
            user_uuid: "ebea4631-da5d-4a15-808c-ba126a2b4e16".into(),
            hy2_password: "-46:ebea4631da5d4a15808cba126a2b4e16".into(),
            _awg_private_key: None,
        }
    }

    /// Генератор не читает подписку, поэтому берётся минимально валидная.
    fn sub() -> Subscription {
        serde_json::from_value(json!({
            "id": 1,
            "user_id": 46,
            "plan_id": 1,
            "status": "active",
            "used_traffic": 0,
            "subscription_uuid": "feb7e480-314d-4834-8304-220db70684c2",
            "created_at": "2026-09-02T00:00:00Z",
            "expires_at": "2026-12-02T00:00:00Z",
        }))
        .expect("подписка-заглушка")
    }

    /// Фикстура повторяет узел 1 живого тенанта: восемь включённых инбаундов,
    /// один из которых `naive` (id 304, порт 15400), и весь узел стоит за
    /// релеем — то есть все имена уходят в корзину Auto-Relay.
    fn node_one_behind_a_relay() -> NodeInfo {
        let tls =
            r#"{"network":"tcp","security":"tls","tls_settings":{"server_name":"www.dekulta.de"}}"#;
        let mut exit = node(
            "de1",
            "de",
            vec![
                inbound(
                    303,
                    "hysteria2",
                    11466,
                    r#"{"network":"udp","security":"tls","tlsSettings":{"serverName":"dev.portal.example"}}"#,
                ),
                inbound(304, "naive", 15400, tls),
                inbound(
                    305,
                    "tuic",
                    16400,
                    r#"{"network":"udp","security":"tls","tlsSettings":{"serverName":"dev.portal.example"},"tuicSettings":{"congestion":"bbr"}}"#,
                ),
                inbound(
                    306,
                    "vless",
                    10400,
                    r#"{"network":"grpc","security":"tls","tls_settings":{"server_name":"www.dekulta.de"}}"#,
                ),
                inbound(
                    307,
                    "vless",
                    13400,
                    r#"{"network":"httpupgrade","security":"tls","tls_settings":{"server_name":"www.dekulta.de"},"http_upgrade_settings":{"path":"/hu","host":"www.dekulta.de"}}"#,
                ),
                inbound(
                    308,
                    "vless",
                    443,
                    r#"{"network":"tcp","security":"reality","reality_settings":{"server_names":["www.dekulta.de"],"public_key":"3Tsh7haY915qWht_DsC4Vxunj15EBbTUo0VIIjycSDQ","short_ids":["0b4bf3f48a32ccb8"]}}"#,
                ),
                inbound(309, "vless", 14400, tls),
                inbound(
                    310,
                    "vless",
                    12400,
                    r#"{"network":"ws","security":"tls","tls_settings":{"server_name":"www.dekulta.de"},"ws_settings":{"path":"/ws"}}"#,
                ),
            ],
        );
        let mut relay = node(
            "ru-relay",
            "ru",
            vec![inbound(1, "hysteria2", 8443, r#"{"network":"udp"}"#)],
        );
        relay.address = "141.98.191.214".into();
        relay.is_relay = true;
        exit.relay_info = Some(Box::new(relay));
        exit
    }

    fn parse(yaml: &str) -> (Vec<String>, Vec<(String, Vec<String>)>) {
        let doc: Value = serde_yaml::from_str(yaml).expect("тело Clash — валидный YAML");
        let names = doc["proxies"]
            .as_array()
            .expect("есть proxies:")
            .iter()
            .map(|p| p["name"].as_str().expect("у прокси есть имя").to_string())
            .collect();
        let groups = doc["proxy-groups"]
            .as_array()
            .expect("есть proxy-groups:")
            .iter()
            .map(|g| {
                (
                    g["name"].as_str().expect("у группы есть имя").to_string(),
                    g["proxies"]
                        .as_array()
                        .expect("у группы есть список")
                        .iter()
                        .map(|m| m.as_str().expect("участник — строка").to_string())
                        .collect(),
                )
            })
            .collect();
        (names, groups)
    }

    /// Главная проба: каждый участник каждой группы обязан разрешаться —
    /// либо в выпущенный прокси, либо в другую группу этого же тела.
    ///
    /// До правки `naive` давал ровно этот дефект: имя «🇩🇪 Naive ↪» стояло в
    /// Auto-Relay живой подписки, а прокси под ним не существовало, потому что
    /// имена копились ДО match по протоколу.
    #[test]
    fn every_group_member_resolves_to_an_emitted_proxy() {
        let exit = node_one_behind_a_relay();
        let yaml = generate_clash_config(&sub(), std::slice::from_ref(&exit), &keys(), &[])
            .expect("генератор выпускает конфиг");
        let (names, groups) = parse(&yaml);

        let proxy_set: HashSet<&str> = names.iter().map(|s| s.as_str()).collect();
        let group_set: HashSet<&str> = groups.iter().map(|(n, _)| n.as_str()).collect();
        assert!(!groups.is_empty(), "группы вообще выпущены");

        for (group, members) in &groups {
            assert!(
                !members.is_empty(),
                "группа {group} пуста, mihomo её отвергнет"
            );
            for m in members {
                assert!(
                    proxy_set.contains(m.as_str()) || group_set.contains(m.as_str()),
                    "группа {group} ссылается на «{m}», которого нет ни в proxies:, ни среди групп.\n{yaml}"
                );
            }
        }
    }

    /// `naive` опускается целиком: ни прокси, ни имени в группах. Выбор
    /// зафиксирован тестом, потому что у mihomo нет исходящего `naive`
    /// (adapter/parser.go), а каталог CSM опускает его тем же правилом.
    #[test]
    fn naive_appears_neither_as_a_proxy_nor_as_a_group_member() {
        let exit = node_one_behind_a_relay();
        let yaml = generate_clash_config(&sub(), std::slice::from_ref(&exit), &keys(), &[])
            .expect("генератор выпускает конфиг");
        let (names, groups) = parse(&yaml);

        // Восемь инбаундов минус naive.
        assert_eq!(
            names.len(),
            7,
            "выпущены все инбаунды кроме naive: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("Naive")),
            "naive не должен выпускаться прокси: {names:?}"
        );
        for (group, members) in &groups {
            assert!(
                !members.iter().any(|m| m.contains("Naive")),
                "имя naive осталось в группе {group}: {members:?}"
            );
        }
    }

    /// Говорит ли этот тип прокси по TLS вообще. У vless/vmess TLS
    /// опционален (`tls: true`), у trojan/hysteria2/tuic он обязателен по
    /// устройству протокола, у ss/wireguard TLS нет.
    fn speaks_tls(proxy: &Value) -> bool {
        match proxy["type"].as_str().unwrap_or_default() {
            "vless" | "vmess" => proxy["tls"] == json!(true),
            "trojan" | "hysteria2" | "tuic" => true,
            _ => false,
        }
    }

    /// Главный инвариант этого раунда: каждый TLS-прокси в теле обязан нести
    /// гарантию доверия к самоподписанному сертификату узла.
    ///
    /// Именно этого не хватало живой подписке: восемь VLESS+TLS из тринадцати
    /// прокси уезжали с `tls: true` и без `skip-cert-verify`, и mihomo рвал
    /// рукопожатие на проверке цепочки. Reality исключён осознанно — он
    /// аутентифицирует сервер X25519-ключом, а не сертификатом.
    #[test]
    fn every_tls_proxy_carries_a_trust_guarantee() {
        let exit = node_one_behind_a_relay();
        let yaml = generate_clash_config(&sub(), std::slice::from_ref(&exit), &keys(), &[])
            .expect("генератор выпускает конфиг");
        let doc: Value = serde_yaml::from_str(&yaml).expect("тело Clash — валидный YAML");
        let proxies = doc["proxies"].as_array().expect("есть proxies:");

        let mut checked = 0usize;
        for p in proxies {
            if !speaks_tls(p) {
                continue;
            }
            let name = p["name"].as_str().unwrap_or("<без имени>");
            if p.get("reality-opts").is_some() {
                assert!(
                    p.get("skip-cert-verify").is_none() && p.get("fingerprint").is_none(),
                    "«{name}»: Reality не проверяет цепочку, лишняя опция доверия только путает"
                );
                continue;
            }
            checked += 1;

            let skip = p["skip-cert-verify"] == json!(true);
            let pinned = p
                .get("fingerprint")
                .and_then(|v| v.as_str())
                .map(|fp| {
                    let hex: String = fp.chars().filter(|c| *c != ':').collect();
                    hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())
                })
                .unwrap_or(false);
            assert!(
                skip || pinned,
                "«{name}» ({}) идёт по TLS без пина и без skip-cert-verify — \
                 mihomo отвергнет самоподписанный сертификат узла:\n{p:#}",
                p["type"].as_str().unwrap_or("?")
            );
        }
        assert!(
            checked >= 5,
            "фикстура должна содержать не-Reality TLS-прокси, иначе тест ничего не проверяет"
        );
    }

    /// Ветка пина — не мёртвый код: когда `node_cert_pin` начнёт возвращать
    /// отпечаток, тело обязано нести `fingerprint`, а НЕ `skip-cert-verify`.
    /// Формат — тот, что принимает `ca.NewFingerprintVerifier`: hex SHA-256,
    /// двоеточия допустимы.
    #[test]
    fn a_known_fingerprint_pins_instead_of_skipping_verification() {
        // Реальный отпечаток узла 1 (85.215.196.151, CN=essentialhome.live).
        const FP: &str = "B34FCC4D69C01F21E0B3BC61B9143D2C34C0755F3AD7F7F9E4C8CEEF956A2F65";

        let mut pinned = json!({ "name": "n", "type": "vless", "tls": true });
        apply_clash_tls_trust(&mut pinned, Some(FP));
        assert_eq!(pinned["fingerprint"], json!(FP));
        assert!(
            pinned.get("skip-cert-verify").is_none(),
            "пин и отключённая проверка вместе бессмысленны: {pinned:#}"
        );

        let mut unpinned = json!({ "name": "n", "type": "vless", "tls": true });
        apply_clash_tls_trust(&mut unpinned, None);
        assert_eq!(unpinned["skip-cert-verify"], json!(true));
        assert!(unpinned.get("fingerprint").is_none());
    }

    /// Обратная сторона того же инварианта: узел, у которого ВСЕ инбаунды
    /// генератору не по зубам, не должен рождать ни пустую группу, ни
    /// висячего участника. Раньше `has_relay` считался по параллельному
    /// списку имён и такая нода выпускала Auto-Relay из одних призраков.
    #[test]
    fn a_node_with_only_undialable_inbounds_produces_no_ghost_group() {
        let mut exit = node(
            "de1",
            "de",
            vec![inbound(
                304,
                "naive",
                15400,
                r#"{"network":"tcp","security":"tls"}"#,
            )],
        );
        let mut relay = node(
            "ru-relay",
            "ru",
            vec![inbound(1, "hysteria2", 8443, r#"{"network":"udp"}"#)],
        );
        relay.is_relay = true;
        exit.relay_info = Some(Box::new(relay));

        let direct = node(
            "nl1",
            "nl",
            vec![inbound(
                400,
                "vless",
                443,
                r#"{"network":"tcp","security":"reality","reality_settings":{"server_names":["www.dekulta.de"],"public_key":"pk","short_ids":["00"]}}"#,
            )],
        );

        let yaml = generate_clash_config(&sub(), &[exit, direct], &keys(), &[])
            .expect("генератор выпускает конфиг");
        let (names, groups) = parse(&yaml);

        assert_eq!(names.len(), 1, "выпущен только дозвонимый выход: {names:?}");
        assert!(
            !groups.iter().any(|(n, _)| n == "Auto-Relay"),
            "Auto-Relay выпущена без единого прокси-цепочки: {groups:?}"
        );
        let proxy_set: HashSet<&str> = names.iter().map(|s| s.as_str()).collect();
        let group_set: HashSet<&str> = groups.iter().map(|(n, _)| n.as_str()).collect();
        for (group, members) in &groups {
            for m in members {
                assert!(
                    proxy_set.contains(m.as_str()) || group_set.contains(m.as_str()),
                    "группа {group} ссылается на «{m}», которого нет"
                );
            }
        }
    }
}
