use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiscoveredSni {
    pub domain: String,
    pub ip: String,
    pub latency_ms: u32,
    pub h2: bool,
    pub h3: bool,
}

pub mod api {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct HeartbeatRequest {
        pub version: String,
        pub uptime: u64,
        pub status: String,
        pub config_hash: Option<String>,
        pub traffic_up: u64,
        pub traffic_down: u64,
        pub certificates: Option<Vec<CertificateStatus>>,
        // Telemetry
        pub latency: Option<f64>,
        pub cpu_usage: Option<f64>,
        pub memory_usage: Option<f64>,
        pub speed_mbps: Option<i32>,
        pub active_connections: Option<u32>, // Added for Telemetry (Phase 3)
        /// Per-user traffic usage. Key is User Tag (e.g. "user_123"), value is bytes used.
        pub user_usage: Option<std::collections::HashMap<String, u64>>,
        pub discovered_snis: Option<Vec<DiscoveredSni>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct CertificateStatus {
        pub sni: String,
        pub valid: bool,
        pub expires_at: i64,
        pub error: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct HeartbeatResponse {
        pub success: bool,
        pub action: AgentAction,
        pub latest_version: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum AgentAction {
        None,
        UpdateConfig,
        RestartService,
    }
}

pub mod config {
    use super::*;
    
    #[derive(Debug, Serialize, Deserialize)]
    pub struct ConfigResponse {
        pub hash: String,
        pub content: serde_json::Value,
    }
}
