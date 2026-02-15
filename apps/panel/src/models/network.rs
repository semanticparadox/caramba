use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Inbound {
    pub id: i64,
    pub node_id: i64,
    pub tag: String,
    pub protocol: String, // 'vless', 'hysteria2', 'trojan', etc.
    pub listen_port: i64, // SQLite integer is i64
    pub listen_ip: String,
    
    // Stored as raw JSON strings in DB, parsed to structs in app logic if needed
    // or kept as serde_json::Value for flexibility
    pub settings: String, 
    pub stream_settings: String,

    pub remark: Option<String>,
    pub enable: bool,
    pub renew_interval_mins: i64,
    pub port_range_start: i64,
    pub port_range_end: i64,
    pub last_rotated_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}


#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(dead_code)]
pub struct PlanInbound {
    pub plan_id: i64,
    pub inbound_id: i64,
    pub created_at: Option<DateTime<Utc>>,
}

// Helper structs for strong typing when parsing the JSON fields

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum InboundType {
    Vless(VlessSettings),
    Hysteria2(Hysteria2Settings),
    Trojan(TrojanSettings),
    #[serde(rename = "amneziawg")]
    AmneziaWg(AmneziaWgSettings),
    Tuic(TuicSettings),
    Naive(NaiveSettings),
    Shadowsocks(ShadowsocksSettings),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NaiveSettings {
    pub users: Vec<NaiveUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NaiveUser {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowsocksSettings {
    pub method: String,
    pub users: Vec<ShadowsocksUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowsocksUser {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AmneziaWgSettings {
    pub users: Vec<AmneziaWgUser>,
    pub private_key: String,
    pub public_key: String,
    pub listen_port: u16,
    // Obfuscation parameters
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AmneziaWgUser {
    pub name: Option<String>,
    pub private_key: String, // Client's private key (usually we generate it)
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub client_ip: String, // e.g. 10.10.0.2/32
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlessSettings {
    pub clients: Vec<VlessClient>,
    pub decryption: String, // "none"
    pub fallbacks: Option<Vec<Fallback>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlessClient {
    pub id: String, // UUID
    pub flow: String, // "xtls-rprx-vision"
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hysteria2Settings {
    pub users: Vec<Hysteria2User>,
    #[serde(default = "default_bw")]
    pub up_mbps: i32,
    #[serde(default = "default_bw")]
    pub down_mbps: i32,
    // Optional fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs: Option<Hysteria2Obfs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masquerade: Option<String>,
}

fn default_bw() -> i32 { 100 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub ttype: String, // "salamander"
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hysteria2User {
    pub name: Option<String>,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuicSettings {
    pub users: Vec<TuicUser>,
    #[serde(default = "default_congestion")]
    pub congestion_control: String,
    #[serde(default = "default_auth_timeout")]
    pub auth_timeout: String,
    #[serde(default)]
    pub zero_rtt_handshake: bool,
    #[serde(default = "default_heartbeat")]
    pub heartbeat: String,
}

fn default_congestion() -> String { "cubic".to_string() }
fn default_auth_timeout() -> String { "3s".to_string() }
fn default_heartbeat() -> String { "10s".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuicUser {
    pub name: Option<String>,
    pub uuid: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrojanSettings {
    pub clients: Vec<TrojanClient>,
    pub fallback: Option<Fallback>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrojanClient {
    pub password: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fallback {
    pub dest: String, // 80 or 8080 or domain
    pub xver: i32,
}

// Stream Settings

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct StreamSettings {
    pub network: Option<String>, // "tcp", "udp", "quic", "grpc"
    pub security: Option<String>, // "none", "tls", "reality"
    #[serde(alias = "tlsSettings", default)]
    pub tls_settings: Option<TlsSettings>,
    #[serde(alias = "realitySettings", default)]
    pub reality_settings: Option<RealitySettings>,
    #[serde(alias = "wsSettings", default)]
    pub ws_settings: Option<WsSettings>,
    #[serde(alias = "httpUpgradeSettings", default)]
    pub http_upgrade_settings: Option<HttpUpgradeSettings>,
    #[serde(alias = "xhttpSettings", default)]
    pub xhttp_settings: Option<XhttpSettings>,
    #[serde(alias = "packetEncoding", default)]
    pub packet_encoding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XhttpSettings {
    pub path: String,
    pub host: String,
    pub mode: Option<String>,
    pub extra: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSettings {
    pub path: String,
    pub headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpUpgradeSettings {
    pub path: String,
    pub host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSettings {
    #[serde(alias = "serverName")]
    pub server_name: String,
    pub certificates: Option<Vec<Certificate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub certificate_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RealitySettings {
    pub show: bool,
    pub dest: String,
    pub xver: i32,
    #[serde(alias = "serverNames", default)]
    pub server_names: Vec<String>,
    #[serde(alias = "privateKey", default)]
    pub private_key: String,
    #[serde(alias = "publicKey", default)]
    pub public_key: Option<String>,
    #[serde(alias = "shortIds", default)]
    pub short_ids: Vec<String>,
    // Optional
    pub max_time_diff: Option<i64>,
}
