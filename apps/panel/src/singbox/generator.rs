use crate::singbox::config::*;
use crate::models::network::{StreamSettings as DbStreamSettings, InboundType};
use tracing::{error, warn};

pub struct ConfigGenerator;

impl ConfigGenerator {
    /// Generates a complete Sing-box configuration from a list of database Inbounds
    /// Generates a complete Sing-box configuration from a list of database Inbounds
    pub fn generate_config(
        node: &crate::models::node::Node,
        inbounds: Vec<crate::models::network::Inbound>,
    ) -> SingBoxConfig {
        
        let mut generated_inbounds = Vec::new();

        for inbound in inbounds {
            if !inbound.enable {
                continue;
            }

            // Parse Protocol Settings
            let protocol_settings: InboundType = match serde_json::from_str(&inbound.settings) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to parse settings for inbound {}: {}", inbound.tag, e);
                    continue;
                }
            };

            // Parse Stream Settings
            let stream_settings: DbStreamSettings = match serde_json::from_str(&inbound.stream_settings) {
                Ok(s) => s,
                Err(e) => {
                    error!("StreamSettings parse failed for inbound tag='{}', json='{}': {}", 
                        inbound.tag, inbound.stream_settings, e);
                    continue;
                }
            };

            // Map DB Inbound to Sing-box Inbound
            match protocol_settings {
                InboundType::Vless(vless) => {
                    let mut tls_config = None;
                    
                    let security = stream_settings.security.as_deref().unwrap_or("none");
                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                             tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: reality.server_names.first().cloned().unwrap_or_default(),
                                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                reality: RealityConfig {
                                    enabled: true,
                                    handshake: RealityHandshake {
                                        server: reality.dest.split(':').next().unwrap_or(&reality.dest).to_string(),
                                        server_port: reality.dest.split(':').last().and_then(|p| p.parse().ok()).unwrap_or(443),
                                    },
                                    private_key: reality.private_key,
                                    short_id: reality.short_ids,
                                }
                             });
                        }
                    } else if security == "tls" {
                         // Regular TLS implementation (placeholder)
                    }

                    // Convert users
                    let users = vless.clients.iter().map(|c| VlessUser {
                        name: c.email.clone(),
                        uuid: c.id.clone(),
                        flow: if c.flow.is_empty() { None } else { Some(c.flow.clone()) },
                    }).collect();

                    generated_inbounds.push(Inbound::Vless(VlessInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        tls: tls_config,
                    }));
                },
                InboundType::Hysteria2(hy2) => {
                    let mut tls_config = Hysteria2TlsConfig {
                        enabled: true,
                        server_name: "drive.google.com".to_string(), // Default or from stream
                        key_path: Some("/etc/sing-box/certs/key.pem".to_string()),
                        certificate_path: Some("/etc/sing-box/certs/cert.pem".to_string()),
                        alpn: Some(vec!["h3".to_string()]),
                    };

                    if let Some(tls) = stream_settings.tls_settings {
                         tls_config.server_name = tls.server_name;
                         if let Some(certs) = tls.certificates {
                             if let Some(first) = certs.first() {
                                 // Only overwrite if not empty
                                 if !first.key_path.is_empty() {
                                     tls_config.key_path = Some(first.key_path.clone());
                                 }
                                 if !first.certificate_path.is_empty() {
                                     tls_config.certificate_path = Some(first.certificate_path.clone());
                                 }
                             }
                         }
                    }

                    // FINAL SAFEGUARD: If still None, force defaults
                    if tls_config.key_path.is_none() {
                        tls_config.key_path = Some("/etc/sing-box/certs/key.pem".to_string());
                    }
                    if tls_config.certificate_path.is_none() {
                        tls_config.certificate_path = Some("/etc/sing-box/certs/cert.pem".to_string());
                    }

                    let users = hy2.users.iter().map(|u| Hysteria2User {
                        name: u.name.clone(),
                        // CRITICAL FIX: Sing-box Hysteria2 treats the entire auth payload as 'password'.
                        // Official clients send 'user:password'.
                        // So we must set the server-side password to match 'user:password'.
                        password: format!("{}:{}", u.name, u.password.replace("-", "")),
                    }).collect();

                    generated_inbounds.push(Inbound::Hysteria2(Hysteria2Inbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        // Configured bandwidth hints (integers in Mbps)
                        up_mbps: Some(hy2.up_mbps),
                        down_mbps: Some(hy2.down_mbps),
                        
                        // Conflict resolution: Docs say 'ignore_client_bandwidth' conflicts with up/down limits.
                        // So we only set it if limits are NOT set (or implicitly handling it).
                        // Since we are setting limits, we set this to None to let Serde skip it.
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
                    }));
                },
                InboundType::AmneziaWg(_awg) => {
                    warn!("AmneziaWG is currently disabled/deprecated. Skipping inbound {}", inbound.tag);
                    continue;
                },
                InboundType::Tuic(tuic) => {
                    let mut tls_config = TuicTlsConfig {
                        enabled: true,
                        server_name: "www.google.com".to_string(), // Default
                        key_path: Some("/etc/sing-box/certs/key.pem".to_string()),
                        certificate_path: Some("/etc/sing-box/certs/cert.pem".to_string()),
                        alpn: Some(vec!["h3".to_string()]),
                    };

                    if let Some(tls) = stream_settings.tls_settings {
                         tls_config.server_name = tls.server_name;
                         if let Some(certs) = tls.certificates {
                             if let Some(first) = certs.first() {
                                 if !first.key_path.is_empty() {
                                     tls_config.key_path = Some(first.key_path.clone());
                                 }
                                 if !first.certificate_path.is_empty() {
                                     tls_config.certificate_path = Some(first.certificate_path.clone());
                                 }
                             }
                         }
                    }

                    let users = tuic.users.iter().map(|u| TuicUser {
                        name: u.name.clone(),
                        uuid: u.uuid.clone(),
                        password: u.password.clone(),
                    }).collect();

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
                },
                InboundType::Trojan(trojan) => {
                    let mut tls_config = None;
                    
                    let security = stream_settings.security.as_deref().unwrap_or("none");
                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                             tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: reality.server_names.first().cloned().unwrap_or_default(),
                                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                reality: RealityConfig {
                                    enabled: true,
                                    handshake: RealityHandshake {
                                        server: reality.dest.split(':').next().unwrap_or(&reality.dest).to_string(),
                                        server_port: reality.dest.split(':').last().and_then(|p| p.parse().ok()).unwrap_or(443),
                                    },
                                    private_key: reality.private_key,
                                    short_id: reality.short_ids,
                                },
                             });
                        }
                    } else if security == "tls" {
                        if let Some(tls) = stream_settings.tls_settings {
                            tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: tls.server_name,
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
                            });
                        }
                    }

                    let users = trojan.clients.iter().map(|c| TrojanUser {
                        name: c.email.clone(),
                        password: c.password.clone(),
                    }).collect();

                    generated_inbounds.push(Inbound::Trojan(TrojanInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        tls: tls_config,
                    }));
                },

                _ => {
                    warn!("Unsupported protocol for inbound {}", inbound.tag);
                }
            }
        }


        SingBoxConfig {
            log: LogConfig {
                level: "info".to_string(),
                timestamp: true,
            },
            inbounds: generated_inbounds,
            outbounds: vec![
                Outbound::Direct { tag: "direct".to_string() }
            ],
            route: Some(RouteConfig {
                rules: {
                    let mut rules = Vec::new();
                    
                    // 1. Block BitTorrent
                    if node.config_block_torrent {
                        rules.push(RouteRule {
                            action: Some("reject".to_string()),
                            protocol: Some(vec!["bittorrent".to_string()]),
                            outbound: None, port: None, domain: None, geosite: None, geoip: None,
                        });
                    }

                    // 2. Block Ads
                    if node.config_block_ads {
                        rules.push(RouteRule {
                            action: Some("reject".to_string()),
                            geosite: Some(vec!["category-ads-all".to_string()]),
                            outbound: None, protocol: None, port: None, domain: None, geoip: None,
                        });
                    }

                    // 3. Block Porn
                    if node.config_block_porn {
                        rules.push(RouteRule {
                            action: Some("reject".to_string()),
                            geosite: Some(vec!["category-porn".to_string()]),
                            outbound: None, protocol: None, port: None, domain: None, geoip: None,
                        });
                    }

                    // 4. QoS (Prioritize UDP/QUIC)
                    if node.config_qos_enabled {
                        rules.push(RouteRule {
                            action: Some("route".to_string()),
                            protocol: Some(vec!["stun".to_string(), "quic".to_string(), "dtls".to_string()]),
                            outbound: Some("direct".to_string()),
                            port: None, domain: None, geosite: None, geoip: None,
                        });
                    }
                    
                    // Default Rule (Implicitly Direct due to outbounds order, but good to be explicit if needed)
                    // Sing-box defaults to first outbound if no match.
                    
                    rules
                }
            }),
            // Enable Clash API for device monitoring and limit enforcement
            experimental: Some(ExperimentalConfig {
                clash_api: ClashApiConfig {
                    external_controller: "0.0.0.0:9090".to_string(),
                    secret: None, // Internal access only, no auth needed
                    external_ui: None,
                },
            }),
        }
    }


}
