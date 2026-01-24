use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]


pub struct SingBoxConfig {
    pub log: LogConfig,
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
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<VlessUser>,
    pub tls: Option<VlessTlsConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessUser {
    pub name: String,
    pub uuid: String,
    pub flow: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessTlsConfig {
    pub enabled: bool,
    pub server_name: String,
    pub reality: RealityConfig,
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
    pub tls: Hysteria2TlsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2User {
    pub name: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2TlsConfig {
    pub enabled: bool,
    pub server_name: String,
    pub key_path: Option<String>,
    pub certificate_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outbound {
    Direct { tag: String },
    Block { tag: String },
    Dns { tag: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteConfig {
    pub rules: Vec<RouteRule>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteRule {
    pub outbound: String,
    pub protocol: Option<Vec<String>>,
    pub port: Option<Vec<u16>>,
    pub domain: Option<Vec<String>>,
}
