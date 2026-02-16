use crate::singbox::config::*;
use crate::models::network::{StreamSettings as DbStreamSettings, InboundType};
use tracing::{error, warn};

pub struct ConfigGenerator;

impl ConfigGenerator {
    /// Generates a complete Sing-box configuration from a list of database Inbounds
    pub fn generate_config(
        node: &crate::models::node::Node,
        inbounds: Vec<crate::models::network::Inbound>,
        target_node: Option<crate::models::node::Node>,
        relay_clients: Vec<crate::models::node::Node>,
    ) -> SingBoxConfig {
        
        let mut generated_inbounds = Vec::new();

        // 1. Process Inbounds (Normal + Relay Injection)
        for inbound in inbounds {
            if !inbound.enable {
                error!("🚫 Inbound {} is DISABLED, skipping generation", inbound.tag);
                continue;
            }

            // Parse Protocol Settings
            let mut settings_value: serde_json::Value = serde_json::from_str(&inbound.settings).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            
            if let Some(obj) = settings_value.as_object_mut() {
                if !obj.contains_key("protocol") {
                    obj.insert("protocol".to_string(), serde_json::Value::String(inbound.protocol.clone().to_lowercase()));
                }
            }

            let protocol_settings: InboundType = match serde_json::from_value(settings_value.clone()) {
                Ok(s) => s,
                Err(e) => {
                    let proto = inbound.protocol.clone().to_lowercase();
                    error!("❌ Failed to parse settings for inbound {}: {} (json: {}). Protocol: {}", inbound.tag, e, inbound.settings, proto);
                    continue;
                }
            };

            // Parse Stream Settings
            let stream_settings: DbStreamSettings = match serde_json::from_str(&inbound.stream_settings) {
                Ok(s) => s,
                Err(_) => {
                    warn!("⚠️ StreamSettings parse failed for inbound '{}', using defaults", inbound.tag);
                    DbStreamSettings::default()
                }
            };

            // Map DB Inbound to Sing-box Inbound
            match protocol_settings {
                InboundType::Vless(mut vless) => {
                    // Inject Relay Clients as Users if this is a suitable inbound
                    // For now, we only inject into Shadowsocks for simplicity, but VLESS is possible too.
                    // Let's stick to Shadowsocks for inter-node transport unless VLESS is required.
                    
                    let mut tls_config = None;
                    
                    let security = stream_settings.security.as_deref().unwrap_or("none");
                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                             tls_config = Some(VlessTlsConfig {
                                enabled: true,
                                server_name: reality.server_names.first().cloned().unwrap_or_else(|| {
                                    node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string())
                                }),
                                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                reality: RealityConfig {
                                    enabled: true,
                                    handshake: RealityHandshake {
                                        server: if reality.dest.is_empty() {
                                            node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string())
                                        } else {
                                            reality.dest.split(':').next().unwrap_or(&reality.dest).to_string()
                                        },
                                        server_port: reality.dest.split(':').last().and_then(|p| p.parse().ok()).unwrap_or(443),
                                    },
                                    private_key: {
                                        let k = if reality.private_key.is_empty() { 
                                            node.reality_priv.clone().unwrap_or_default() 
                                        } else { 
                                            reality.private_key 
                                        };
                                        k.trim().replace('+', "-").replace('/', "_").replace('=', "")
                                    },
                                    short_id: {
                                        let ids = if reality.short_ids.is_empty() { 
                                            node.short_id.clone().map(|s| vec![s]).unwrap_or_default() 
                                        } else { 
                                            reality.short_ids 
                                        };
                                        ids.into_iter().map(|s| s.trim().to_string()).collect()
                                    },
                                },
                                key_path: None,
                                certificate_path: None,
                             });

                             if let Some(ref cfg) = tls_config {
                                 let pkey = &cfg.reality.private_key;
                                 let is_invalid = pkey.is_empty() || pkey.len() < 43 || pkey.contains(' ');
                                 if cfg.reality.enabled && is_invalid {
                                     warn!("⚠️ Skipping Reality block for inbound '{}' due to INVALID OR MISSING PRIVATE KEY (len: {})", inbound.tag, pkey.len());
                                     tls_config = None;
                                 }
                             }
                        }
                    } else if security == "tls" {
                         let mut server_name = "www.google.com".to_string();
                         let mut key_path = None;
                         let mut cert_path = None;

                         if let Some(tls) = &stream_settings.tls_settings {
                             server_name = tls.server_name.clone();
                             if let Some(certs) = &tls.certificates {
                                 if let Some(first) = certs.first() {
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
                                 handshake: RealityHandshake { server: "".to_string(), server_port: 0 },
                                 private_key: "".to_string(),
                                 short_id: vec![],
                             },
                             key_path,
                             certificate_path: cert_path,
                         });
                    }

                    // Transport Settings
                    let mut transport_config = None;
                    if let Some(network) = &stream_settings.network {
                        match network.as_str() {
                            "ws" => {
                                if let Some(ws) = stream_settings.ws_settings.as_ref()
                                    .or(stream_settings.ws_settings.as_ref()) {
                                    transport_config = Some(VlessTransportConfig::Ws(WsTransport {
                                        path: ws.path.clone(),
                                        headers: ws.headers.clone(),
                                    }));
                                }
                            },
                            "httpupgrade" => {
                                if let Some(http) = stream_settings.http_upgrade_settings.as_ref() {
                                    transport_config = Some(VlessTransportConfig::HttpUpgrade(HttpUpgradeTransport {
                                        path: http.path.clone(),
                                        host: http.host.clone().map(|h| vec![h]),
                                    }));
                                }
                            },
                            "xhttp" | "splithttp" => {
                                if let Some(xhttp) = stream_settings.xhttp_settings.as_ref() {
                                    transport_config = Some(VlessTransportConfig::HttpUpgrade(HttpUpgradeTransport {
                                        path: xhttp.path.clone(),
                                        host: if xhttp.host.is_empty() { None } else { Some(vec![xhttp.host.clone()]) },
                                    }));
                                }
                            },
                            _ => {}
                        }
                    }

                    let default_flow = if security == "reality" && stream_settings.network.as_deref() == Some("tcp") {
                        "xtls-rprx-vision"
                    } else {
                        ""
                    };

                    let users: Vec<VlessUser> = vless.clients.iter().map(|c| VlessUser {
                        name: c.email.clone(),
                        uuid: c.id.clone(),
                        flow: if !c.flow.is_empty() { 
                            Some(c.flow.clone()) 
                        } else if !default_flow.is_empty() {
                            Some(default_flow.to_string())
                        } else {
                            None
                        },
                    }).collect();

                    if users.is_empty() {
                        warn!("⚠️ VLESS inbound '{}' has no users, skipping to avoid sing-box FATAL", inbound.tag);
                        continue;
                    }

                    generated_inbounds.push(Inbound::Vless(VlessInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        tls: tls_config,
                        transport: transport_config,
                        packet_encoding: stream_settings.packet_encoding.clone(),
                    }));
                },
                InboundType::Hysteria2(hy2) => {
                    let mut tls_config = Hysteria2TlsConfig {
                        enabled: true,
                        server_name: node.reality_sni.clone().unwrap_or_else(|| "drive.google.com".to_string()),
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

                    if tls_config.key_path.is_none() {
                        tls_config.key_path = Some("/etc/sing-box/certs/key.pem".to_string());
                    }
                    if tls_config.certificate_path.is_none() {
                        tls_config.certificate_path = Some("/etc/sing-box/certs/cert.pem".to_string());
                    }

                    let users: Vec<Hysteria2User> = hy2.users.iter().map(|u| Hysteria2User {
                        name: u.name.clone(),
                        password: format!("{}:{}", u.name.as_deref().unwrap_or("unknown"), u.password.replace("-", "")),
                    }).collect();

                    if users.is_empty() {
                        warn!("⚠️ Hysteria2 inbound '{}' has no users, skipping to avoid sing-box FATAL", inbound.tag);
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
                InboundType::AmneziaWg(awg) => {
                    let peers = awg.users.iter().map(|u| AmneziaWgUser {
                        name: u.name.clone(),
                        public_key: u.public_key.clone(),
                        preshared_key: u.preshared_key.clone(),
                        allowed_ips: vec![u.client_ip.clone()],
                    }).collect();

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
                },
                InboundType::Tuic(tuic) => {
                    let mut tls_config = TuicTlsConfig {
                        enabled: true,
                        server_name: node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string()),
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

                    let users: Vec<TuicUser> = tuic.users.iter().map(|u| TuicUser {
                        name: u.name.clone(),
                        uuid: u.uuid.clone(),
                        password: u.password.clone(),
                    }).collect();

                    if users.is_empty() {
                        warn!("⚠️ TUIC inbound '{}' has no users, skipping to avoid sing-box FATAL", inbound.tag);
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
                },
                InboundType::Trojan(trojan) => {
                    let mut tls_config = None;
                    
                    let security = stream_settings.security.as_deref().unwrap_or("none");
                    if security == "reality" {
                        if let Some(reality) = stream_settings.reality_settings {
                              tls_config = Some(VlessTlsConfig {
                                 enabled: true,
                                 server_name: reality.server_names.first().cloned().unwrap_or_else(|| {
                                     node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string())
                                 }),
                                 alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                 reality: RealityConfig {
                                     enabled: true,
                                     handshake: RealityHandshake {
                                         server: if reality.dest.is_empty() {
                                             node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string())
                                         } else {
                                             reality.dest.split(':').next().unwrap_or(&reality.dest).to_string()
                                         },
                                         server_port: reality.dest.split(':').last().and_then(|p| p.parse().ok()).unwrap_or(443),
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
                                if let Some(first) = certs.first() {
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

                    let users: Vec<TrojanUser> = trojan.clients.iter().map(|c| TrojanUser {
                        name: c.email.clone(),
                        password: c.password.clone(),
                    }).collect();

                    if users.is_empty() {
                        warn!("⚠️ Trojan inbound '{}' has no users, skipping to avoid sing-box FATAL", inbound.tag);
                        continue;
                    }

                    generated_inbounds.push(Inbound::Trojan(TrojanInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        users,
                        tls: tls_config,
                    }));
                },
                InboundType::Naive(naive) => {
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
                                    private_key: if reality.private_key.is_empty() { node.reality_priv.clone().unwrap_or_default() } else { reality.private_key },
                                    short_id: if reality.short_ids.is_empty() { node.short_id.clone().map(|s| vec![s]).unwrap_or_default() } else { reality.short_ids },
                                },
                                key_path: None,
                                certificate_path: None,
                            });
                        }
                    } else {
                        let mut server_name = stream_settings.tls_settings.as_ref().map(|t| t.server_name.clone()).unwrap_or_else(|| "www.google.com".to_string());
                        let mut key_path = None;
                        let mut cert_path = None;

                        if let Some(tls) = &stream_settings.tls_settings {
                             server_name = tls.server_name.clone();
                             if let Some(certs) = &tls.certificates {
                                 if let Some(first) = certs.first() {
                                     key_path = Some(first.key_path.clone());
                                     cert_path = Some(first.certificate_path.clone());
                                 }
                             }
                        }

                        if key_path.is_none() { key_path = Some("/etc/sing-box/certs/key.pem".to_string()); }
                        if cert_path.is_none() { cert_path = Some("/etc/sing-box/certs/cert.pem".to_string()); }

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

                    let inbound_obj = NaiveInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip.clone(),
                        listen_port: inbound.listen_port as u16,
                        users: naive.users.iter().map(|u| NaiveUser {
                            username: u.username.clone(),
                            password: u.password.clone(),
                        }).collect(),
                        tls: tls_config,
                    };
                    
                    if inbound_obj.users.is_empty() {
                        warn!("⚠️ Naive inbound '{}' has no users, skipping to avoid sing-box FATAL", inbound_obj.tag);
                        continue;
                    }

                    generated_inbounds.push(Inbound::Naive(inbound_obj));
                },
                InboundType::Shadowsocks(mut ss) => {
                    // Inject Relay Clients if this is a suitable Shadowsocks inbound
                    for client_node in &relay_clients {
                        if let Some(token) = &client_node.join_token {
                            warn!("🔗 Injecting Relay Access for Node {} ({}). User: relay_{}", client_node.name, client_node.ip, client_node.id);
                            ss.users.push(crate::models::network::ShadowsocksUser {
                                username: Some(format!("relay_{}", client_node.id)), // Keep it traceable
                                password: token.clone(), // Use the client node's token as password
                            });
                        }
                    }

                    let users: Vec<crate::singbox::config::ShadowsocksUser> = ss.users.iter().map(|u| crate::singbox::config::ShadowsocksUser {
                        name: u.username.clone(),
                        password: u.password.clone(),
                    }).collect();

                    if users.is_empty() {
                        warn!("⚠️ Shadowsocks inbound '{}' has no users, skipping to avoid sing-box FATAL", inbound.tag);
                        continue;
                    }

                    generated_inbounds.push(Inbound::Shadowsocks(ShadowsocksInbound {
                        tag: inbound.tag,
                        listen: inbound.listen_ip,
                        listen_port: inbound.listen_port as u16,
                        method: ss.method,
                        users,
                    }));
                },
            }
        }

        // 2. Generate Outbounds (Standard + Relay)
        let mut outbounds = vec![
            Outbound::Direct { tag: "direct".to_string() },
        ];

        // 3. Relay Logic: Add Relay Outbound if enabled
        let mut default_outbound_tag = "direct".to_string();

        if let Some(target) = target_node {
            if node.is_relay {
                warn!("🔗 Configuring Node as RELAY -> Target: {} ({})", target.name, target.ip);
                
                // We use Shadowsocks for inter-node transport
                // IMPORTANT: The target node MUST have a Shadowsocks inbound listening
                // For now, we assume standard VPN port or 443 with Shadowsocks
                
                // We actually need to know WHICH inbound on the target node to connect to.
                // Assuming standard Shadowsocks inbound on target.vpn_port for now.
                // Or maybe VLESS?
                // Let's use the target's VPN port with Shadowsocks 2022 if possible, or wait, we just use Shadowsocks.
                
                // IMPROVEMENT: Protocol Selection? For now, hardcode Shadowsocks.
                outbounds.push(Outbound::Shadowsocks(ShadowsocksOutbound {
                    tag: "relay-out".to_string(),
                    server: target.ip.clone(),
                    server_port: target.vpn_port as u16,
                    method: "chacha20-ietf-poly1305".to_string(), // Safer default for variable length passwords (tokens)
                    // ISSUE: We don't know the exact method/password of the target's inbound here easily WITHOUT fetching its inbounds.
                    // BUT! We injected the user `relay_<id>` with password `token` into the target's SS inbound above.
                    // effectively we need to know the METHOD of the target's SS inbound.
                    // Fallback: This requires the TARGET node to have a matching inbound.
                    password: node.join_token.clone().unwrap_or_default(), // We authenticate using OUR token
                }));
                
                // Override default route to Relay
                default_outbound_tag = "relay-out".to_string();
            }
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
                        detour: None
                    }),
                    DnsServer::Local(LocalDnsServer { 
                        tag: "local".to_string(), 
                        detour: Some("direct".to_string()) 
                    })
                ],
                rules: vec![
                    DnsRule { 
                        domain_resolver: None,
                        server: Some("local".to_string()),
                        clash_mode: None,
                    }
                ]
            }),
            inbounds: generated_inbounds,
            outbounds, // Use our modified outbounds
            route: Some(RouteConfig {
                default_domain_resolver: Some("google".to_string()),
                rules: {
                    let mut rules = Vec::new();
                    
                    // 0. Route DNS
                    rules.push(RouteRule {
                        action: Some("route".to_string()),
                        protocol: Some(vec!["dns".to_string()]),
                        outbound: Some("direct".to_string()),
                        port: None, domain: None, geosite: None, geoip: None,
                        domain_resolver: None,
                    });

                    // 1. Block BitTorrent
                    if node.config_block_torrent {
                        rules.push(RouteRule {
                            action: Some("reject".to_string()),
                            protocol: Some(vec!["bittorrent".to_string()]),
                            outbound: None, port: None, domain: None, geosite: None, geoip: None,
                            domain_resolver: None,
                        });
                    }

                    // 2. Block Ads
                    if node.config_block_ads {
                        rules.push(RouteRule {
                            action: Some("reject".to_string()),
                            geosite: Some(vec!["category-ads-all".to_string()]),
                            outbound: None, protocol: None, port: None, domain: None, geoip: None,
                            domain_resolver: None,
                        });
                    }

                    // 3. Block Porn
                    if node.config_block_porn {
                        rules.push(RouteRule {
                            action: Some("reject".to_string()),
                            geosite: Some(vec!["category-porn".to_string()]),
                            outbound: None, protocol: None, port: None, domain: None, geoip: None,
                            domain_resolver: None,
                        });
                    }

                    // 4. Default Route (If Relay is active, this sends everything to relay-out)
                    if default_outbound_tag != "direct" {
                        rules.push(RouteRule {
                             action: Some("route".to_string()),
                             outbound: Some(default_outbound_tag),
                             // Match everything else
                             protocol: None, port: None, domain: None, geosite: None, geoip: None, domain_resolver: None,
                        });
                    }
                    
                    rules
                }
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
        use std::process::Command;
        use std::io::Write;
        
        // Serialize to JSON
        let config_json = serde_json::to_string_pretty(config)?;
        
        // Create temp file
        let mut temp_path = std::env::temp_dir();
        temp_path.push(format!("singbox_check_{}.json", uuid::Uuid::new_v4()));
        
        // Write to file
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(config_json.as_bytes())?;
        
        // Run sing-box check
        // We assume sing-box is in PATH. If not, this will fail.
        let output = Command::new("sing-box")
            .arg("check")
            .arg("-c")
            .arg(&temp_path)
            .output();
            
        // Clean up temp file immediately
        let _ = std::fs::remove_file(&temp_path);
        
        match output {
            Ok(out) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return Err(anyhow::anyhow!("Sing-box validation failed: {}", stderr));
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to execute sing-box binary: {}", e));
            }
        }
        
        Ok(())
    }
}
