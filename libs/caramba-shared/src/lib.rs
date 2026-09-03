/// Адрес gRPC-статистики sing-box (`experimental.v2ray_api.listen`). Панель
/// пишет его в конфиг узла, узел по нему опрашивает счётчики — поэтому константа
/// одна на двоих и живёт здесь. Порт намеренно высокий и необычный: первая
/// версия брала 8080, на узле Canada он был занят docker-proxy, sing-box падал
/// на старте с «address already in use», и VPN на узле лежал (2026-09-01).
pub const V2RAY_API_LISTEN: &str = "127.0.0.1:26517";

pub mod geo_service;
pub use geo_service::{GeoData, GeoService};

#[cfg(feature = "self-update")]
pub mod self_update;

pub mod license;

#[cfg(feature = "csm")]
pub mod csm;

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

    #[derive(Debug, Serialize, Deserialize, Clone)]
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
        pub max_ram: Option<u64>,
        pub cpu_cores: Option<i32>,
        pub cpu_model: Option<String>,
        pub speed_mbps: Option<i32>,
        pub active_connections: Option<u32>, // Added for Telemetry (Phase 3)
        /// Per-user traffic usage. Key is User Tag (e.g. "user_123"), value is bytes used.
        pub user_usage: Option<std::collections::HashMap<String, u64>>,
        /// Собран ли sing-box на этом узле с `with_v2ray_api`.
        ///
        /// Предохранитель, а не информация. Панель пишет секцию
        /// `experimental.v2ray_api` ТОЛЬКО тем узлам, которые ответили `true`:
        /// сборка без этого тега отвергает такую секцию при старте
        /// («v2ray api is not included in this build») и узел не поднимается
        /// вовсе. Без флага выкат панели раньше узлов положил бы VPN всем сразу.
        ///
        /// `#[serde(default)]` = `None` у старых агентов, которые поле не шлют;
        /// `None` трактуется как «не умеет», то есть в сторону безопасности.
        #[serde(default)]
        pub supports_v2ray_api: Option<bool>,
        pub discovered_snis: Option<Vec<DiscoveredSni>>,
        /// U22 (config versioning/ACK): hash of the config the node has actually
        /// applied AND successfully restarted sing-box with. The panel uses this
        /// to know rollout state and avoid SNI-rotation race conditions.
        /// `config_hash` is what the node *fetched*; this is what it has *applied*.
        /// Optional + `#[serde(default)]` so older nodes (which never send it)
        /// keep deserializing to `None` — backward compatible across version skew.
        #[serde(default)]
        pub last_applied_config_hash: Option<String>,
        /// U23 (RU-side block detection canary): early-RST / handshake-terminated-early
        /// symptoms observed by the node against its current SNI. Present only when the
        /// node detected suspicious termination behaviour. Optional for skew safety.
        #[serde(default)]
        pub block_signals: Option<BlockSignals>,
    }

    /// U23 — RU-side block detection canary payload.
    ///
    /// The Feb 2026 RU failure mode terminated TLS sessions very early (RST after
    /// SYN/ACK or during/just after the ClientHello), rather than failing DNS or
    /// the TCP connect. The node probes its active SNI and reports these symptoms
    /// so the panel can rotate SNI faster than the conservative 30-min cooldown.
    ///
    /// All fields are plain (non-Option) but the whole struct is optional on the
    /// heartbeat, and `#[serde(default)]` keeps it forward/backward compatible.
    #[derive(Debug, Serialize, Deserialize, Clone, Default)]
    pub struct BlockSignals {
        /// SNI the node probed.
        #[serde(default)]
        pub sni: String,
        /// Connection was reset very early (RST during/right after handshake start).
        #[serde(default)]
        pub early_rst: bool,
        /// TLS handshake was terminated early (peer closed mid-handshake / EOF before
        /// ServerHello completed) — classic DPI active-probe / RST-injection symptom.
        #[serde(default)]
        pub handshake_terminated_early: bool,
        /// Number of consecutive failing probes observed for this SNI (saturating).
        /// Lets the panel gauge confidence before reacting.
        #[serde(default)]
        pub consecutive_failures: u32,
        /// Free-form classification string (e.g. "early_rst", "tls_eof").
        #[serde(default)]
        pub detail: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
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
        CollectLogs,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct LogRequest {
        pub services: Vec<String>, // e.g., ["sing-box", "caramba-node", "nginx", "caddy"]
        pub include_config: bool,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct LogResponse {
        pub logs: std::collections::HashMap<String, String>,
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
