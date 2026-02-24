use crate::panel_client::{InternalNode, UserKeys};
use serde_json::{json, Value};

pub struct ConfigGenerator;

fn is_placeholder_sni(sni: &str) -> bool {
    let sni = sni.trim().to_ascii_lowercase();
    sni.is_empty() || sni == "www.google.com" || sni == "google.com" || sni == "drive.google.com"
}

fn best_node_sni(node: &crate::panel_client::Node) -> String {
    node.reality_sni
        .as_ref()
        .or(node.domain.as_ref())
        .filter(|s| !is_placeholder_sni(s))
        .cloned()
        .unwrap_or_else(|| node.ip.clone())
}

impl ConfigGenerator {
    pub fn generate(
        internal_nodes: Vec<InternalNode>,
        user_keys: &UserKeys,
        region: &str, // "RU", "global", etc.
    ) -> Value {
        let mut outbounds = vec![];

        // 1. Identify Relay Nodes
        let relay_nodes: Vec<&InternalNode> =
            internal_nodes.iter().filter(|n| n.node.is_relay).collect();

        // 2. Select Routing Strategy
        // If Region is RU (or others we want to optimize), route through Relay
        let use_relay = region == "RU";

        // 3. Generate Outbounds (VLESS/Hysteria2)
        for i_node in &internal_nodes {
            let node = &i_node.node;

            // Skip processing incompatible nodes or disabled ones if any

            for inbound in &i_node.inbounds {
                if !inbound.enable {
                    continue;
                }

                // Parse Settings
                let stream_settings: Value =
                    serde_json::from_str(&inbound.stream_settings).unwrap_or(json!({}));

                let protocol = inbound.protocol.to_lowercase();

                // We only generate config for VLESS/Hysteria (Exit) and Shadowsocks (Relay Transport)
                if protocol == "vless" {
                    // Generate VLESS Outbound
                    let uuid = &user_keys.user_uuid;
                    let server = &node.ip;
                    let port = inbound.listen_port;

                    let mut tls = json!({ "enabled": false });
                    let mut transport: Option<Value> = None;
                    let mut flow: Option<String> = None;

                    let security = stream_settings
                        .get("security")
                        .and_then(|v| v.as_str())
                        .unwrap_or("none");
                    let network = stream_settings
                        .get("network")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tcp");

                    if security == "reality" || security == "tls" {
                        tls["enabled"] = json!(true);
                        let fallback_sni = best_node_sni(node);

                        if security == "reality" {
                            if let Some(reality) = stream_settings.get("reality_settings") {
                                let server_names_array =
                                    reality.get("server_names").or_else(|| reality.get("serverNames"));
                                let inbound_sni = server_names_array
                                    .and_then(|v| v.get(0))
                                    .or_else(|| reality.get("serverName"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .filter(|s| !is_placeholder_sni(s));
                                let node_sni = node
                                    .reality_sni
                                    .as_ref()
                                    .filter(|s| !is_placeholder_sni(s))
                                    .cloned();

                                tls["server_name"] =
                                    json!(node_sni.or(inbound_sni).unwrap_or(fallback_sni.clone()));
                                tls["reality"] = json!({
                                    "enabled": true,
                                    "public_key": reality.get("public_key").cloned().unwrap_or(json!(node.reality_pub.clone().unwrap_or_default())),
                                    "short_id": reality.get("short_ids").and_then(|v| v.get(0)).cloned().unwrap_or(json!(node.short_id.clone().unwrap_or_default()))
                                });
                                tls["utls"] = json!({ "enabled": true, "fingerprint": "chrome" });

                                if network == "tcp" {
                                    flow = Some("xtls-rprx-vision".to_string());
                                }
                            }
                        } else if security == "tls" {
                            if let Some(t) = stream_settings
                                .get("tls_settings")
                                .or_else(|| stream_settings.get("tlsSettings"))
                            {
                                let inbound_sni = t
                                    .get("server_name")
                                    .or_else(|| t.get("serverName"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .filter(|s| !is_placeholder_sni(s));
                                let node_sni = node
                                    .reality_sni
                                    .as_ref()
                                    .filter(|s| !is_placeholder_sni(s))
                                    .cloned();
                                tls["server_name"] =
                                    json!(node_sni.or(inbound_sni).unwrap_or(fallback_sni.clone()));
                            }
                        }
                    }

                    // Transport logic
                    if network == "ws" {
                        if let Some(ws) = stream_settings
                            .get("ws_settings")
                            .or_else(|| stream_settings.get("wsSettings"))
                        {
                            transport = Some(json!({
                                "type": "ws",
                                "path": ws.get("path").cloned().unwrap_or(json!("/")),
                                "headers": ws.get("headers").cloned().unwrap_or(json!({}))
                            }));
                        }
                    } else if network == "grpc" {
                        if let Some(grpc) = stream_settings
                            .get("grpc_settings")
                            .or_else(|| stream_settings.get("grpcSettings"))
                        {
                            transport = Some(json!({
                                "type": "grpc",
                                "service_name": grpc.get("service_name").or_else(|| grpc.get("serviceName")).cloned().unwrap_or(json!(""))
                            }));
                        }
                    }

                    let mut outbound = json!({
                        "type": "vless",
                        "tag": format!("{}-vless", node.name),
                        "server": server,
                        "server_port": port,
                        "uuid": uuid,
                        "packet_encoding": "xudp"
                    });

                    if tls["enabled"].as_bool() == Some(true) {
                        outbound["tls"] = tls;
                    }
                    if let Some(f) = flow {
                        outbound["flow"] = json!(f);
                    }
                    if let Some(t) = transport {
                        outbound["transport"] = t;
                    }

                    outbounds.push(outbound);
                } else if protocol == "hysteria2" {
                    let password = format!("{}:{}", user_keys.user_uuid, user_keys.hy2_password);
                    let tag = format!("{}-hy2", node.name);

                    let mut tls = json!({ "enabled": true });
                    tls["server_name"] = json!(best_node_sni(node));

                    outbounds.push(json!({
                        "type": "hysteria2",
                        "tag": tag,
                        "server": node.ip,
                        "server_port": inbound.listen_port,
                        "password": password,
                        "tls": tls
                    }));
                }
            }
        }

        // 4. Relay Logic (Sing-box Detour)
        // If use_relay && we have relays
        if use_relay && !relay_nodes.is_empty() {
            if let Some(relay) = relay_nodes.first() {
                let relay_tag = format!("{}-relay-ss", relay.node.name);

                if let Some(ss_inbound) =
                    relay.inbounds.iter().find(|i| i.protocol == "shadowsocks")
                {
                    let settings: Value =
                        serde_json::from_str(&ss_inbound.settings).unwrap_or(json!({}));
                    let method = settings
                        .get("method")
                        .and_then(|s| s.as_str())
                        .unwrap_or("chacha20-ietf-poly1305");
                    let password = settings
                        .get("password")
                        .and_then(|s| s.as_str())
                        .unwrap_or("");

                    outbounds.push(json!({
                        "type": "shadowsocks",
                        "tag": relay_tag,
                        "server": relay.node.ip,
                        "server_port": ss_inbound.listen_port,
                        "method": method,
                        "password": password
                    }));

                    for outbound in &mut outbounds {
                        if let Some(tag) = outbound.get("tag").and_then(|t| t.as_str()) {
                            if tag != relay_tag && tag != "direct" && tag != "block" {
                                outbound["detour"] = json!(relay_tag);
                            }
                        }
                    }
                }
            }
        }

        // Final Config
        json!({
            "log": { "level": "info", "timestamp": true },
            "inbounds": [],
            "outbounds": outbounds,
            "route": {
                "auto_detect_interface": true,
                "final": "direct",
                "rules": []
            }
        })
    }
}
