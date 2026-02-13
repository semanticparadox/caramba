use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]


pub struct SingBoxConfig {
    pub log: LogConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<serde_json::Value>, 
    pub inbounds: Vec<Inbound>,
    pub outbounds: Vec<Outbound>,
    pub route: Option<RouteConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<ExperimentalConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExperimentalConfig {
    pub clash_api: ClashApiConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClashApiConfig {
    pub external_controller: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ui: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_control_allow_origin: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_control_allow_private_network: Option<bool>,
}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogConfig {
    pub level: String,
    pub timestamp: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Inbound {
    Vless(VlessInbound),
    Hysteria2(Hysteria2Inbound),
    #[serde(rename = "wireguard")]
    AmneziaWg(AmneziaWgInbound),
    Trojan(TrojanInbound),
    Tuic(TuicInbound),
    Http(HttpInbound),
    Naive(NaiveInbound),
    Shadowsocks(ShadowsocksInbound),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<HttpUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<VlessTlsConfig>, 
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpUser {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NaiveInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<NaiveUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<VlessTlsConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NaiveUser {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<VlessUser>,
    pub tls: Option<VlessTlsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<VlessTransportConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowsocksInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub method: String,
    pub users: Vec<ShadowsocksUser>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowsocksUser {
    pub name: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessUser {
    pub name: String,
    pub uuid: String,
    pub flow: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VlessTransportConfig {
    Ws(WsTransport),
    HttpUpgrade(HttpUpgradeTransport),
    #[serde(rename = "xhttp")]
    Xhttp(XhttpTransport),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct XhttpTransport {
    pub path: String,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WsTransport {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpUpgradeTransport {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessTlsConfig {
    pub enabled: bool,
    pub server_name: String,
    // ALPN often needed for Vision/Reality
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
    pub reality: RealityConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RealityConfig {
    pub enabled: bool,
    pub handshake: RealityHandshake,
    pub private_key: String,
    pub short_id: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RealityHandshake {
    pub server: String,
    pub server_port: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2Inbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<Hysteria2User>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_mbps: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down_mbps: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_client_bandwidth: Option<bool>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs: Option<Hysteria2Obfs>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub masquerade: Option<String>,
    
    pub tls: Hysteria2TlsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub ttype: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2User {
    pub name: Option<String>,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2TlsConfig {
    pub enabled: bool,
    pub server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AmneziaWgInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub peers: Vec<AmneziaWgUser>,
    pub private_key: String,
    // AmneziaWG specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jc: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jmin: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jmax: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s1: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s2: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h2: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h3: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h4: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AmneziaWgUser {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preshared_key: Option<String>,
    pub allowed_ips: Vec<String>,
}


#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outbound {
    Direct { tag: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrojanInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<TrojanUser>,
    pub tls: Option<VlessTlsConfig>, // Can reuse VlessTlsConfig or define TrojanTlsConfig
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrojanUser {
    pub name: Option<String>,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TuicInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<TuicUser>,
    pub congestion_control: String,
    pub auth_timeout: String,
    pub zero_rtt_handshake: bool,
    pub heartbeat: String,
    pub tls: TuicTlsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TuicUser {
    pub name: Option<String>,
    pub uuid: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TuicTlsConfig {
    pub enabled: bool,
    pub server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteConfig {
    pub rules: Vec<RouteRule>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geosite: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geoip: Option<Vec<String>>,
}
