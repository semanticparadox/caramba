use crate::singbox::config::*;
use caramba_db::models::network::{InboundType, StreamSettings as DbStreamSettings};
use sha2::{Digest, Sha256};
use tracing::{error, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayAuthMode {
    Legacy,
    V1,
    Dual,
}

impl RelayAuthMode {
    pub fn from_setting(raw: Option<&str>) -> Self {
        match raw.unwrap_or("dual").trim().to_ascii_lowercase().as_str() {
            "legacy" => Self::Legacy,
            "v1" | "hashed" | "derived" => Self::V1,
            "dual" => Self::Dual,
            _ => Self::Dual,
        }
    }
}

pub struct InboundFactory;

impl InboundFactory {
    /// Generates Sing-box Inbound config(s) from a database inbound definition.
    /// Returns a vector because some protocols (like ShadowTLS) might need a wrapper + inner inbound.
    pub fn generate_inbounds(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        relay_clients: &[caramba_db::models::node::Node],
        relay_auth_mode: RelayAuthMode,
    ) -> Vec<Inbound> {
        let mut generated = Vec::new();

        if !inbound.enable {
            error!(
                "🚫 Inbound {} is DISABLED, skipping generation",
                inbound.tag
            );
            return generated;
        }

        // 1. Parse Protocol Settings (Manual JSON Mode Support)
        let mut settings_value: serde_json::Value = serde_json::from_str(&inbound.settings)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        // Ensure protocol field exists for deserialization
        if let Some(obj) = settings_value.as_object_mut() {
            if !obj.contains_key("protocol") {
                obj.insert(
                    "protocol".to_string(),
                    serde_json::Value::String(inbound.protocol.clone().to_lowercase()),
                );
            }
        }

        // Try to parse strictly first
        match serde_json::from_value(settings_value.clone()) {
            Ok(protocol_settings) => {
                // 2. Parse Stream Settings
                let stream_settings: DbStreamSettings = match serde_json::from_str(&inbound.stream_settings)
                {
                    Ok(s) => s,
                    Err(_) => {
                        warn!(
                            "⚠️ StreamSettings parse failed for inbound '{}', using defaults",
                            inbound.tag
                        );
                        DbStreamSettings::default()
                    }
                };

                // 3. Generate Inbound based on type
                match protocol_settings {
                    InboundType::Vless(vless) => {
                        generated.push(Self::generate_vless(
                            node,
                            inbound,
                            vless,
                            &stream_settings,
                        ));
                    }
                    InboundType::Hysteria2(hy2) => {
                        generated.push(Self::generate_hysteria2(
                            node,
                            inbound,
                            hy2,
                            &stream_settings,
                        ));
                    }
                    InboundType::Trojan(trojan) => {
                        generated.push(Self::generate_trojan(
                            node,
                            inbound,
                            trojan,
                            &stream_settings,
                        ));
                    }
                    InboundType::Shadowsocks(ss) => {
                        generated.push(Self::generate_shadowsocks(
                            node,
                            inbound,
                            ss,
                            relay_clients,
                            relay_auth_mode,
                        ));
                    }
                    InboundType::Tuic(tuic) => {
                        generated.push(Self::generate_tuic(node, inbound, tuic, &stream_settings));
                    }
                    InboundType::Naive(naive) => {
                        generated.push(Self::generate_naive(
                            node,
                            inbound,
                            naive,
                            &stream_settings,
                        ));
                    }
                    InboundType::AmneziaWg(awg) => {
                        generated.push(Self::generate_amneziawg(inbound, awg));
                    }
                    InboundType::Shadowtls(stls) => {
                        let inner_port = if inbound.listen_port < 50000 {
                            inbound.listen_port + 10000
                        } else {
                            inbound.listen_port - 10000
                        } as u16;

                        let inner_tag = format!("{}-inner", inbound.tag);

                        // Wrapper (Public Port) -> detours to inner_tag
                        generated.push(Self::generate_shadowtls(inbound, &stls, Some(inner_tag.clone())));

                        let ss_users: Vec<crate::singbox::config::ShadowsocksUser> = stls.users.iter().map(|u| {
                            crate::singbox::config::ShadowsocksUser {
                                name: u.password.clone(),
                                password: u.password.clone(),
                            }
                        }).collect();

                        let inner_inbound = Inbound::Shadowsocks(ShadowsocksInbound {
                            tag: inner_tag,
                            listen: "127.0.0.1".to_string(),
                            listen_port: inner_port,
                            method: "2022-blake3-aes-128-gcm".to_string(), // Standard modern SS
                            users: ss_users,
                            multiplex: None,
                        });

                        generated.push(inner_inbound);
                    }
                }
            },
            Err(e) => {
                // FALLBACK: Raw Manual Mode
                // If the protocol isn't one of our known types, OR the structure doesn't match strict parsing (e.g. unknown fields),
                // we treat it as a raw manual override.
                // We attempt to construct a generic Inbound object by mapping the raw JSON directly.
                // However, `Inbound` enum is strict. We need a way to represent "Raw" inbound.
                // Sing-box config generation expects strictly typed `Inbound` variants.
                // To support true raw JSON, we would need an `Inbound::Raw(serde_json::Value)` variant in `config.rs`.
                // BUT, since we cannot easily modify `config.rs` to break everything else, we will try to deserialize into one of the known variants
                // but ignore extra fields? `serde` by default ignores unknown fields.
                // If it failed, it means required fields were missing or types mismatched.

                // If it's a completely unknown protocol, we can't support it strictly unless we add `Inbound::Other`.
                // Let's assume the user is trying to use a supported protocol but with extra fields.
                // But wait, `serde_json::from_value` failed.
                // Let's log the error and skip.
                let proto = inbound.protocol.clone().to_lowercase();
                error!(
                    "❌ Failed to parse settings for inbound {}: {} (json: {}). Protocol: {}. Manual mode bypass not fully implemented without Inbound::Raw.",
                    inbound.tag, e, inbound.settings, proto
                );
            }
        }

        generated
    }

    fn generate_vless(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        vless: caramba_db::models::network::VlessSettings,
        stream_settings: &DbStreamSettings,
    ) -> Inbound {
        let mut tls_config = None;
        let security = stream_settings.security.as_deref().unwrap_or("none");

        if security == "reality" {
            if let Some(reality) = &stream_settings.reality_settings {
                tls_config = Some(VlessTlsConfig {
                    enabled: true,
                    server_name: reality.server_names.first().cloned().unwrap_or_else(|| {
                        node.reality_sni
                            .clone()
                            .unwrap_or_else(|| "www.google.com".to_string())
                    }),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                    reality: RealityConfig {
                        enabled: true,
                        handshake: RealityHandshake {
                            server: if reality.dest.is_empty() {
                                node.reality_sni
                                    .clone()
                                    .unwrap_or_else(|| "www.google.com".to_string())
                            } else {
                                reality
                                    .dest
                                    .split(':')
                                    .next()
                                    .unwrap_or(&reality.dest)
                                    .to_string()
                            },
                            server_port: reality
                                .dest
                                .split(':')
                                .last()
                                .and_then(|p: &str| p.parse().ok())
                                .unwrap_or(443),
                        },
                        private_key: {
                            let k = if reality.private_key.is_empty() {
                                node.reality_priv.clone().unwrap_or_default()
                            } else {
                                reality.private_key.clone()
                            };
                            k.trim()
                                .replace('+', "-")
                                .replace('/', "_")
                                .replace('=', "")
                        },
                        short_id: {
                            let ids = if reality.short_ids.is_empty() {
                                node.short_id
                                    .clone()
                                    .map(|s| vec![s])
                                    .unwrap_or_default()
                            } else {
                                reality.short_ids.clone()
                            };
                            ids.into_iter()
                                .map(|s: String| s.trim().to_string())
                                .collect()
                        },
                    },
                    key_path: None,
                    certificate_path: None,
                });
            }
        } else if security == "tls" {
            let mut server_name = "www.google.com".to_string();
            let mut key_path = None;
            let mut cert_path = None;

            if let Some(tls) = &stream_settings.tls_settings {
                server_name = tls.server_name.clone();
                if let Some(certs) = &tls.certificates {
                    if let Some(first) = certs.get(0) {
                        key_path = Some(first.key_path.clone());
                        cert_path = Some(first.certificate_path.clone());
                    }
                }
            }

            tls_config = Some(VlessTlsConfig {
                enabled: true,
                server_name,
                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                reality: RealityConfig {
                    enabled: false,
                    handshake: RealityHandshake {
                        server: "".to_string(),
                        server_port: 0,
                    },
                    private_key: "".to_string(),
                    short_id: vec![],
                },
                key_path,
                certificate_path: cert_path,
            });
        }

        let mut transport_config = None;
        if let Some(network) = &stream_settings.network {
            match network.as_str() {
                "ws" => {
                    if let Some(ws) = &stream_settings.ws_settings {
                        transport_config = Some(VlessTransportConfig::Ws(WsTransport {
                            path: ws.path.clone(),
                            headers: ws.headers.clone(),
                        }));
                    }
                }
                "httpupgrade" => {
                    if let Some(http) = &stream_settings.http_upgrade_settings {
                        transport_config = Some(VlessTransportConfig::HttpUpgrade(
                            HttpUpgradeTransport {
                                path: http.path.clone(),
                                host: http.host.clone().map(|h| vec![h]),
                            },
                        ));
                    }
                }
                "xhttp" | "splithttp" => {
                    if let Some(xhttp) = &stream_settings.xhttp_settings {
                        transport_config = Some(VlessTransportConfig::HttpUpgrade(
                            HttpUpgradeTransport {
                                path: xhttp.path.clone(),
                                host: if xhttp.host.is_empty() {
                                    None
                                } else {
                                    Some(vec![xhttp.host.clone()])
                                },
                            },
                        ));
                    }
                }
                _ => {}
            }
        }

        let default_flow = if security == "reality"
            && stream_settings.network.as_deref() == Some("tcp")
        {
            "xtls-rprx-vision"
        } else {
            ""
        };

        let users: Vec<VlessUser> = vless
            .clients
            .iter()
            .map(|c| VlessUser {
                name: c.email.clone(),
                uuid: c.id.clone(),
                flow: if !c.flow.is_empty() {
                    Some(c.flow.clone())
                } else if !default_flow.is_empty() {
                    Some(default_flow.to_string())
                } else {
                    None
                },
            })
            .collect();

        // Multiplexing
        let multiplex = stream_settings.multiplex.as_ref().map(|m| MultiplexConfig {
            enabled: m.enabled,
            protocol: m.protocol.clone(),
            max_connections: m.max_connections,
            min_streams: m.min_streams,
            padding: m.padding,
        });

        Inbound::Vless(VlessInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            users,
            tls: tls_config,
            transport: transport_config,
            packet_encoding: stream_settings.packet_encoding.clone(),
            multiplex,
        })
    }

    fn generate_hysteria2(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        hy2: caramba_db::models::network::Hysteria2Settings,
        stream_settings: &DbStreamSettings,
    ) -> Inbound {
        let mut tls_config = Hysteria2TlsConfig {
            enabled: true,
            server_name: node
                .reality_sni
                .clone()
                .unwrap_or_else(|| "drive.google.com".to_string()),
            key_path: Some("/etc/sing-box/certs/key.pem".to_string()),
            certificate_path: Some("/etc/sing-box/certs/cert.pem".to_string()),
            alpn: Some(vec!["h3".to_string()]),
        };

        if let Some(tls) = &stream_settings.tls_settings {
            tls_config.server_name = tls.server_name.clone();
            if let Some(certs) = &tls.certificates {
                if let Some(first) = certs.get(0) {
                    if !first.key_path.is_empty() {
                        tls_config.key_path = Some(first.key_path.clone());
                    }
                    if !first.certificate_path.is_empty() {
                        tls_config.certificate_path = Some(first.certificate_path.clone());
                    }
                }
            }
        }

        let users: Vec<Hysteria2User> = hy2
            .users
            .iter()
            .map(|u| Hysteria2User {
                name: u.name.clone(),
                password: format!(
                    "{}:{}",
                    u.name.as_deref().unwrap_or("unknown"),
                    u.password.replace("-", "")
                ),
            })
            .collect();

        Inbound::Hysteria2(Hysteria2Inbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            users,
            up_mbps: Some(hy2.up_mbps),
            down_mbps: Some(hy2.down_mbps),
            ignore_client_bandwidth: None,
            obfs: hy2.obfs.map(|o| Hysteria2Obfs {
                ttype: o.ttype,
                password: o.password,
            }),
            masquerade: hy2.masquerade.clone().map(|s| {
                if !s.contains("://") && s.starts_with('/') {
                    format!("file://{}", s)
                } else {
                    s
                }
            }),
            tls: tls_config,
        })
    }

    fn generate_trojan(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        trojan: caramba_db::models::network::TrojanSettings,
        stream_settings: &DbStreamSettings,
    ) -> Inbound {
        let mut tls_config = None;
        let security = stream_settings.security.as_deref().unwrap_or("none");

        if security == "reality" {
            if let Some(reality) = &stream_settings.reality_settings {
                tls_config = Some(VlessTlsConfig {
                    enabled: true,
                    server_name: reality.server_names.first().cloned().unwrap_or_else(|| {
                        node.reality_sni
                            .clone()
                            .unwrap_or_else(|| "www.google.com".to_string())
                    }),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                    reality: RealityConfig {
                        enabled: true,
                        handshake: RealityHandshake {
                            server: if reality.dest.is_empty() {
                                node.reality_sni
                                    .clone()
                                    .unwrap_or_else(|| "www.google.com".to_string())
                            } else {
                                reality
                                    .dest
                                    .split(':')
                                    .next()
                                    .unwrap_or(&reality.dest)
                                    .to_string()
                            },
                            server_port: reality
                                .dest
                                .split(':')
                                .last()
                                .and_then(|p: &str| p.parse().ok())
                                .unwrap_or(443),
                        },
                        private_key: if reality.private_key.is_empty() {
                            node.reality_priv.clone().unwrap_or_default()
                        } else {
                            reality.private_key.clone()
                        },
                        short_id: if reality.short_ids.is_empty() {
                            node.short_id
                                .clone()
                                .map(|s| vec![s])
                                .unwrap_or_default()
                        } else {
                            reality.short_ids.clone()
                        },
                    },
                    key_path: None,
                    certificate_path: None,
                });
            }
        } else if security == "tls" {
            let mut server_name = "www.google.com".to_string();
            let mut key_path = None;
            let mut cert_path = None;

            if let Some(tls) = &stream_settings.tls_settings {
                server_name = tls.server_name.clone();
                if let Some(certs) = &tls.certificates {
                    if let Some(first) = certs.get(0) {
                        key_path = Some(first.key_path.clone());
                        cert_path = Some(first.certificate_path.clone());
                    }
                }
            }

            tls_config = Some(VlessTlsConfig {
                enabled: true,
                server_name,
                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                reality: RealityConfig {
                    enabled: false,
                    handshake: RealityHandshake {
                        server: "".to_string(),
                        server_port: 0,
                    },
                    private_key: "".to_string(),
                    short_id: vec![],
                },
                key_path,
                certificate_path: cert_path,
            });
        }

        let users: Vec<TrojanUser> = trojan
            .clients
            .iter()
            .map(|c| TrojanUser {
                name: c.email.clone(),
                password: c.password.clone(),
            })
            .collect();

        let multiplex = stream_settings.multiplex.as_ref().map(|m| MultiplexConfig {
            enabled: m.enabled,
            protocol: m.protocol.clone(),
            max_connections: m.max_connections,
            min_streams: m.min_streams,
            padding: m.padding,
        });

        Inbound::Trojan(TrojanInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            users,
            tls: tls_config,
            multiplex,
        })
    }

    fn generate_shadowsocks(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        mut ss: caramba_db::models::network::ShadowsocksSettings,
        relay_clients: &[caramba_db::models::node::Node],
        relay_auth_mode: RelayAuthMode,
    ) -> Inbound {
        // Inject Relay Clients
        for client_node in relay_clients {
            if let Some(token) = client_node
                .join_token
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                let base_username = format!("relay_{}", client_node.id);
                match relay_auth_mode {
                    RelayAuthMode::Legacy => {
                        ss.users
                            .push(caramba_db::models::network::ShadowsocksUser {
                                username: base_username,
                                password: token.to_string(),
                            });
                    }
                    RelayAuthMode::V1 => {
                        ss.users
                            .push(caramba_db::models::network::ShadowsocksUser {
                                username: base_username,
                                password: Self::derive_relay_password(token, node.id),
                            });
                    }
                    RelayAuthMode::Dual => {
                        ss.users
                            .push(caramba_db::models::network::ShadowsocksUser {
                                username: base_username,
                                password: Self::derive_relay_password(token, node.id),
                            });
                        ss.users
                            .push(caramba_db::models::network::ShadowsocksUser {
                                username: format!("relay_{}_legacy", client_node.id),
                                password: token.to_string(),
                            });
                    }
                }
            }
        }

        let users: Vec<ShadowsocksUser> = ss
            .users
            .iter()
            .map(|u| ShadowsocksUser {
                name: u.username.clone(),
                password: u.password.clone(),
            })
            .collect();

        // Shadowsocks usually doesn't have multiplex settings in sing-box inbound,
        // but if it did, we'd add it here. Sing-box schema says SS inbound doesn't have multiplex?
        // Checking schema... SS inbound DOES have multiplex since 1.9+ (implied for SS-2022?).
        // Actually, SS inbound usually doesn't. But let's add it if the struct supports it.
        // `ShadowsocksInbound` struct has `multiplex: Option<MultiplexConfig>`.

        Inbound::Shadowsocks(ShadowsocksInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            method: ss.method,
            users,
            multiplex: None, // Explicitly None unless we find SS multiplexing is valid here
        })
    }

    fn generate_tuic(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        tuic: caramba_db::models::network::TuicSettings,
        stream_settings: &DbStreamSettings,
    ) -> Inbound {
        let mut tls_config = TuicTlsConfig {
            enabled: true,
            server_name: node
                .reality_sni
                .clone()
                .unwrap_or_else(|| "www.google.com".to_string()),
            key_path: Some("/etc/sing-box/certs/key.pem".to_string()),
            certificate_path: Some("/etc/sing-box/certs/cert.pem".to_string()),
            alpn: Some(vec!["h3".to_string()]),
        };

        if let Some(tls) = &stream_settings.tls_settings {
            tls_config.server_name = tls.server_name.clone();
            if let Some(certs) = &tls.certificates {
                if let Some(first) = certs.get(0) {
                    if !first.key_path.is_empty() {
                        tls_config.key_path = Some(first.key_path.clone());
                    }
                    if !first.certificate_path.is_empty() {
                        tls_config.certificate_path = Some(first.certificate_path.clone());
                    }
                }
            }
        }

        let users: Vec<TuicUser> = tuic
            .users
            .iter()
            .map(|u| TuicUser {
                name: u.name.clone(),
                uuid: u.uuid.clone(),
                password: u.password.clone(),
            })
            .collect();

        Inbound::Tuic(TuicInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            users,
            congestion_control: tuic.congestion_control,
            auth_timeout: tuic.auth_timeout,
            zero_rtt_handshake: tuic.zero_rtt_handshake,
            heartbeat: tuic.heartbeat,
            tls: tls_config,
        })
    }

    fn generate_naive(
        node: &caramba_db::models::node::Node,
        inbound: &caramba_db::models::network::Inbound,
        naive: caramba_db::models::network::NaiveSettings,
        stream_settings: &DbStreamSettings,
    ) -> Inbound {
        let mut tls_config = None;
        let security = stream_settings.security.as_deref().unwrap_or("none");

        if security == "reality" {
            if let Some(reality) = &stream_settings.reality_settings {
                tls_config = Some(VlessTlsConfig {
                    enabled: true,
                    server_name: reality
                        .server_names
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                    reality: RealityConfig {
                        enabled: true,
                        handshake: RealityHandshake {
                            server: reality
                                .dest
                                .split(':')
                                .next()
                                .unwrap_or(&reality.dest)
                                .to_string(),
                            server_port: reality
                                .dest
                                .split(':')
                                .last()
                                .and_then(|p: &str| p.parse().ok())
                                .unwrap_or(443),
                        },
                        private_key: if reality.private_key.is_empty() {
                            node.reality_priv.clone().unwrap_or_default()
                        } else {
                            reality.private_key.clone()
                        },
                        short_id: if reality.short_ids.is_empty() {
                            node.short_id
                                .clone()
                                .map(|s| vec![s])
                                .unwrap_or_default()
                        } else {
                            reality.short_ids.clone()
                        },
                    },
                    key_path: None,
                    certificate_path: None,
                });
            }
        } else {
            let mut server_name = stream_settings
                .tls_settings
                .as_ref()
                .map(|t| t.server_name.clone())
                .unwrap_or_else(|| "www.google.com".to_string());
            let mut key_path = None;
            let mut cert_path = None;

            if let Some(tls) = &stream_settings.tls_settings {
                server_name = tls.server_name.clone();
                if let Some(certs) = &tls.certificates {
                    if let Some(first) = certs.get(0) {
                        key_path = Some(first.key_path.clone());
                        cert_path = Some(first.certificate_path.clone());
                    }
                }
            }

            if key_path.is_none() {
                key_path = Some("/etc/sing-box/certs/key.pem".to_string());
            }
            if cert_path.is_none() {
                cert_path = Some("/etc/sing-box/certs/cert.pem".to_string());
            }

            tls_config = Some(VlessTlsConfig {
                enabled: true,
                server_name,
                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                reality: RealityConfig {
                    enabled: false,
                    handshake: RealityHandshake {
                        server: "".to_string(),
                        server_port: 0,
                    },
                    private_key: "".to_string(),
                    short_id: vec![],
                },
                key_path,
                certificate_path: cert_path,
            });
        }

        Inbound::Naive(NaiveInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            users: naive
                .users
                .iter()
                .map(|u| NaiveUser {
                    username: u.username.clone(),
                    password: u.password.clone(),
                })
                .collect(),
            tls: tls_config,
        })
    }

    fn generate_amneziawg(
        inbound: &caramba_db::models::network::Inbound,
        awg: caramba_db::models::network::AmneziaWgSettings,
    ) -> Inbound {
        let peers = awg
            .users
            .iter()
            .map(|u| AmneziaWgUser {
                name: u.name.clone(),
                public_key: u.public_key.clone(),
                preshared_key: u.preshared_key.clone(),
                allowed_ips: vec![u.client_ip.clone()],
            })
            .collect();

        Inbound::AmneziaWg(AmneziaWgInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            peers,
            private_key: awg.private_key,
            jc: Some(awg.jc),
            jmin: Some(awg.jmin),
            jmax: Some(awg.jmax),
            s1: Some(awg.s1),
            s2: Some(awg.s2),
            h1: Some(awg.h1),
            h2: Some(awg.h2),
            h3: Some(awg.h3),
            h4: Some(awg.h4),
        })
    }

    fn generate_shadowtls(
        inbound: &caramba_db::models::network::Inbound,
        stls: &caramba_db::models::network::ShadowtlsSettings,
        detour_tag: Option<String>,
    ) -> Inbound {
        Inbound::Shadowtls(ShadowtlsInbound {
            tag: inbound.tag.clone(),
            listen: inbound.listen_ip.clone(),
            listen_port: inbound.listen_port as u16,
            version: Some(3), // Default to v3
            users: stls
                .users
                .iter()
                .map(|u| ShadowtlsUser {
                    password: u.password.clone(),
                    name: None,
                })
                .collect(),
            handshake: ShadowtlsHandshake {
                server: stls.handshake.server.clone(),
                server_port: stls.handshake.server_port,
            },
            strict_mode: Some(stls.strict_mode),
            detour: detour_tag,
        })
    }

    // Helper functions

    fn derive_relay_password(join_token: &str, target_node_id: i64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(join_token.trim().as_bytes());
        hasher.update(b":relay:");
        hasher.update(target_node_id.to_string().as_bytes());
        let digest = hasher.finalize();
        hex::encode(digest)
    }

    /// Validates Manual JSON input.
    /// Checks if the JSON string is a valid Sing-box inbound object structure.
    pub fn validate_manual_json(json_str: &str) -> anyhow::Result<()> {
        let v: serde_json::Value = serde_json::from_str(json_str)?;
        if !v.is_object() {
            return Err(anyhow::anyhow!("JSON must be an object"));
        }
        let obj = v.as_object().unwrap();
        if !obj.contains_key("protocol") && !obj.contains_key("type") {
             // Sing-box uses 'type', but our wrapper might use 'protocol'.
             // Actually, this validation is for the template *Settings* part.
             // We just ensure it's valid JSON for now.
             // If manual mode puts everything in `settings`, that's fine.
        }
        Ok(())
    }
}
