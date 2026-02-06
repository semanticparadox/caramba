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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmneziaWgSettings {
    pub users: Vec<AmneziaWgUser>,
    pub private_key: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmneziaWgUser {
    pub name: String,
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
    pub up_mbps: i32,
    pub down_mbps: i32,
    // Optional fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs: Option<Hysteria2Obfs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masquerade: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub ttype: String, // "salamander"
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hysteria2User {
    pub name: String,
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
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fallback {
    pub dest: String, // 80 or 8080 or domain
    pub xver: i32,
}

// Stream Settings

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSettings {
    pub network: Option<String>, // "tcp", "udp", "quic", "grpc"
    pub security: Option<String>, // "none", "tls", "reality"
    pub tls_settings: Option<TlsSettings>,
    pub reality_settings: Option<RealitySettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSettings {
    pub server_name: String,
    pub certificates: Option<Vec<Certificate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub certificate_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealitySettings {
    pub show: bool,
    pub dest: String,
    pub xver: i32,
    pub server_names: Vec<String>,
    pub private_key: String,
    pub short_ids: Vec<String>,
    // Optional
    pub max_time_diff: Option<i64>,
}
