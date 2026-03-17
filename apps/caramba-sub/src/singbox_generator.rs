use crate::panel_client::{Inbound, InternalNode, Node, UserKeys};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashSet;

pub struct ConfigGenerator;

const FULL_TAG_01: &str = "01 - WS TLS (Relay)";
const FULL_TAG_02: &str = "02 - HTTPUpgrade TLS (Relay)";
const FULL_TAG_03: &str = "03 - gRPC TLS (Relay)";
const FULL_TAG_04: &str = "04 - TCP TLS (Relay)";
const FULL_TAG_05: &str = "05 - Hysteria2 (Relay)";
const FULL_TAG_06: &str = "06 - Reality (Relay)";
const FULL_TAG_07: &str = "07 - Reality (Direct)";
const FULL_TAG_08: &str = "08 - gRPC TLS (Direct)";
const FULL_TAG_09: &str = "09 - Hysteria2 (Direct)";

const DIRECT_TAG_01: &str = "01 - Reality (Direct)";
const DIRECT_TAG_02: &str = "02 - gRPC TLS (Direct)";
const DIRECT_TAG_03: &str = "03 - Hysteria2 (Direct)";

const EXPECTED_TAGS_FULL: [&str; 9] = [
    FULL_TAG_01,
    FULL_TAG_02,
    FULL_TAG_03,
    FULL_TAG_04,
    FULL_TAG_05,
    FULL_TAG_06,
    FULL_TAG_07,
    FULL_TAG_08,
    FULL_TAG_09,
];

const EXPECTED_TAGS_DIRECT: [&str; 3] = [DIRECT_TAG_01, DIRECT_TAG_02, DIRECT_TAG_03];

#[derive(Clone, Copy)]
enum NodeScope {
    Relay,
    Exit,
}

#[derive(Clone, Copy)]
enum VariantKind {
    VlessTls(&'static str), // normalized network
    VlessReality,
    Hysteria2,
}

struct VariantSpec {
    tag: &'static str,
    scope: NodeScope,
    kind: VariantKind,
    detour: Option<&'static str>,
}

const FULL_VARIANTS: [VariantSpec; 9] = [
    VariantSpec {
        tag: FULL_TAG_01,
        scope: NodeScope::Relay,
        kind: VariantKind::VlessTls("ws"),
        detour: Some(FULL_TAG_07),
    },
    VariantSpec {
        tag: FULL_TAG_02,
        scope: NodeScope::Relay,
        kind: VariantKind::VlessTls("httpupgrade"),
        detour: Some(FULL_TAG_07),
    },
    VariantSpec {
        tag: FULL_TAG_03,
        scope: NodeScope::Relay,
        kind: VariantKind::VlessTls("grpc"),
        detour: Some(FULL_TAG_08),
    },
    VariantSpec {
        tag: FULL_TAG_04,
        scope: NodeScope::Relay,
        kind: VariantKind::VlessTls("tcp"),
        detour: Some(FULL_TAG_07),
    },
    VariantSpec {
        tag: FULL_TAG_05,
        scope: NodeScope::Relay,
        kind: VariantKind::Hysteria2,
        detour: Some(FULL_TAG_09),
    },
    VariantSpec {
        tag: FULL_TAG_06,
        scope: NodeScope::Relay,
        kind: VariantKind::VlessReality,
        detour: Some(FULL_TAG_07),
    },
    VariantSpec {
        tag: FULL_TAG_07,
        scope: NodeScope::Exit,
        kind: VariantKind::VlessReality,
        detour: None,
    },
    VariantSpec {
        tag: FULL_TAG_08,
        scope: NodeScope::Exit,
        kind: VariantKind::VlessTls("grpc"),
        detour: None,
    },
    VariantSpec {
        tag: FULL_TAG_09,
        scope: NodeScope::Exit,
        kind: VariantKind::Hysteria2,
        detour: None,
    },
];

const DIRECT_VARIANTS: [VariantSpec; 3] = [
    VariantSpec {
        tag: DIRECT_TAG_01,
        scope: NodeScope::Exit,
        kind: VariantKind::VlessReality,
        detour: None,
    },
    VariantSpec {
        tag: DIRECT_TAG_02,
        scope: NodeScope::Exit,
        kind: VariantKind::VlessTls("grpc"),
        detour: None,
    },
    VariantSpec {
        tag: DIRECT_TAG_03,
        scope: NodeScope::Exit,
        kind: VariantKind::Hysteria2,
        detour: None,
    },
];

fn is_placeholder_sni(sni: &str) -> bool {
    let sni = sni.trim().to_ascii_lowercase();
    sni.is_empty() || sni == "www.google.com" || sni == "google.com" || sni == "drive.google.com"
}

fn best_node_sni(node: &Node) -> String {
    node.reality_sni
        .as_ref()
        .or(node.domain.as_ref())
        .filter(|s| !is_placeholder_sni(s))
        .cloned()
        .unwrap_or_else(|| node.ip.clone())
}

fn parse_stream_settings(inbound: &Inbound) -> Value {
    serde_json::from_str(&inbound.stream_settings).unwrap_or_else(|_| json!({}))
}

fn normalize_network(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], "")
        .to_string()
}

fn stream_security(stream: &Value) -> String {
    stream
        .get("security")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase()
}

fn stream_network(stream: &Value) -> String {
    normalize_network(
        stream
            .get("network")
            .and_then(Value::as_str)
            .unwrap_or("tcp"),
    )
}

fn is_vless(inbound: &Inbound) -> bool {
    inbound.protocol.trim().eq_ignore_ascii_case("vless")
}

fn is_hysteria2(inbound: &Inbound) -> bool {
    matches!(
        inbound.protocol.trim().to_ascii_lowercase().as_str(),
        "hysteria2" | "hy2"
    )
}

fn is_vless_reality(stream: &Value) -> bool {
    let security = stream_security(stream);
    security == "reality"
        || stream.get("reality_settings").is_some()
        || stream.get("realitySettings").is_some()
}

fn pick_first_matching_inbound<'a, F>(
    node: &'a InternalNode,
    predicate: F,
) -> Option<(&'a Inbound, Value)>
where
    F: Fn(&Inbound, &Value) -> bool,
{
    for inbound in &node.inbounds {
        if !inbound.enable {
            continue;
        }
        let stream = parse_stream_settings(inbound);
        if predicate(inbound, &stream) {
            return Some((inbound, stream));
        }
    }
    None
}

fn pick_vless_tls_inbound<'a>(
    node: &'a InternalNode,
    network: &'static str,
) -> Option<(&'a Inbound, Value)> {
    pick_first_matching_inbound(node, |inbound, stream| {
        is_vless(inbound) && stream_security(stream) == "tls" && stream_network(stream) == network
    })
}

fn pick_vless_reality_inbound(node: &InternalNode) -> Option<(&Inbound, Value)> {
    pick_first_matching_inbound(node, |inbound, stream| {
        is_vless(inbound) && is_vless_reality(stream)
    })
}

fn pick_hysteria2_inbound(node: &InternalNode) -> Option<(&Inbound, Value)> {
    pick_first_matching_inbound(node, |inbound, _| is_hysteria2(inbound))
}

fn get_nested_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

fn get_nested_array_first_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key)
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
}

fn reality_and_tls_block(node: &Node, stream: &Value, security: &str) -> Value {
    let fallback_sni = best_node_sni(node);

    if security == "reality" {
        let reality = stream
            .get("reality_settings")
            .or_else(|| stream.get("realitySettings"));

        let inbound_sni = reality
            .and_then(|r| {
                get_nested_array_first_str(r, "server_names")
                    .or_else(|| get_nested_array_first_str(r, "serverNames"))
                    .or_else(|| get_nested_str(r, "server_name"))
                    .or_else(|| get_nested_str(r, "serverName"))
            })
            .map(str::to_string)
            .filter(|s| !is_placeholder_sni(s));

        let effective_sni = node
            .reality_sni
            .as_ref()
            .filter(|s| !is_placeholder_sni(s))
            .cloned()
            .or(inbound_sni)
            .unwrap_or(fallback_sni);

        let public_key = reality
            .and_then(|r| {
                get_nested_str(r, "public_key").or_else(|| get_nested_str(r, "publicKey"))
            })
            .map(str::to_string)
            .or_else(|| node.reality_pub.clone())
            .unwrap_or_default();

        let short_id = reality
            .and_then(|r| {
                get_nested_array_first_str(r, "short_ids")
                    .or_else(|| get_nested_array_first_str(r, "shortIds"))
                    .or_else(|| get_nested_str(r, "short_id"))
                    .or_else(|| get_nested_str(r, "shortId"))
            })
            .map(str::to_string)
            .or_else(|| node.short_id.clone())
            .unwrap_or_default();

        json!({
            "enabled": true,
            "server_name": effective_sni,
            "reality": {
                "enabled": true,
                "public_key": public_key,
                "short_id": short_id
            },
            "utls": {
                "enabled": true,
                "fingerprint": "chrome"
            }
        })
    } else {
        let tls_settings = stream
            .get("tls_settings")
            .or_else(|| stream.get("tlsSettings"));
        let inbound_sni = tls_settings
            .and_then(|tls| {
                get_nested_str(tls, "server_name").or_else(|| get_nested_str(tls, "serverName"))
            })
            .map(str::to_string)
            .filter(|s| !is_placeholder_sni(s));
        let effective_sni = node
            .reality_sni
            .as_ref()
            .filter(|s| !is_placeholder_sni(s))
            .cloned()
            .or(inbound_sni)
            .unwrap_or(fallback_sni);

        json!({
            "enabled": true,
            "server_name": effective_sni,
            "utls": {
                "enabled": true,
                "fingerprint": "chrome"
            }
        })
    }
}

fn ws_transport(stream: &Value) -> Value {
    let ws = stream
        .get("ws_settings")
        .or_else(|| stream.get("wsSettings"));
    json!({
        "type": "ws",
        "path": ws.and_then(|v| get_nested_str(v, "path")).unwrap_or("/"),
        "headers": ws
            .and_then(|v| v.get("headers"))
            .cloned()
            .unwrap_or_else(|| json!({}))
    })
}

fn httpupgrade_transport(stream: &Value, node: &Node) -> Value {
    let http = stream
        .get("httpUpgradeSettings")
        .or_else(|| stream.get("http_upgrade_settings"))
        .or_else(|| stream.get("httpupgrade_settings"));
    let host = http
        .and_then(|v| get_nested_str(v, "host"))
        .map(str::to_string)
        .unwrap_or_else(|| best_node_sni(node));
    json!({
        "type": "httpupgrade",
        "path": http.and_then(|v| get_nested_str(v, "path")).unwrap_or("/"),
        "host": host
    })
}

fn grpc_transport(stream: &Value) -> Value {
    let grpc = stream
        .get("grpcSettings")
        .or_else(|| stream.get("grpc_settings"));
    json!({
        "type": "grpc",
        "service_name": grpc
            .and_then(|v| get_nested_str(v, "service_name").or_else(|| get_nested_str(v, "serviceName")))
            .unwrap_or("grpc")
    })
}

fn build_vless_outbound(
    node: &Node,
    inbound: &Inbound,
    stream: &Value,
    user_keys: &UserKeys,
    tag: &str,
    detour: Option<&str>,
) -> Result<Value> {
    let security = stream_security(stream);
    if security != "tls" && security != "reality" {
        return Err(anyhow!(
            "inbound '{}' on node '{}' is not TLS/Reality",
            inbound.tag,
            node.name
        ));
    }
    let network = stream_network(stream);

    let mut outbound = json!({
        "type": "vless",
        "tag": tag,
        "server": node.ip,
        "server_port": inbound.listen_port,
        "uuid": user_keys.user_uuid,
        "packet_encoding": "xudp",
        "tls": reality_and_tls_block(node, stream, &security)
    });

    if security == "reality" && network == "tcp" {
        outbound["flow"] = json!("xtls-rprx-vision");
    }

    match network.as_str() {
        "ws" => outbound["transport"] = ws_transport(stream),
        "httpupgrade" => outbound["transport"] = httpupgrade_transport(stream, node),
        "grpc" => outbound["transport"] = grpc_transport(stream),
        "tcp" => {}
        other => {
            return Err(anyhow!(
                "unsupported VLESS network '{}' for inbound '{}' on node '{}'",
                other,
                inbound.tag,
                node.name
            ));
        }
    }

    if let Some(detour_tag) = detour {
        outbound["detour"] = json!(detour_tag);
    }

    Ok(outbound)
}

fn build_hysteria2_outbound(
    node: &Node,
    inbound: &Inbound,
    user_keys: &UserKeys,
    tag: &str,
    detour: Option<&str>,
) -> Value {
    let mut outbound = json!({
        "type": "hysteria2",
        "tag": tag,
        "server": node.ip,
        "server_port": inbound.listen_port,
        "password": user_keys.hy2_password,
        "tls": {
            "enabled": true,
            "server_name": best_node_sni(node)
        }
    });

    if let Some(detour_tag) = detour {
        outbound["detour"] = json!(detour_tag);
    }

    outbound
}

fn stable_subscription_hash(value: &str) -> u64 {
    // FNV-1a with fixed constants to keep selection deterministic across restarts.
    let mut hash = 14695981039346656037u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211u64);
    }
    hash
}

fn deterministic_relay_index(subscription_uuid: &str, relay_count: usize) -> usize {
    debug_assert!(relay_count > 0);
    (stable_subscription_hash(subscription_uuid) % relay_count as u64) as usize
}

fn select_exit_and_relay<'a>(
    internal_nodes: &'a [InternalNode],
    subscription_uuid: &str,
    preferred_exit_id: Option<i64>,
) -> Result<(&'a InternalNode, Option<&'a InternalNode>)> {
    let mut exit_nodes = internal_nodes
        .iter()
        .filter(|n| n.node.normalized_node_type() == "exit")
        .collect::<Vec<_>>();
    if exit_nodes.is_empty() {
        return Err(anyhow!("no active exit nodes available"));
    }
    exit_nodes.sort_by_key(|node| node.node.id);

    let exit_node = preferred_exit_id
        .and_then(|exit_id| exit_nodes.iter().copied().find(|n| n.node.id == exit_id))
        .unwrap_or(exit_nodes[0]);

    let mut relay_nodes = internal_nodes
        .iter()
        .filter(|n| n.node.normalized_node_type() == "relay")
        .collect::<Vec<_>>();
    if relay_nodes.is_empty() {
        return Ok((exit_node, None));
    }
    relay_nodes.sort_by_key(|node| node.node.id);

    let selected_index = deterministic_relay_index(subscription_uuid, relay_nodes.len());
    let relay_node = relay_nodes[selected_index];

    if relay_node.node.id == exit_node.node.id {
        return Err(anyhow!(
            "relay node and exit node resolved to the same id ({})",
            relay_node.node.id
        ));
    }

    Ok((exit_node, Some(relay_node)))
}

impl ConfigGenerator {
    pub fn generate(
        internal_nodes: Vec<InternalNode>,
        subscription_uuid: &str,
        user_keys: &UserKeys,
        preferred_exit_id: Option<i64>,
    ) -> Result<Value> {
        let (exit_node, relay_node) =
            select_exit_and_relay(&internal_nodes, subscription_uuid, preferred_exit_id)?;
        let has_relay = relay_node.is_some();
        let variants: &[VariantSpec] = if has_relay {
            &FULL_VARIANTS
        } else {
            &DIRECT_VARIANTS
        };
        let expected_tags: &[&str] = if has_relay {
            &EXPECTED_TAGS_FULL
        } else {
            &EXPECTED_TAGS_DIRECT
        };

        let mut outbounds = Vec::with_capacity(variants.len());

        for variant in variants {
            let source = match variant.scope {
                NodeScope::Relay => relay_node
                    .ok_or_else(|| anyhow!("relay variant requested without relay node"))?,
                NodeScope::Exit => exit_node,
            };

            let outbound = match variant.kind {
                VariantKind::VlessTls(network) => {
                    let (inbound, stream) =
                        pick_vless_tls_inbound(source, network).ok_or_else(|| {
                            anyhow!(
                                "missing VLESS+TLS+{} inbound on {} node '{}'",
                                network,
                                source.node.normalized_node_type(),
                                source.node.name
                            )
                        })?;
                    build_vless_outbound(
                        &source.node,
                        inbound,
                        &stream,
                        user_keys,
                        variant.tag,
                        variant.detour,
                    )?
                }
                VariantKind::VlessReality => {
                    let (inbound, stream) =
                        pick_vless_reality_inbound(source).ok_or_else(|| {
                            anyhow!(
                                "missing VLESS+Reality inbound on {} node '{}'",
                                source.node.normalized_node_type(),
                                source.node.name
                            )
                        })?;
                    build_vless_outbound(
                        &source.node,
                        inbound,
                        &stream,
                        user_keys,
                        variant.tag,
                        variant.detour,
                    )?
                }
                VariantKind::Hysteria2 => {
                    let (inbound, _) = pick_hysteria2_inbound(source).ok_or_else(|| {
                        anyhow!(
                            "missing Hysteria2 inbound on {} node '{}'",
                            source.node.normalized_node_type(),
                            source.node.name
                        )
                    })?;
                    build_hysteria2_outbound(
                        &source.node,
                        inbound,
                        user_keys,
                        variant.tag,
                        variant.detour,
                    )
                }
            };

            outbounds.push(outbound);
        }

        if outbounds.len() != variants.len() {
            return Err(anyhow!(
                "generator contract violation: expected {} outbounds, got {}",
                variants.len(),
                outbounds.len(),
            ));
        }

        let mut seen = HashSet::new();
        let tags = outbounds
            .iter()
            .map(|outbound| {
                outbound
                    .get("tag")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("outbound is missing tag"))
            })
            .collect::<Result<Vec<_>>>()?;
        for tag in &tags {
            if !seen.insert((*tag).to_string()) {
                return Err(anyhow!("duplicate outbound tag '{}'", tag));
            }
        }
        if tags.as_slice() != expected_tags {
            return Err(anyhow!(
                "generator contract violation: outbounds are not in strict zero-padded order"
            ));
        }

        Ok(json!({
            "log": { "level": "info", "timestamp": true },
            "inbounds": [],
            "outbounds": outbounds,
            "route": {
                "auto_detect_interface": true,
                "final": expected_tags[0],
                "rules": []
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: i64, name: &str, ip: &str, node_type: &str, relay_id: Option<i64>) -> Node {
        Node {
            id,
            name: name.to_string(),
            ip: ip.to_string(),
            vpn_port: 443,
            reality_pub: Some("pub_key".to_string()),
            reality_priv: None,
            short_id: Some("a1".to_string()),
            domain: Some("cdn.example.com".to_string()),
            country_code: Some("RU".to_string()),
            node_type: node_type.to_string(),
            is_relay: node_type == "relay",
            relay_id,
            join_token: None,
            config_block_torrent: false,
            config_block_ads: false,
            config_block_porn: false,
            reality_sni: Some("cdn.example.com".to_string()),
        }
    }

    fn make_vless_inbound(
        id: i64,
        node_id: i64,
        tag: &str,
        port: i32,
        security: &str,
        network: &str,
    ) -> Inbound {
        let mut stream = json!({
            "security": security,
            "network": network
        });
        if security == "tls" {
            stream["tlsSettings"] = json!({"serverName": "tls.example.com"});
        }
        if security == "reality" {
            stream["realitySettings"] = json!({
                "serverNames": ["reality.example.com"],
                "publicKey": "pub_key",
                "shortIds": ["a1"]
            });
        }
        if network == "ws" {
            stream["wsSettings"] = json!({"path": "/ws", "headers": {"Host": "ws.example.com"}});
        }
        if network == "httpupgrade" {
            stream["httpUpgradeSettings"] = json!({"path": "/hu", "host": "hu.example.com"});
        }
        if network == "grpc" {
            stream["grpcSettings"] = json!({"serviceName": "grpc-service"});
        }

        Inbound {
            id,
            node_id,
            tag: tag.to_string(),
            protocol: "vless".to_string(),
            listen_ip: "::".to_string(),
            listen_port: port,
            settings: "{}".to_string(),
            stream_settings: stream.to_string(),
            enable: true,
        }
    }

    fn make_hy2_inbound(id: i64, node_id: i64, tag: &str, port: i32) -> Inbound {
        Inbound {
            id,
            node_id,
            tag: tag.to_string(),
            protocol: "hysteria2".to_string(),
            listen_ip: "::".to_string(),
            listen_port: port,
            settings: "{}".to_string(),
            stream_settings: "{}".to_string(),
            enable: true,
        }
    }

    fn build_complete_fixture() -> (Vec<InternalNode>, UserKeys) {
        let relay = InternalNode {
            node: make_node(1, "relay-1", "10.0.0.1", "relay", None),
            inbounds: vec![
                make_vless_inbound(1, 1, "relay-ws", 4431, "tls", "ws"),
                make_vless_inbound(2, 1, "relay-httpupgrade", 4432, "tls", "httpupgrade"),
                make_vless_inbound(3, 1, "relay-grpc", 4433, "tls", "grpc"),
                make_vless_inbound(4, 1, "relay-tcp", 4434, "tls", "tcp"),
                make_hy2_inbound(5, 1, "relay-hy2", 4435),
                make_vless_inbound(6, 1, "relay-reality", 4436, "reality", "tcp"),
            ],
        };

        let exit = InternalNode {
            node: make_node(2, "exit-1", "10.0.0.2", "exit", Some(1)),
            inbounds: vec![
                make_vless_inbound(7, 2, "exit-reality", 4441, "reality", "tcp"),
                make_vless_inbound(8, 2, "exit-grpc", 4442, "tls", "grpc"),
                make_hy2_inbound(9, 2, "exit-hy2", 4443),
            ],
        };

        let keys = UserKeys {
            user_uuid: "11111111-1111-1111-1111-111111111111".to_string(),
            hy2_password: "hy2-pass".to_string(),
        };

        (vec![relay, exit], keys)
    }

    fn build_multi_relay_fixture(reverse_order: bool) -> (Vec<InternalNode>, UserKeys) {
        let relay_one = InternalNode {
            node: make_node(1, "relay-1", "10.0.0.1", "relay", None),
            inbounds: vec![
                make_vless_inbound(1, 1, "relay1-ws", 4431, "tls", "ws"),
                make_vless_inbound(2, 1, "relay1-httpupgrade", 4432, "tls", "httpupgrade"),
                make_vless_inbound(3, 1, "relay1-grpc", 4433, "tls", "grpc"),
                make_vless_inbound(4, 1, "relay1-tcp", 4434, "tls", "tcp"),
                make_hy2_inbound(5, 1, "relay1-hy2", 4435),
                make_vless_inbound(6, 1, "relay1-reality", 4436, "reality", "tcp"),
            ],
        };
        let relay_two = InternalNode {
            node: make_node(3, "relay-2", "10.0.0.3", "relay", None),
            inbounds: vec![
                make_vless_inbound(10, 3, "relay2-ws", 4531, "tls", "ws"),
                make_vless_inbound(11, 3, "relay2-httpupgrade", 4532, "tls", "httpupgrade"),
                make_vless_inbound(12, 3, "relay2-grpc", 4533, "tls", "grpc"),
                make_vless_inbound(13, 3, "relay2-tcp", 4534, "tls", "tcp"),
                make_hy2_inbound(14, 3, "relay2-hy2", 4535),
                make_vless_inbound(15, 3, "relay2-reality", 4536, "reality", "tcp"),
            ],
        };
        let exit = InternalNode {
            node: make_node(2, "exit-1", "10.0.0.2", "exit", None),
            inbounds: vec![
                make_vless_inbound(7, 2, "exit-reality", 4441, "reality", "tcp"),
                make_vless_inbound(8, 2, "exit-grpc", 4442, "tls", "grpc"),
                make_hy2_inbound(9, 2, "exit-hy2", 4443),
            ],
        };

        let keys = UserKeys {
            user_uuid: "11111111-1111-1111-1111-111111111111".to_string(),
            hy2_password: "hy2-pass".to_string(),
        };

        if reverse_order {
            (vec![relay_two, exit, relay_one], keys)
        } else {
            (vec![relay_one, exit, relay_two], keys)
        }
    }

    #[test]
    fn generates_exactly_nine_variants_in_strict_order() {
        let (nodes, keys) = build_complete_fixture();
        let cfg =
            ConfigGenerator::generate(nodes, "sub-a", &keys, Some(2)).expect("config generation");

        let outbounds = cfg["outbounds"].as_array().expect("outbounds array");
        assert_eq!(outbounds.len(), 9);

        let tags = outbounds
            .iter()
            .map(|o| o["tag"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(tags, EXPECTED_TAGS_FULL);

        let expected_detours = [
            Some(FULL_TAG_07),
            Some(FULL_TAG_07),
            Some(FULL_TAG_08),
            Some(FULL_TAG_07),
            Some(FULL_TAG_09),
            Some(FULL_TAG_07),
        ];
        for (idx, expected) in expected_detours.iter().enumerate() {
            assert_eq!(
                outbounds[idx].get("detour").and_then(Value::as_str),
                *expected
            );
        }
        for outbound in outbounds.iter().take(6) {
            assert_eq!(
                outbound.get("server").and_then(Value::as_str),
                Some("10.0.0.1")
            );
        }
        for outbound in outbounds.iter().skip(6) {
            assert_eq!(
                outbound.get("server").and_then(Value::as_str),
                Some("10.0.0.2")
            );
        }
        for outbound in outbounds.iter().skip(6) {
            assert!(outbound.get("detour").is_none());
        }

        assert_eq!(cfg["route"]["final"].as_str(), Some(EXPECTED_TAGS_FULL[0]));
    }

    #[test]
    fn generates_direct_only_fallback_when_no_relays_available() {
        let (nodes, keys) = build_complete_fixture();
        let exit_only = nodes
            .into_iter()
            .filter(|n| n.node.normalized_node_type() == "exit")
            .collect::<Vec<_>>();

        let cfg = ConfigGenerator::generate(exit_only, "sub-no-relay", &keys, Some(2))
            .expect("direct-only fallback");
        let outbounds = cfg["outbounds"].as_array().expect("outbounds array");
        assert_eq!(outbounds.len(), 3);

        let tags = outbounds
            .iter()
            .map(|o| o["tag"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(tags, EXPECTED_TAGS_DIRECT);
        assert!(
            outbounds
                .iter()
                .all(|outbound| outbound.get("detour").is_none())
        );
        assert_eq!(
            cfg["route"]["final"].as_str(),
            Some(EXPECTED_TAGS_DIRECT[0])
        );
    }

    #[test]
    fn selects_single_relay_deterministically_by_subscription_hash() {
        let subscription_uuid = "sub-deterministic";
        let selected_index = deterministic_relay_index(subscription_uuid, 2);
        let expected_relay_ip = if selected_index == 0 {
            "10.0.0.1"
        } else {
            "10.0.0.3"
        };

        let (nodes_a, keys_a) = build_multi_relay_fixture(false);
        let cfg_a = ConfigGenerator::generate(nodes_a, subscription_uuid, &keys_a, Some(2))
            .expect("config generation A");
        let relay_outbound_a = &cfg_a["outbounds"].as_array().expect("outbounds A")[0];
        assert_eq!(relay_outbound_a["server"].as_str(), Some(expected_relay_ip));

        let (nodes_b, keys_b) = build_multi_relay_fixture(true);
        let cfg_b = ConfigGenerator::generate(nodes_b, subscription_uuid, &keys_b, Some(2))
            .expect("config generation B");
        let relay_outbound_b = &cfg_b["outbounds"].as_array().expect("outbounds B")[0];
        assert_eq!(relay_outbound_b["server"].as_str(), Some(expected_relay_ip));
    }

    #[test]
    fn fails_when_required_direct_variant_is_missing() {
        let (mut nodes, keys) = build_complete_fixture();
        if let Some(exit) = nodes.iter_mut().find(|n| n.node.id == 2) {
            exit.inbounds.retain(|inbound| inbound.tag != "exit-grpc");
        }

        let err = ConfigGenerator::generate(nodes, "sub-a", &keys, Some(2)).expect_err("must fail");
        assert!(err.to_string().contains("missing VLESS+TLS+grpc inbound"));
    }
}
