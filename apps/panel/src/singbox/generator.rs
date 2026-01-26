use crate::singbox::config::*;
use crate::models::network::{StreamSettings as DbStreamSettings, InboundType};
use tracing::{error, warn};

pub struct ConfigGenerator;

impl ConfigGenerator {
    /// Generates a complete Sing-box configuration from a list of database Inbounds
    pub fn generate_config(
        _node_ip: &str, // Used for logging or binding checks if needed
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
                    error!("Failed to parse stream settings for inbound {}: {}", inbound.tag, e);
                    continue;
                }
            };

            // Map DB Inbound to Sing-box Inbound
            match protocol_settings {
                InboundType::Vless(vless) => {
                    let mut tls_config = None;
                    
                    if stream_settings.security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                             tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: reality.server_names.first().cloned().unwrap_or_default(),
                                reality: RealityConfig {
                                    enabled: true,
                                    handshake: RealityHandshake {
                                        server: reality.dest.clone(),
                                        server_port: reality.dest.split(':').last().and_then(|p| p.parse().ok()).unwrap_or(443),
                                    },
                                    private_key: reality.private_key,
                                    short_id: reality.short_ids,
                                }
                             });
                        }
                    } else if stream_settings.security == "tls" {
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
                        server_name: "example.com".to_string(), // Default or from stream
                        key_path: None,
                        certificate_path: None,
                    };

                    if let Some(tls) = stream_settings.tls_settings {
                         tls_config.server_name = tls.server_name;
                         if let Some(certs) = tls.certificates {
                             if let Some(first) = certs.first() {
                                 tls_config.key_path = Some(first.key_file.clone());
                                 tls_config.certificate_path = Some(first.certificate_file.clone());
                             }
                         }
                    }

                    let users = hy2.users.iter().map(|u| Hysteria2User {
                        name: u.name.clone(),
                        password: u.password.clone(),
                    }).collect();

                    generated_inbounds.push(Inbound::Hysteria2(Hysteria2Inbound {
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
                level: "panic".to_string(),
                timestamp: true,
            },
            inbounds: generated_inbounds,
            outbounds: vec![
                Outbound::Direct { tag: "direct".to_string() },
                Outbound::Block { tag: "block".to_string() },
                Outbound::Dns { tag: "dns-out".to_string() },
            ],
            // Simplified routing for now
            route: Some(RouteConfig {
                rules: vec![
                     RouteRule {
                        action: Some("route".to_string()),
                        outbound: Some("direct".to_string()),
                        protocol: Some(vec!["dns".to_string()]),
                        port: None,
                        domain: None,
                    },
                ],
            }),
            // Enable Clash API for device monitoring and limit enforcement
            experimental: Some(ExperimentalConfig {
                clash_api: ClashApiConfig {
                    external_controller: "127.0.0.1:9090".to_string(),
                    secret: None, // Internal access only, no auth needed
                    external_ui: None,
                },
            }),
        }
    }
}
