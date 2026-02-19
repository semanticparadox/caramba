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

                    let flow = "xtls-rprx-vision"; // Simplified assumption for Reality/TCP

                    let mut tls = json!({ "enabled": true });

                    // Extract Reality/TLS settings
                    let fallback_sni = best_node_sni(node);
                    if let Some(security) = stream_settings.get("security").and_then(|s| s.as_str())
                    {
                        if security == "reality" {
                            if let Some(reality) = stream_settings.get("reality_settings") {
                                let inbound_sni = reality
                                    .get("server_names")
                                    .and_then(|v| v.get(0))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .filter(|s| !is_placeholder_sni(s));
                                tls["server_name"] =
                                    json!(inbound_sni.unwrap_or(fallback_sni.clone()));
                                tls["reality"] = json!({
                                    "enabled": true,
                                    "public_key": reality.get("public_key").cloned().unwrap_or(json!(node.reality_pub.clone().unwrap_or_default())),
                                    "short_id": reality.get("short_ids").and_then(|v| v.get(0)).cloned().unwrap_or(json!(node.short_id.clone().unwrap_or_default()))
                                });
                                tls["utls"] = json!({ "enabled": true, "fingerprint": "chrome" });
                            }
                        } else if security == "tls" {
                            if let Some(t) = stream_settings.get("tls_settings") {
                                let inbound_sni = t
                                    .get("server_name")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .filter(|s| !is_placeholder_sni(s));
                                tls["server_name"] =
                                    json!(inbound_sni.unwrap_or(fallback_sni.clone()));
                            }
                        }
                    }

                    let tag = format!("{}-vless", node.name);

                    // If this is a Relay Node, we mark it.
                    // But usually Relays are Shadowsocks/Hysteria.
                    // This block generates standard VLESS for direct connection.

                    outbounds.push(json!({
                        "type": "vless",
                        "tag": tag,
                        "server": server,
                        "server_port": port,
                        "uuid": uuid,
                        "flow": flow,
                        "tls": tls,
                        "packet_encoding": "xudp"
                    }));
                } else if protocol == "hysteria2" {
                    let password = format!("{}:{}", user_keys.user_uuid, user_keys.hy2_password);
                    let tag = format!("{}-hy2", node.name);

                    let mut tls = json!({ "enabled": true });
                    // Hysteria2 usually uses Node's SNI matching Reality/Cert
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
            // Find a suitable relay (e.g. first one)
            // Ideally load balance, but let's pick first active relay
            if let Some(relay) = relay_nodes.first() {
                let relay_tag = format!("{}-relay-ss", relay.node.name);

                // Add Relay Outbound (Shadowsocks)
                // We need to find the Shadowsocks inbound on the Relay Node
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
                    // Wait, Relay authentication is usually via per-user token, OR a shared password if it's a dedicated relay.
                    // In `caramba-panel`, we injected `relay_<id>` user.
                    // Here we are the Client. We need to authenticate to the Relay.
                    // The Relay verifies US.
                    // Actually, if we use a "Relay Node" from the standard node list, it's just another node.
                    // The "Smart Routing" usually means: Use Relay X as a DETOUR for Node Y.

                    // Let's implement Chain: Client -> Relay (SS) -> Endpoint (VLESS).
                    // We need to add `detour` to the Endpoint outbound.

                    // 1. Add Relay Outbound
                    outbounds.push(json!({
                        "type": "shadowsocks",
                        "tag": relay_tag,
                        "server": relay.node.ip,
                        "server_port": ss_inbound.listen_port,
                        "method": method,
                        "password": password // This needs to be the password for the Relay User.
                        // In Panel generator, we injected `user: relay_X`.
                        // But here we are the end user.
                        // If the Relay is public/shared, we use that password.
                        // If it's private, we need a user on the relay.
                        // Simplified: Assume Relay has a shared password or we use the Node's join_token?
                        // For this implementation, let's assume direct access or standard user auth.
                    }));

                    // 2. Modify other outbounds to use this detour
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
            "inbounds": [{
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": 2080
            }],
            "outbounds": outbounds,
            "route": {
                "auto_detect_interface": true,
                "final": "direct",
                // basic rules
            }
        })
    }
}
