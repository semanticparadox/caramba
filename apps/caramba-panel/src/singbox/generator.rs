use crate::singbox::config::*;
use crate::singbox::inbound_factory::RelayAuthMode;
use caramba_db::models::network::{Certificate, InboundType, StreamSettings as DbStreamSettings}; // Added Certificate
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
        for inbound in inbounds {
            if !inbound.enable {
                error!(
                    "🚫 Inbound {} is DISABLED, skipping generation",
                    inbound.tag
                );
                continue;
            }

            // Parse Protocol Settings
            let mut settings_value: serde_json::Value = serde_json::from_str(&inbound.settings)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

            if let Some(obj) = settings_value.as_object_mut() {
                if !obj.contains_key("protocol") {
                    obj.insert(
                        "protocol".to_string(),
                        serde_json::Value::String(inbound.protocol.clone().to_lowercase()),
                    );
                }
            }

            let protocol_settings: InboundType = match serde_json::from_value(
                settings_value.clone(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    let proto = inbound.protocol.clone().to_lowercase();
                    error!(
                        "❌ Failed to parse settings for inbound {}: {} (json: {}). Protocol: {}",
                        inbound.tag, e, inbound.settings, proto
                    );
                    continue;
                }
            };

            // Parse Stream Settings
            let stream_settings: DbStreamSettings =
                match serde_json::from_str(&inbound.stream_settings) {
                    Ok(s) => s,
                    Err(_) => {
                        warn!(
                            "⚠️ StreamSettings parse failed for inbound '{}', using defaults",
                            inbound.tag
                        );
                        DbStreamSettings::default()
                    }
                };

            // Map DB Inbound to Sing-box Inbound
            match protocol_settings {
                InboundType::Vless(vless) => {
                    // Inject Relay Clients as Users if this is a suitable inbound
                    // For now, we only inject into Shadowsocks for simplicity, but VLESS is possible too.
                    // Let's stick to Shadowsocks for inter-node transport unless VLESS is required.

                    let mut tls_config = None;

                    let security = stream_settings.security.as_deref().unwrap_or("none");
                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                            let reality_private_key = {
                                let k = if reality.private_key.is_empty() {
                                    node.reality_priv.clone().unwrap_or_default()
                                } else {
                                    reality.private_key.clone()
                                };
                                k.trim()
                                    .replace('+', "-")
                                    .replace('/', "_")
                                    .replace('=', "")
                            };

                            // Валидируем ключ до создания конфига — пустой ключ вызывает FATAL в sing-box
                            let pkey_invalid = reality_private_key.is_empty()
                                || reality_private_key.len() < 43
                                || reality_private_key.contains(' ');
                            if pkey_invalid {
                                warn!(
                                    "⚠️ Skipping Reality block for inbound '{}' due to INVALID OR MISSING PRIVATE KEY (len: {})",
                                    inbound.tag,
                                    reality_private_key.len()
                                );
                            } else {
                                tls_config = Some(VlessTlsConfig {
                                    enabled: true,
                                    server_name: reality.server_names.first().cloned().unwrap_or_else(
                                        || {
                                            node.reality_sni
                                                .clone()
                                                .unwrap_or_else(|| "www.google.com".to_string())
                                        },
                                    ),
                                    alpn: None, // Set later based on transport type
                                    // reality = Some только когда enabled: true и ключ валиден
                                    reality: Some(RealityConfig {
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
                                        private_key: reality_private_key,
                                        short_id: {
                                            let ids = if reality.short_ids.is_empty() {
                                                node.short_id
                                                    .clone()
                                                    .map(|s| vec![s])
                                                    .unwrap_or_default()
                                            } else {
                                                reality.short_ids
                                            };
                                            ids.into_iter()
                                                .map(|s: String| s.trim().to_string())
                                                .collect()
                                        },
                                    }),
                                    key_path: None,
                                    certificate_path: None,
                                });
                            }
                        }
                    } else if security == "tls" {
                        // Для TLS-инбаундов используем домен ноды (если задан),
                        // иначе IP — серт генерируется агентом с этим CN/SAN.
                        let mut server_name = node
                            .domain
                            .clone()
                            .unwrap_or_else(|| node.ip.clone());
                        let mut key_path = None;
                        let mut cert_path = None;

                        if let Some(tls) = &stream_settings.tls_settings {
                            // Используем server_name из настроек только если он непустой
                            if !tls.server_name.is_empty() {
                                server_name = tls.server_name.clone();
                            }
                            if let Some(certs) = &tls.certificates {
                                let certs: &Vec<Certificate> = certs;
                                if let Some(first) = certs.get(0) {
                                    if !first.key_path.is_empty() {
                                        key_path = Some(first.key_path.clone());
                                    }
                                    if !first.certificate_path.is_empty() {
                                        cert_path = Some(first.certificate_path.clone());
                                    }
                                }
                            }
                        }

                        // Самоподписанный серт генерируется агентом на ноде при старте.
                        // Клиенты подключаются с insecure: true для этих транспортов.
                        if key_path.is_none() {
                            key_path = Some("/etc/sing-box/certs/key.pem".to_string());
                        }
                        if cert_path.is_none() {
                            cert_path = Some("/etc/sing-box/certs/cert.pem".to_string());
                        }

                        tls_config = Some(VlessTlsConfig {
                            enabled: true,
                            server_name,
                            alpn: None, // Set later based on transport type
                            // reality = None для обычного TLS — sing-box не должен видеть этот блок
                            reality: None,
                            key_path,
                            certificate_path: cert_path,
                        });
                    }

                    // Transport Settings
                    let mut transport_config = None;
                    if let Some(network) = &stream_settings.network {
                        let network: &String = network;
                        match network.as_str() {
                            "ws" => {
                                if let Some(ws) = stream_settings
                                    .ws_settings
                                    .as_ref()
                                    .or(stream_settings.ws_settings.as_ref())
                                {
                                    transport_config =
                                        Some(VlessTransportConfig::Ws(WsTransport {
                                            path: ws.path.clone(),
                                            headers: ws.headers.clone(),
                                        }));
                                }
                            }
                            "httpupgrade" => {
                                if let Some(http) = stream_settings.http_upgrade_settings.as_ref() {
                                    transport_config = Some(VlessTransportConfig::HttpUpgrade(
                                        HttpUpgradeTransport {
                                            path: http.path.clone(),
                                            // host — строка, не массив (sing-box spec HTTPUpgrade inbound)
                                            host: http.host.clone(),
                                        },
                                    ));
                                }
                            }
                            "grpc" => {
                                // gRPC transport — read service_name from raw stream_settings JSON
                                let raw: serde_json::Value = serde_json::from_str(&inbound.stream_settings).unwrap_or_default();
                                let grpc = raw.get("grpcSettings").or_else(|| raw.get("grpc_settings"));
                                let service_name = grpc
                                    .and_then(|g| g.get("serviceName").or_else(|| g.get("service_name")))
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("grpc")
                                    .to_string();
                                transport_config = Some(VlessTransportConfig::Grpc(
                                    GrpcTransport { service_name },
                                ));
                            }
                            "xhttp" | "splithttp" => {
                                if let Some(xhttp) = stream_settings.xhttp_settings.as_ref() {
                                    transport_config = Some(VlessTransportConfig::HttpUpgrade(
                                        HttpUpgradeTransport {
                                            path: xhttp.path.clone(),
                                            // host — строка, не массив
                                            host: if xhttp.host.is_empty() {
                                                None
                                            } else {
                                                Some(xhttp.host.clone())
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

                    if users.is_empty() {
                        warn!(
                            "⚠️ VLESS inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound.tag
                        );
                        continue;
                    }

                    // ALPN intentionally NOT set on server inbounds.
                    // sing-box docs: "connection will fail if no mutually supported protocol".
                    // Without ALPN from server, there's no negotiation conflict with any transport.
                    // tls_config.alpn remains None → omitted from JSON via skip_serializing_if.

                    generated_inbounds.push(Inbound::Vless(VlessInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        tls: tls_config,
                        transport: transport_config,
                        packet_encoding: stream_settings.packet_encoding.clone(),
                        multiplex: None,
                    }));
                }
                InboundType::Hysteria2(hy2) => {
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

                    if let Some(tls) = stream_settings.tls_settings {
                        // Don't override server_name — orchestration_service already
                        // enforced node.reality_sni on all TLS configs
                        if let Some(certs) = tls.certificates {
                            let certs: Vec<caramba_db::models::network::Certificate> = certs;
                            if let Some(first) = certs.get(0) {
                                if !first.key_path.is_empty() {
                                    tls_config.key_path = Some(first.key_path.clone());
                                }
                                if !first.certificate_path.is_empty() {
                                    tls_config.certificate_path =
                                        Some(first.certificate_path.clone());
                                }
                            }
                        }
                    }

                    if tls_config.key_path.is_none() {
                        tls_config.key_path = Some("/etc/sing-box/certs/key.pem".to_string());
                    }
                    if tls_config.certificate_path.is_none() {
                        tls_config.certificate_path =
                            Some("/etc/sing-box/certs/cert.pem".to_string());
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

                    if users.is_empty() {
                        warn!(
                            "⚠️ Hysteria2 inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound.tag
                        );
                        continue;
                    }

                    generated_inbounds.push(Inbound::Hysteria2(Hysteria2Inbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        up_mbps: Some(hy2.up_mbps),
                        down_mbps: Some(hy2.down_mbps),
                        ignore_client_bandwidth: None,
                        obfs: hy2.obfs.map(|o| Hysteria2Obfs {
                            ttype: o.ttype,
                            password: o.password,
                        }),
                        masquerade: hy2.masquerade.clone().map(|s: String| {
                            if !s.contains("://") && s.starts_with('/') {
                                format!("file://{}", s)
                            } else {
                                s
                            }
                        }),
                        tls: tls_config,
                    }));
                }
                InboundType::AmneziaWg(awg) => {
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

                    generated_inbounds.push(Inbound::AmneziaWg(AmneziaWgInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
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
                    }));
                }
                InboundType::Tuic(tuic) => {
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

                    if let Some(tls) = stream_settings.tls_settings {
                        tls_config.server_name = tls.server_name;
                        if let Some(certs) = tls.certificates {
                            let certs: Vec<caramba_db::models::network::Certificate> = certs;
                            if let Some(first) = certs.get(0) {
                                if !first.key_path.is_empty() {
                                    tls_config.key_path = Some(first.key_path.clone());
                                }
                                if !first.certificate_path.is_empty() {
                                    tls_config.certificate_path =
                                        Some(first.certificate_path.clone());
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

                    if users.is_empty() {
                        warn!(
                            "⚠️ TUIC inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound.tag
                        );
                        continue;
                    }

                    generated_inbounds.push(Inbound::Tuic(TuicInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        congestion_control: tuic.congestion_control,
                        auth_timeout: tuic.auth_timeout,
                        zero_rtt_handshake: tuic.zero_rtt_handshake,
                        heartbeat: tuic.heartbeat,
                        tls: tls_config,
                    }));
                }
                InboundType::Trojan(trojan) => {
                    let mut tls_config = None;

                    let security = stream_settings.security.as_deref().unwrap_or("none");
                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                            tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: reality.server_names.first().cloned().unwrap_or_else(
                                    || {
                                        node.reality_sni
                                            .clone()
                                            .unwrap_or_else(|| "www.google.com".to_string())
                                    },
                                ),
                                alpn: None, // Intentionally omitted — avoids ALPN negotiation conflicts
                                reality: Some(RealityConfig {
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
                                        reality.private_key
                                    },
                                    short_id: if reality.short_ids.is_empty() {
                                        node.short_id.clone().map(|s| vec![s]).unwrap_or_default()
                                    } else {
                                        reality.short_ids
                                    },
                                }),
                                key_path: None,
                                certificate_path: None,
                            });
                        }
                    } else if security == "tls" {
                        // Для TLS-инбаундов используем домен ноды (если задан),
                        // иначе IP — серт генерируется агентом с этим CN/SAN.
                        let mut server_name = node
                            .domain
                            .clone()
                            .unwrap_or_else(|| node.ip.clone());
                        let mut key_path = None;
                        let mut cert_path = None;

                        if let Some(tls) = &stream_settings.tls_settings {
                            if !tls.server_name.is_empty() {
                                server_name = tls.server_name.clone();
                            }
                            if let Some(certs) = &tls.certificates {
                                let certs: &Vec<caramba_db::models::network::Certificate> = certs;
                                if let Some(first) = certs.get(0) {
                                    if !first.key_path.is_empty() {
                                        key_path = Some(first.key_path.clone());
                                    }
                                    if !first.certificate_path.is_empty() {
                                        cert_path = Some(first.certificate_path.clone());
                                    }
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
                            alpn: None, // Intentionally omitted — avoids ALPN negotiation conflicts
                            reality: None,
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

                    if users.is_empty() {
                        warn!(
                            "⚠️ Trojan inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound.tag
                        );
                        continue;
                    }

                    generated_inbounds.push(Inbound::Trojan(TrojanInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        tls: tls_config,
                        multiplex: None,
                    }));
                }
                InboundType::Naive(naive) => {
                    let mut tls_config = None;
                    let security = stream_settings.security.as_deref().unwrap_or("none");

                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                            tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: reality
                                    .server_names
                                    .first()
                                    .cloned()
                                    .unwrap_or_default(),
                                alpn: None, // Intentionally omitted — avoids ALPN negotiation conflicts
                                reality: Some(RealityConfig {
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
                                        reality.private_key
                                    },
                                    short_id: if reality.short_ids.is_empty() {
                                        node.short_id.clone().map(|s| vec![s]).unwrap_or_default()
                                    } else {
                                        reality.short_ids
                                    },
                                }),
                                key_path: None,
                                certificate_path: None,
                            });
                        }
                    } else {
                        // Для TLS-инбаундов используем домен ноды (если задан),
                        // иначе IP — серт генерируется агентом с этим CN/SAN.
                        let mut server_name = node
                            .domain
                            .clone()
                            .unwrap_or_else(|| node.ip.clone());
                        let mut key_path = None;
                        let mut cert_path = None;

                        if let Some(tls) = &stream_settings.tls_settings {
                            if !tls.server_name.is_empty() {
                                server_name = tls.server_name.clone();
                            }
                            if let Some(certs) = &tls.certificates {
                                let certs: &Vec<caramba_db::models::network::Certificate> = certs;
                                if let Some(first) = certs.get(0) {
                                    if !first.key_path.is_empty() {
                                        key_path = Some(first.key_path.clone());
                                    }
                                    if !first.certificate_path.is_empty() {
                                        cert_path = Some(first.certificate_path.clone());
                                    }
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
                            alpn: None, // Intentionally omitted — avoids ALPN negotiation conflicts
                            reality: None,
                            key_path,
                            certificate_path: cert_path,
                        });
                    }

                    let inbound_obj = NaiveInbound {
                        tag: inbound.tag,
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
                    };

                    if inbound_obj.users.is_empty() {
                        warn!(
                            "⚠️ Naive inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound_obj.tag
                        );
                        continue;
                    }

                    generated_inbounds.push(Inbound::Naive(inbound_obj));
                }
                InboundType::Shadowsocks(mut ss) => {
                    // Inject Relay Clients if this is a suitable Shadowsocks inbound
                    for client_node in &relay_clients {
                        if let Some(token) = client_node
                            .join_token
                            .as_deref()
                            .map(str::trim)
                            .filter(|t| !t.is_empty())
                        {
                            warn!(
                                "🔗 Injecting Relay Access for Node {} ({}). User: relay_{}",
                                client_node.name, client_node.ip, client_node.id
                            );
                            let base_username = format!("relay_{}", client_node.id);
                            match relay_auth_mode {
                                RelayAuthMode::Legacy => {
                                    ss.users.push(caramba_db::models::network::ShadowsocksUser {
                                        username: base_username,
                                        password: token.to_string(),
                                    });
                                }
                                RelayAuthMode::V1 => {
                                    ss.users.push(caramba_db::models::network::ShadowsocksUser {
                                        username: base_username,
                                        password: derive_relay_password(token, node.id),
                                    });
                                }
                                RelayAuthMode::Dual => {
                                    ss.users.push(caramba_db::models::network::ShadowsocksUser {
                                        username: base_username,
                                        password: derive_relay_password(token, node.id),
                                    });
                                    ss.users.push(caramba_db::models::network::ShadowsocksUser {
                                        username: format!("relay_{}_legacy", client_node.id),
                                        password: token.to_string(),
                                    });
                                }
                            }
                        } else {
                            warn!(
                                "⚠️ Relay client node {} has no join_token. Skipping injected relay user.",
                                client_node.id
                            );
                        }
                    }

                    let users: Vec<crate::singbox::config::ShadowsocksUser> = ss
                        .users
                        .iter()
                        .map(|u| crate::singbox::config::ShadowsocksUser {
                            name: u.username.clone(),
                            password: u.password.clone(),
                        })
                        .collect();

                    if users.is_empty() {
                        warn!(
                            "⚠️ Shadowsocks inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound.tag
                        );
                        continue;
                    }

                    generated_inbounds.push(Inbound::Shadowsocks(ShadowsocksInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        method: ss.method,
                        users,
                        multiplex: None,
                    }));
                }
                InboundType::Shadowtls(stls) => {
                    let users: Vec<ShadowtlsUser> = stls
                        .users
                        .iter()
                        .map(|u| ShadowtlsUser {
                            password: u.password.clone(),
                            name: None,
                        })
                        .collect();

                    if users.is_empty() {
                        warn!(
                            "⚠️ ShadowTLS inbound '{}' has no users, skipping to avoid sing-box FATAL",
                            inbound.tag
                        );
                        continue;
                    }

                    generated_inbounds.push(Inbound::Shadowtls(ShadowtlsInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        version: Some(3),
                        users,
                        handshake: ShadowtlsHandshake {
                            server: stls.handshake.server,
                            server_port: stls.handshake.server_port,
                        },
                        strict_mode: Some(stls.strict_mode),
                        detour: None,
                    }));
                }
            }
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
