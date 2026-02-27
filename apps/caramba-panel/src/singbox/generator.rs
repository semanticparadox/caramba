use crate::singbox::config::*;
use crate::singbox::inbound_factory::{InboundFactory, RelayAuthMode};
use sha2::{Digest, Sha256};
use tracing::{error, warn};

pub struct ConfigGenerator;

fn parse_shadowsocks_method(settings_raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(settings_raw).ok()?;
    value
        .get("method")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}

fn derive_relay_password(join_token: &str, target_node_id: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(join_token.trim().as_bytes());
    hasher.update(b":relay:");
    hasher.update(target_node_id.to_string().as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)
}

impl ConfigGenerator {
    /// Generates a complete Sing-box configuration from a list of database Inbounds
    pub fn generate_config(
        node: &caramba_db::models::node::Node,
        inbounds: Vec<caramba_db::models::network::Inbound>,
        target_node: Option<caramba_db::models::node::Node>,
        relay_target_inbound: Option<caramba_db::models::network::Inbound>,
        relay_clients: Vec<caramba_db::models::node::Node>,
        relay_auth_mode: RelayAuthMode,
    ) -> SingBoxConfig {
        let mut generated_inbounds = Vec::new();

        // 1. Process Inbounds (Normal + Relay Injection)
        // Use InboundFactory to generate Inbounds
        for inbound in inbounds {
            if !inbound.enable {
                error!(
                    "🚫 Inbound {} is DISABLED, skipping generation",
                    inbound.tag
                );
                continue;
            }

            // InboundFactory now handles all inbound generation including relay injection for SS
            let factory_inbounds = InboundFactory::generate_inbounds(
                node,
                &inbound,
                &relay_clients,
                relay_auth_mode,
            );
            generated_inbounds.extend(factory_inbounds);
        }

        // 2. Generate Outbounds (Standard + Relay)
        let mut outbounds = vec![Outbound::Direct {
            tag: "direct".to_string(),
        }];

        // 3. Relay Logic: Add Relay Outbound if enabled
        let mut default_outbound_tag = "direct".to_string();

        if let Some(target) = target_node {
            if node.is_relay {
                warn!(
                    "🔗 Configuring Node as RELAY -> Target: {} ({})",
                    target.name, target.ip
                );
                let relay_password = node
                    .join_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|token| match relay_auth_mode {
                        RelayAuthMode::Legacy => token.to_string(),
                        RelayAuthMode::V1 | RelayAuthMode::Dual => {
                            derive_relay_password(token, target.id)
                        }
                    });

                if relay_password.is_none() {
                    warn!(
                        "⚠️ Relay mode requested for node {} but join_token is missing. Skipping relay detour.",
                        node.id
                    );
                } else if let Some(target_inbound) = relay_target_inbound.as_ref() {
                    let relay_port = target_inbound.listen_port as u16;
                    let relay_method = parse_shadowsocks_method(&target_inbound.settings)
                        .unwrap_or_else(|| "chacha20-ietf-poly1305".to_string());

                    outbounds.push(Outbound::Shadowsocks(ShadowsocksOutbound {
                        tag: "relay-out".to_string(),
                        server: target.ip.clone(),
                        server_port: relay_port,
                        method: relay_method,
                        password: relay_password.unwrap_or_default(), // We authenticate using OUR token
                    }));

                    // Override default route to Relay
                    default_outbound_tag = "relay-out".to_string();
                } else {
                    warn!(
                        "⚠️ Relay mode requested for node {} but target node {} has no active shadowsocks inbound. Skipping relay detour.",
                        node.id, target.id
                    );
                }
            }
        }

        // 4. Rule Sets & Blocking Logic
        let mut rule_sets = Vec::new();
        let mut router_rules = Vec::new();
        let mut dns_rules = Vec::new();

        // 0. DNS Route (Always first)
        router_rules.push(RouteRule {
            action: Some("route".to_string()),
            protocol: Some(vec!["dns".to_string()]),
            outbound: Some("direct".to_string()),
            port: None,
            domain: None,
            geosite: None,
            geoip: None,
            domain_resolver: None,
            rule_set: None,
        });

        // 1. BitTorrent Blocking (Protocol + Geosite)
        if node.config_block_torrent {
            router_rules.push(RouteRule {
                action: Some("reject".to_string()),
                protocol: Some(vec!["bittorrent".to_string()]),
                outbound: None,
                port: None,
                domain: None,
                geosite: None,
                geoip: None,
                domain_resolver: None,
                rule_set: None,
            });
            // Try to use geosite if available, but keep protocol as primary fallback
            router_rules.push(RouteRule {
                action: Some("reject".to_string()),
                geosite: Some(vec!["category-p2p".to_string()]),
                outbound: None,
                protocol: None,
                port: None,
                domain: None,
                geoip: None,
                domain_resolver: None,
                rule_set: None,
            });
        }

        // 2. Ad Blocking (Remote RuleSet)
        if node.config_block_ads {
            rule_sets.push(RuleSet::Remote(RemoteRuleSet {
                tag: "geosite-ads".to_string(),
                format: "binary".to_string(),
                url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs".to_string(),
                download_detour: Some("direct".to_string()),
                update_interval: Some("24h".to_string()),
            }));

            // Block in DNS
            dns_rules.push(DnsRule {
                rule_set: Some(vec!["geosite-ads".to_string()]),
                server: Some("block".to_string()), // "block" isn't a server, usually "reject" or 127.0.0.1. Sing-box DNS rules don't have "action": "reject".
                // Wait, DNS rules map to a server. We need a "block" server or just use "reject" action in 1.10+?
                // Sing-box 1.9+ DNS rule doesn't have action. It has `server` or `interrupt`.
                // We'll define a fake "block" server or use the route rule to reject.
                // Actually, best practice for DNS AdBlock in sing-box:
                // Define a "block" DNS server (e.g. 0.0.0.0) or use `action: reject` in Route (which handles traffic).
                // But to stop DNS resolution itself:
                // We can't easily do it in `dns.rules` without a sinkhole server.
                // Let's use 127.0.0.1 as a sinkhole server.
                domain_resolver: None,
                clash_mode: None,
                // outbound: None, // Verified removed
            });

            // Block in Route
            router_rules.push(RouteRule {
                action: Some("reject".to_string()),
                rule_set: Some(vec!["geosite-ads".to_string()]),
                outbound: None,
                protocol: None,
                port: None,
                domain: None,
                geosite: None,
                geoip: None,
                domain_resolver: None,
            });
        }

        // 3. Adult Content (Remote RuleSet)
        if node.config_block_porn {
            rule_sets.push(RuleSet::Remote(RemoteRuleSet {
                tag: "geosite-porn".to_string(),
                format: "binary".to_string(),
                url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-porn.srs".to_string(),
                download_detour: Some("direct".to_string()),
                update_interval: Some("24h".to_string()),
            }));

            dns_rules.push(DnsRule {
                rule_set: Some(vec!["geosite-porn".to_string()]),
                server: Some("block".to_string()),
                domain_resolver: None,
                clash_mode: None,
            });

            router_rules.push(RouteRule {
                action: Some("reject".to_string()),
                rule_set: Some(vec!["geosite-porn".to_string()]),
                outbound: None,
                protocol: None,
                port: None,
                domain: None,
                geosite: None,
                geoip: None,
                domain_resolver: None,
            });
        }

        // 4. Default Route
        if default_outbound_tag != "direct" {
            router_rules.push(RouteRule {
                action: Some("route".to_string()),
                outbound: Some(default_outbound_tag),
                protocol: None,
                port: None,
                domain: None,
                geosite: None,
                geoip: None,
                domain_resolver: None,
                rule_set: None,
            });
        }

        SingBoxConfig {
            log: LogConfig {
                level: "info".to_string(),
                timestamp: true,
            },
            dns: Some(DnsConfig {
                servers: vec![
                    DnsServer::Udp(UdpDnsServer {
                        tag: "google".to_string(),
                        server: "8.8.8.8".to_string(),
                        detour: None,
                    }),
                    DnsServer::Local(LocalDnsServer {
                        tag: "local".to_string(),
                        detour: Some("direct".to_string()),
                    }),
                    // Sinkhole for AdBlock
                    DnsServer::Udp(UdpDnsServer {
                        tag: "block".to_string(),
                        server: "127.0.0.1".to_string(),
                        detour: None,
                    }),
                ],
                rules: {
                    let mut final_dns_rules = dns_rules;
                    final_dns_rules.push(DnsRule {
                        domain_resolver: None,
                        server: Some("local".to_string()),
                        clash_mode: None,
                        rule_set: None,
                    });
                    final_dns_rules
                },
            }),
            inbounds: generated_inbounds,
            outbounds,
            route: Some(RouteConfig {
                default_domain_resolver: Some("google".to_string()),
                rules: router_rules,
                rule_set: if rule_sets.is_empty() {
                    None
                } else {
                    Some(rule_sets)
                },
            }),
            // Enable Clash API for device monitoring and limit enforcement
            experimental: Some(ExperimentalConfig {
                clash_api: ClashApiConfig {
                    external_controller: "0.0.0.0:9090".to_string(),
                    secret: None,
                    external_ui: None,
                    access_control_allow_origin: Some(vec!["*".to_string()]),
                    access_control_allow_private_network: Some(true),
                },
            }),
        }
    }

    /// Validates the configuration using the `sing-box` binary
    pub fn validate_config(config: &SingBoxConfig) -> anyhow::Result<()> {
        use std::io::Write;
        use std::process::Command;

        // Serialize to JSON
        let config_json = serde_json::to_string_pretty(config)?;

        // Create temp file
        let mut temp_path = std::env::temp_dir();
        temp_path.push(format!("singbox_check_{}.json", uuid::Uuid::new_v4()));

        // Write to file
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(config_json.as_bytes())?;

        // Run sing-box check
        // We assume sing-box is in PATH. If not, we skip validation to allow running on servers without sing-box installed.
        let output_result = Command::new("sing-box")
            .arg("check")
            .arg("-c")
            .arg(&temp_path)
            .output();

        // Clean up temp file immediately
        let _ = std::fs::remove_file(&temp_path);

        match output_result {
            Ok(out) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return Err(anyhow::anyhow!("Sing-box validation failed: {}", stderr));
                }
            }
            Err(e) => {
                // If the binary is missing or execution fails, we log a warning but DO NOT fail the request.
                // This enables the panel to run on environments where sing-box is not installed.
                warn!(
                    "⚠️ Skipping Sing-box config validation (binary execution failed: {}). Proceeding blindly.",
                    e
                );
            }
        }

        Ok(())
    }
}
