use crate::singbox::subscription_generator::NodeInfo;
use anyhow::Result;
use caramba_db::models::network::Inbound;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct SingboxConnectionVariant {
    pub id: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
    pub family: &'static str,
    pub transport: &'static str,
    pub relay: bool,
}

const FIXED_VARIANTS: [SingboxConnectionVariant; 7] = [
    SingboxConnectionVariant {
        id: "vless-reality-direct",
        label: "VLESS Reality",
        summary: "Direct VLESS Reality for normal networks and Wi-Fi.",
        family: "vless",
        transport: "tcp-reality",
        relay: false,
    },
    SingboxConnectionVariant {
        id: "vless-httpupgrade-direct",
        label: "VLESS HTTPUpgrade",
        summary: "Direct HTTPUpgrade profile with low overhead.",
        family: "vless",
        transport: "httpupgrade",
        relay: false,
    },
    SingboxConnectionVariant {
        id: "vless-httpupgrade-relay",
        label: "VLESS HTTPUpgrade Relay",
        summary: "HTTPUpgrade through relay for restrictive mobile networks.",
        family: "vless",
        transport: "httpupgrade",
        relay: true,
    },
    SingboxConnectionVariant {
        id: "vless-ws-relay",
        label: "VLESS WS Relay",
        summary: "WebSocket relay fallback when HTTPUpgrade is filtered.",
        family: "vless",
        transport: "ws",
        relay: true,
    },
    SingboxConnectionVariant {
        id: "grpc-auto",
        label: "gRPC Auto",
        summary: "Auto-pick the best gRPC path among available endpoints.",
        family: "grpc",
        transport: "grpc",
        relay: false,
    },
    SingboxConnectionVariant {
        id: "grpc-direct",
        label: "gRPC Direct",
        summary: "Direct gRPC profile for open networks.",
        family: "grpc",
        transport: "grpc",
        relay: false,
    },
    SingboxConnectionVariant {
        id: "grpc-relay",
        label: "gRPC Relay",
        summary: "gRPC through relay for difficult mobile routes.",
        family: "grpc",
        transport: "grpc",
        relay: true,
    },
];

pub fn fixed_connection_variants() -> Vec<SingboxConnectionVariant> {
    FIXED_VARIANTS.to_vec()
}

pub fn available_connection_variants_for_node(node: &NodeInfo) -> Vec<SingboxConnectionVariant> {
    FIXED_VARIANTS
        .iter()
        .copied()
        .filter(|variant| node_supports_variant(node, *variant))
        .collect()
}

pub fn apply_connection_variant(config: &str, variant_id: &str) -> Result<String> {
    let Some(variant) = FIXED_VARIANTS
        .iter()
        .find(|candidate| candidate.id == variant_id)
    else {
        return Ok(config.to_string());
    };

    let mut parsed: Value = serde_json::from_str(config)?;
    let Some(outbounds) = parsed.get_mut("outbounds").and_then(Value::as_array_mut) else {
        return Ok(config.to_string());
    };

    let matching_tags = collect_matching_tags(outbounds, *variant);
    if matching_tags.is_empty() {
        return Ok(config.to_string());
    }

    let preferred_tag = format!("variant::{}", variant.id);
    if !outbounds
        .iter()
        .any(|outbound| tag_of(outbound) == Some(preferred_tag.as_str()))
    {
        outbounds.insert(
            1,
            json!({
                "type": "urltest",
                "tag": preferred_tag.clone(),
                "outbounds": matching_tags,
                "url": "https://www.gstatic.com/generate_204",
                "interval": "3m",
                "tolerance": 50
            }),
        );
    }

    if let Some(selector) = outbounds.iter_mut().find(|outbound| {
        outbound.get("type").and_then(Value::as_str) == Some("selector")
            && tag_of(outbound) == Some("proxy")
    }) {
        if let Some(selector_outbounds) =
            selector.get_mut("outbounds").and_then(Value::as_array_mut)
        {
            selector_outbounds.retain(|value| value.as_str() != Some(preferred_tag.as_str()));
            selector_outbounds.insert(0, Value::String(preferred_tag.clone()));
        }
        selector["default"] = Value::String(preferred_tag);
    }

    Ok(serde_json::to_string(&parsed)?)
}

fn collect_matching_tags(outbounds: &[Value], variant: SingboxConnectionVariant) -> Vec<String> {
    outbounds
        .iter()
        .filter_map(|outbound| {
            if !is_supported_vless_outbound(outbound)
                || !variant_matches_outbound(variant, outbound)
            {
                return None;
            }

            tag_of(outbound).map(ToOwned::to_owned)
        })
        .collect()
}

fn is_supported_vless_outbound(outbound: &Value) -> bool {
    outbound.get("type").and_then(Value::as_str) == Some("vless")
}

fn variant_matches_outbound(variant: SingboxConnectionVariant, outbound: &Value) -> bool {
    let transport = transport_of(outbound);
    let is_relay = outbound.get("detour").and_then(Value::as_str).is_some();
    let is_reality = outbound
        .get("tls")
        .and_then(|tls| tls.get("reality"))
        .and_then(|reality| reality.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    match variant.id {
        "vless-reality-direct" => !is_relay && is_reality && transport == "tcp",
        "vless-httpupgrade-direct" => !is_relay && transport == "httpupgrade",
        "vless-httpupgrade-relay" => is_relay && transport == "httpupgrade",
        "vless-ws-relay" => is_relay && transport == "ws",
        "grpc-auto" => transport == "grpc",
        "grpc-direct" => !is_relay && transport == "grpc",
        "grpc-relay" => is_relay && transport == "grpc",
        _ => false,
    }
}

fn tag_of(outbound: &Value) -> Option<&str> {
    outbound.get("tag").and_then(Value::as_str)
}

fn transport_of(outbound: &Value) -> &str {
    outbound
        .get("transport")
        .and_then(|transport| transport.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("tcp")
}

fn node_supports_variant(node: &NodeInfo, variant: SingboxConnectionVariant) -> bool {
    if node.is_relay {
        return false;
    }

    match variant.id {
        "vless-reality-direct" => has_vless_inbound(node, false, |network, security| {
            network == "tcp" && security == "reality"
        }),
        "vless-httpupgrade-direct" => {
            has_vless_inbound(node, false, |network, _| network == "httpupgrade")
        }
        "vless-httpupgrade-relay" => {
            has_vless_inbound(node, true, |network, _| network == "httpupgrade")
        }
        "vless-ws-relay" => has_vless_inbound(node, true, |network, _| network == "ws"),
        "grpc-auto" | "grpc-direct" => {
            has_vless_inbound(node, false, |network, _| network == "grpc")
        }
        "grpc-relay" => has_vless_inbound(node, true, |network, _| network == "grpc"),
        _ => false,
    }
}

fn has_vless_inbound<F>(node: &NodeInfo, requires_relay: bool, predicate: F) -> bool
where
    F: Fn(&str, &str) -> bool,
{
    if requires_relay && node.relay_info.is_none() {
        return false;
    }

    node.inbounds.iter().any(|inbound| {
        if !inbound.enable || inbound.protocol != "vless" {
            return false;
        }

        let (network, security) = inbound_network_security(inbound);
        predicate(network.as_str(), security.as_str())
    })
}

fn inbound_network_security(inbound: &Inbound) -> (String, String) {
    let parsed: Value =
        serde_json::from_str(&inbound.stream_settings).unwrap_or_else(|_| json!({}));

    let network = parsed
        .get("network")
        .and_then(Value::as_str)
        .unwrap_or("tcp")
        .to_string();

    let security = parsed
        .get("security")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if parsed.get("realitySettings").is_some() || parsed.get("reality_settings").is_some() {
                "reality".to_string()
            } else {
                "none".to_string()
            }
        });

    (network, security)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_connection_variant, available_connection_variants_for_node, fixed_connection_variants,
    };
    use crate::singbox::subscription_generator::{NodeInfo, UserKeys, generate_singbox_config};
    use caramba_db::models::network::Inbound;
    use serde_json::json;

    #[test]
    fn returns_expected_fixed_variant_count() {
        let variants = fixed_connection_variants();
        assert_eq!(variants.len(), 7);
        assert_eq!(variants[0].id, "vless-reality-direct");
        assert_eq!(variants[6].id, "grpc-relay");
    }

    #[test]
    fn returns_only_supported_variants_for_node() {
        let mut node = build_target_node_fixture();
        node.relay_info = None;

        let variants = available_connection_variants_for_node(&node);
        let ids: Vec<&str> = variants.iter().map(|variant| variant.id).collect();

        assert!(ids.contains(&"vless-reality-direct"));
        assert!(ids.contains(&"vless-httpupgrade-direct"));
        assert!(ids.contains(&"grpc-auto"));
        assert!(ids.contains(&"grpc-direct"));
        assert!(!ids.contains(&"vless-httpupgrade-relay"));
        assert!(!ids.contains(&"vless-ws-relay"));
        assert!(!ids.contains(&"grpc-relay"));
    }

    #[test]
    fn returns_relay_variants_when_relay_and_matching_inbounds_exist() {
        let node = build_target_node_fixture();
        let variants = available_connection_variants_for_node(&node);
        let ids: Vec<&str> = variants.iter().map(|variant| variant.id).collect();

        assert!(ids.contains(&"vless-httpupgrade-relay"));
        assert!(ids.contains(&"vless-ws-relay"));
        assert!(ids.contains(&"grpc-relay"));
    }

    #[test]
    fn applies_httpupgrade_relay_variant_as_selector_default() {
        let config = build_config_fixture();
        let variant_config = apply_connection_variant(&config, "vless-httpupgrade-relay").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&variant_config).unwrap();

        let selector = parsed["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|outbound| outbound["tag"] == "proxy")
            .unwrap();

        assert_eq!(selector["default"], "variant::vless-httpupgrade-relay");

        let variant_group = parsed["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|outbound| outbound["tag"] == "variant::vless-httpupgrade-relay")
            .unwrap();

        let picked_tags = variant_group["outbounds"].as_array().unwrap();
        assert!(
            picked_tags
                .iter()
                .all(|tag| tag.as_str().unwrap().ends_with("·r"))
        );
    }

    #[test]
    fn applies_grpc_auto_variant_with_only_grpc_tags() {
        let config = build_config_fixture();
        let variant_config = apply_connection_variant(&config, "grpc-auto").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&variant_config).unwrap();

        let variant_group = parsed["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|outbound| outbound["tag"] == "variant::grpc-auto")
            .unwrap();

        let picked_tags = variant_group["outbounds"].as_array().unwrap();
        assert!(!picked_tags.is_empty());
        assert!(
            picked_tags
                .iter()
                .all(|tag| tag.as_str().unwrap().contains("grpc"))
        );
    }

    fn build_config_fixture() -> String {
        let target_node = build_target_node_fixture();
        let user_keys = UserKeys {
            user_uuid: "uuid-123".to_string(),
            hy2_password: "tg:uuid-123".to_string(),
            _awg_private_key: None,
        };

        generate_singbox_config(&match_any_sub(), &[target_node], &user_keys).unwrap()
    }

    fn build_target_node_fixture() -> NodeInfo {
        let relay_inbound = make_inbound(
            2,
            "relay_entry",
            "vless",
            8443,
            json!({
                "network": "tcp",
                "security": "reality",
                "realitySettings": {
                    "serverNames": ["relay.example.com"],
                    "publicKey": "relay-pub",
                    "shortIds": ["relay-id"]
                }
            }),
        );

        let relay_node = NodeInfo {
            name: "RelayNode".to_string(),
            address: "10.0.0.2".to_string(),
            reality_port: Some(8443),
            reality_sni: Some("relay.example.com".to_string()),
            reality_public_key: Some("relay-pub".to_string()),
            reality_short_id: Some("relay-id".to_string()),
            hy2_port: None,
            hy2_sni: None,
            frontend_url: None,
            inbounds: vec![relay_inbound],
            relay_info: None,
            country_code: Some("RU".to_string()),
            is_relay: true,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        };

        NodeInfo {
            name: "TargetNode".to_string(),
            address: "10.0.0.3".to_string(),
            reality_port: Some(443),
            reality_sni: Some("target.example.com".to_string()),
            reality_public_key: Some("target-pub".to_string()),
            reality_short_id: Some("target-id".to_string()),
            hy2_port: None,
            hy2_sni: None,
            frontend_url: Some("edge.example.com".to_string()),
            inbounds: vec![
                make_inbound(
                    1,
                    "reality_tcp",
                    "vless",
                    443,
                    json!({
                        "network": "tcp",
                        "security": "reality",
                        "realitySettings": {
                            "serverNames": ["target.example.com"],
                            "publicKey": "target-pub",
                            "shortIds": ["target-id"]
                        }
                    }),
                ),
                make_inbound(
                    1,
                    "httpupgrade",
                    "vless",
                    8443,
                    json!({
                        "network": "httpupgrade",
                        "security": "tls",
                        "tlsSettings": { "serverName": "edge.example.com" },
                        "wsSettings": { "path": "/hu" }
                    }),
                ),
                make_inbound(
                    1,
                    "grpc",
                    "vless",
                    9443,
                    json!({
                        "network": "grpc",
                        "security": "tls",
                        "tlsSettings": { "serverName": "edge.example.com" },
                        "grpcSettings": { "serviceName": "edge-grpc" }
                    }),
                ),
                make_inbound(
                    1,
                    "ws",
                    "vless",
                    7443,
                    json!({
                        "network": "ws",
                        "security": "tls",
                        "tlsSettings": { "serverName": "edge.example.com" },
                        "wsSettings": { "path": "/ws" }
                    }),
                ),
            ],
            relay_info: Some(Box::new(relay_node)),
            country_code: Some("DE".to_string()),
            is_relay: false,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        }
    }

    fn make_inbound(
        node_id: i64,
        tag: &str,
        protocol: &str,
        port: i64,
        stream_settings: serde_json::Value,
    ) -> Inbound {
        Inbound {
            id: 1,
            node_id,
            tag: tag.to_string(),
            protocol: protocol.to_string(),
            listen_port: port,
            listen_ip: "0.0.0.0".to_string(),
            settings: "{}".to_string(),
            stream_settings: stream_settings.to_string(),
            remark: Some(tag.to_string()),
            enable: true,
            renew_interval_mins: 0,
            port_range_start: 0,
            port_range_end: 0,
            last_rotated_at: None,
            created_at: None,
        }
    }

    fn match_any_sub() -> caramba_db::models::store::Subscription {
        serde_json::from_value(json!({
            "id": 1,
            "user_id": 1,
            "plan_id": 1,
            "status": "active",
            "created_at": "2023-01-01T00:00:00Z",
            "updated_at": "2023-01-01T00:00:00Z",
            "expires_at": "2024-01-01T00:00:00Z",
            "used_traffic": 0,
            "is_trial": false,
            "subscription_uuid": "uuid"
        }))
        .unwrap()
    }
}
