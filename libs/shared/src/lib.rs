use serde::{Deserialize, Serialize};

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
