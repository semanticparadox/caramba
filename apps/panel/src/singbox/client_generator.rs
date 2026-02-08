use serde::{Deserialize, Serialize};
use crate::singbox::config::{LogConfig, RouteConfig, RouteRule};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientProfile {
    pub log: LogConfig,
    pub dns: Option<DnsConfig>,
    pub inbounds: Vec<ClientInbound>,
    pub outbounds: Vec<ClientOutbound>,
    pub route: RouteConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DnsConfig {
    pub servers: Vec<DnsServer>,
    pub rules: Vec<DnsRule>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DnsServer {
    pub tag: String,
    pub address: String,
    pub detour: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DnsRule {
    pub outbound: Option<String>,
    pub server: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClientInbound {
    #[serde(rename = "mixed")]
    Mixed {
        tag: String,
        listen: String,
        listen_port: u16,
    },
    #[serde(rename = "tun")]
    Tun {
        tag: String,
        interface_name: String,
        inet4_address: String,
        auto_route: bool,
        strict_route: bool,
    }
}

// Client Outbounds (can be Groups or Proxies)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClientOutbound {
    #[serde(rename = "selector")]
    Selector {
        tag: String,
        outbounds: Vec<String>,
        default: Option<String>,
    },
    #[serde(rename = "urltest")]
    UrlTest {
        tag: String,
        outbounds: Vec<String>,
        url: Option<String>,
        interval: Option<String>,
        tolerance: Option<u16>,
    },
    #[serde(rename = "direct")]
    Direct {
        tag: String,
    },
    #[serde(rename = "vless")]
    Vless(ClientVlessOutbound),
    #[serde(rename = "hysteria2")]
    Hysteria2(ClientHysteria2Outbound),
    #[serde(rename = "wireguard")]
    AmneziaWg(ClientAmneziaWgOutbound),
    #[serde(rename = "trojan")]
    Trojan(ClientTrojanOutbound),
    #[serde(rename = "tuic")]
    Tuic(ClientTuicOutbound),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientAmneziaWgOutbound {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub local_address: Vec<String>, // ["10.10.0.2/32"]
    pub private_key: String,
    pub peer_public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preshared_key: Option<String>,
    // AmneziaWG Obfuscation
    pub jc: u16,
    pub jmin: u16,
    pub jmax: u16,
    pub s1: u16,
    pub s2: u16,
    pub h1: u32,
    pub h2: u32,
    pub h3: u32,
    pub h4: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientVlessOutbound {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub uuid: String,
    pub flow: Option<String>,
    pub packet_encoding: Option<String>,
    pub tls: Option<ClientTlsConfig>, 
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientTrojanOutbound {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub password: String,
    pub tls: Option<ClientTlsConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientTuicOutbound {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub uuid: String,
    pub password: String,
    pub congestion_control: String,
    pub tls: ClientTlsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientHysteria2Outbound {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub password: String, // user:pass for server, but client usually just sends pass if auth is simple, but H2 RFC says auth payload
    // In sing-box client: just "password" field.
    pub tls: ClientTlsConfig,
    pub obfs: Option<ClientObfs>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientTlsConfig {
    pub enabled: bool,
    pub server_name: String,
    pub insecure: bool,
    pub alpn: Option<Vec<String>>,
    pub utls: Option<UtlsConfig>,
    pub reality: Option<ClientRealityConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UtlsConfig {
    pub enabled: bool,
    pub fingerprint: String, // chrome
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientRealityConfig {
    pub enabled: bool,
    pub public_key: String,
    pub short_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientObfs {
    #[serde(rename = "type")]
    pub ttype: String,
    pub password: String,
}

pub struct ClientGenerator;

impl ClientGenerator {
    /// Generates a full Sing-box Client Profile (JSON)
    /// Aggregates multiple proxies into a "Best Latency" group and a "Select" group.
    pub fn generate(
        proxies: Vec<ClientOutbound>, 
        country_code: &str // "ru" might trigger specific optimized DNS
    ) -> ClientProfile {
        
        let mut outbounds = Vec::new();
        let mut all_proxy_tags = Vec::new();
        let mut reality_tags = Vec::new();
        let mut hysteria2_tags = Vec::new();
        let mut awg_tags = Vec::new();
        let mut tuic_tags = Vec::new();

        // 1. Add Actual Proxies + Group by Protocol
        for p in proxies {
            match &p {
                ClientOutbound::Vless(v) => {
                    all_proxy_tags.push(v.tag.clone());
                    reality_tags.push(v.tag.clone());
                },
                ClientOutbound::Hysteria2(h) => {
                    all_proxy_tags.push(h.tag.clone());
                    hysteria2_tags.push(h.tag.clone());
                },
                ClientOutbound::AmneziaWg(a) => {
                    all_proxy_tags.push(a.tag.clone());
                    awg_tags.push(a.tag.clone());
                },
                ClientOutbound::Trojan(t) => {
                    all_proxy_tags.push(t.tag.clone());
                    reality_tags.push(t.tag.clone()); // Group with Reality for now or direct? 
                },
                ClientOutbound::Tuic(t) => {
                    all_proxy_tags.push(t.tag.clone());
                    tuic_tags.push(t.tag.clone());
                }
                _ => {}
            }
            outbounds.push(p);
        }

        // 2. Create Protocol-Specific UrlTest Groups
        let mut protocol_group_tags = Vec::new();

        if !reality_tags.is_empty() {
            let reality_group = ClientOutbound::UrlTest {
                tag: "⚡ Reality".to_string(),
                outbounds: reality_tags,
                url: Some("http://www.gstatic.com/generate_204".to_string()),
                interval: Some("10m".to_string()),
                tolerance: Some(50),
            };
            protocol_group_tags.push("⚡ Reality".to_string());
            outbounds.insert(0, reality_group);
        }

        if !hysteria2_tags.is_empty() {
            let hy2_group = ClientOutbound::UrlTest {
                tag: "⚡ Hysteria2".to_string(),
                outbounds: hysteria2_tags,
                url: Some("http://www.gstatic.com/generate_204".to_string()),
                interval: Some("10m".to_string()),
                tolerance: Some(50),
            };
            protocol_group_tags.push("⚡ Hysteria2".to_string());
            outbounds.insert(0, hy2_group);
        }

        if !awg_tags.is_empty() {
            let awg_group = ClientOutbound::UrlTest {
                tag: "⚡ AmneziaWG".to_string(),
                outbounds: awg_tags,
                url: Some("http://www.gstatic.com/generate_204".to_string()),
                interval: Some("10m".to_string()),
                tolerance: Some(50),
            };
            protocol_group_tags.push("⚡ AmneziaWG".to_string());
            outbounds.insert(0, awg_group);
        }

        if !tuic_tags.is_empty() {
             let tuic_group = ClientOutbound::UrlTest {
                tag: "⚡ TUIC".to_string(),
                outbounds: tuic_tags,
                url: Some("http://www.gstatic.com/generate_204".to_string()),
                interval: Some("10m".to_string()),
                tolerance: Some(50),
            };
            protocol_group_tags.push("⚡ TUIC".to_string());
            outbounds.insert(0, tuic_group);
        }

        // 3. Create "Auto Fast" (All Protocols)
        let auto_group = ClientOutbound::UrlTest {
            tag: "⚡ Auto".to_string(),
            outbounds: all_proxy_tags.clone(),
            url: Some("http://www.gstatic.com/generate_204".to_string()),
            interval: Some("3m".to_string()),
            tolerance: Some(50), 
        };
        outbounds.insert(0, auto_group);

        // 4. Selector (Manual): Protocol Groups First, then Individual Servers
        let mut selector_tags = vec!["⚡ Auto".to_string()];
        selector_tags.extend(protocol_group_tags);
        selector_tags.extend(all_proxy_tags);
        
        let select_group = ClientOutbound::Selector {
            tag: "🚀 Proxy".to_string(),
            outbounds: selector_tags,
            default: Some("⚡ Auto".to_string()),
        };
        outbounds.insert(0, select_group);

        // Direct
        outbounds.push(ClientOutbound::Direct { tag: "direct".to_string() });

        // 3. DNS (Optimized for RU)
        let dns = if country_code == "ru" {
            Some(DnsConfig {
                servers: vec![
                    DnsServer { tag: "google".to_string(), address: "8.8.8.8".to_string(), detour: Some("🚀 Proxy".to_string()) },
                    DnsServer { tag: "local".to_string(), address: "local".to_string(), detour: Some("direct".to_string()) },
                ],
                rules: vec![
                    DnsRule { outbound: Some("direct".to_string()), server: "local".to_string() }, // Fallback logic or rule sets needed
                ]
            })
        } else {
            None
        };

        // 4. Routes
        let route = RouteConfig {
            rules: vec![
                RouteRule {
                    protocol: Some(vec!["dns".to_string()]),
                    outbound: Some("dns-out".to_string()),
                    action: None, port: None, domain: None, geosite: None, geoip: None
                },
                RouteRule {
                    outbound: Some("direct".to_string()),
                    domain: Some(vec!["geosite:cn".to_string(), "geosite:private".to_string()]), 
                    action: None, port: None, protocol: None, geosite: None, geoip: None
                },
                // Default rule (implicit in sing-box if no match? No, needs default)
                // Actually sing-box routes to first outbound used in "rules" or just select first one?
                // Sing-box needs explicit final rule or it loops?
                // Usually it picks the first outbound in the list as default if no rules match.
                // Our first outbound is "🚀 Proxy".
            ]
        };

        ClientProfile {
            log: LogConfig { level: "warn".to_string(), timestamp: true },
            dns,
            inbounds: vec![
               // Default TUN or Mixed?
               // For "Link" export, we don't usually define Inbounds (client app handles it).
               // But if we export FULL PROFILE (JSON), we often include a TUN inbound for desktop users.
               // Let's assume this is for "Import Profile" feature.
               ClientInbound::Tun {
                   tag: "tun-in".to_string(),
                   interface_name: "tun0".to_string(),
                   inet4_address: "172.19.0.1/30".to_string(),
                   auto_route: true,
                   strict_route: true,
               }
            ],
            outbounds,
            route,
        }
    }
}
