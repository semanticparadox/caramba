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
        /// Кто живой на узле прямо сейчас и сколько прокачал за интервал.
        ///
        /// Один список на оба источника: sing-box-пользователи приходят из
        /// того же v2ray-дельта-пути, что и `user_usage` (online = дельта за
        /// интервал больше нуля), AWG-пиры из UAPI `get=1` (online =
        /// last_handshake свежее 180 секунд). Панель складывает это в
        /// `node_user_activity`; трафик по-прежнему считается из `user_usage`,
        /// чтобы байты не удвоились.
        ///
        /// `#[serde(default)]` = старый агент поля не шлёт, панель видит None.
        #[serde(default)]
        pub active_users: Option<Vec<ActiveUser>>,
    }

    /// Строка `active_users` в heartbeat.
    #[derive(Debug, Serialize, Deserialize, Clone, Default)]
    pub struct ActiveUser {
        /// Тег пользователя в конфиге узла: `user_{client_id}`.
        pub tag: String,
        /// Принято от клиента за интервал, байт.
        #[serde(default)]
        pub rx_delta: u64,
        /// Отдано клиенту за интервал, байт.
        #[serde(default)]
        pub tx_delta: u64,
        /// Онлайн на момент heartbeat.
        #[serde(default)]
        pub online: bool,
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
        /// Хеш ТОЛЬКО sing-box-конфига (`content`). Смысл прежний: его смена
        /// это единственный повод перезаписать /etc/sing-box/config.json и
        /// перезапустить sing-box. Раздел `awg` в него намеренно не входит,
        /// иначе добавление одного AWG-пира рвало бы всем живым клиентам
        /// sing-box-сессии.
        pub hash: String,
        pub content: serde_json::Value,
        /// Сервер AmneziaWG узла. Лежит РЯДОМ с sing-box-конфигом, а не внутри
        /// него: стоковый sing-box такой инбаунд не понимает и падает на
        /// проверке конфига. `None` = панель этим узлом AWG не управляет.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub awg: Option<AwgSection>,
        /// Хеш раздела `awg`. Отдельный от `hash` ровно затем, чтобы агент мог
        /// применить изменение пиров через UAPI, не трогая sing-box.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub awg_hash: Option<String>,
    }

    /// Раздел `awg` конфига узла. Агент читает ровно эти поля.
    #[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
    pub struct AwgSection {
        /// Поднимать ли awg0 вообще. false = агент гасит интерфейс.
        pub enabled: bool,
        /// UDP-порт awg0.
        pub listen_port: u16,
        /// Приватный ключ сервера, X25519 в стандартном base64 (формат wg).
        /// В UAPI уходит hex — перевод делает агент.
        pub private_key: String,
        /// Адрес интерфейса вместе с маской пула пиров, например 10.66.0.1/16.
        pub address_cidr: String,
        /// Параметры обфускации AmneziaWG.
        pub jc: u16,
        pub jmin: u16,
        pub jmax: u16,
        pub s1: u16,
        pub s2: u16,
        pub h1: u32,
        pub h2: u32,
        pub h3: u32,
        pub h4: u32,
        /// Полный список пиров. Не дельта: агент приводит awg0 ровно к этому
        /// составу, лишние пиры удаляет.
        #[serde(default)]
        pub peers: Vec<AwgPeer>,
    }

    /// Пир AWG: одна подписка.
    #[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
    pub struct AwgPeer {
        /// Публичный ключ клиента, X25519 в стандартном base64.
        pub public_key: String,
        /// Адрес пира с маской, например 10.66.1.7/32.
        pub allowed_ip: String,
        /// Тег пользователя `user_{client_id}` — по нему агент отдаёт трафик
        /// и онлайн обратно в heartbeat.active_users.
        pub user_tag: String,
    }
}

/// Контракт узел-панель. Поля `awg`/`awg_hash`/`active_users` добавлены
/// аддитивно: агент старой версии не шлёт и не ждёт их, и рассинхрон версий
/// не должен ронять ни одну из сторон.
#[cfg(test)]
mod contract_tests {
    use super::api::HeartbeatRequest;
    use super::config::{AwgPeer, AwgSection, ConfigResponse};

    #[test]
    fn old_config_response_without_awg_still_parses() {
        let raw = r#"{"hash":"abc","content":{"inbounds":[]}}"#;
        let parsed: ConfigResponse = serde_json::from_str(raw).expect("старый конфиг узла");
        assert_eq!(parsed.hash, "abc");
        assert!(parsed.awg.is_none());
        assert!(parsed.awg_hash.is_none());
    }

    #[test]
    fn config_response_without_awg_serializes_without_the_keys() {
        // Старый агент разбирает ответ строго; лишние null-поля ему не нужны.
        let body = serde_json::to_string(&ConfigResponse {
            hash: "abc".into(),
            content: serde_json::json!({}),
            awg: None,
            awg_hash: None,
        })
        .expect("сериализация");
        assert!(!body.contains("awg"), "{body}");
    }

    #[test]
    fn awg_section_round_trips() {
        let section = AwgSection {
            enabled: true,
            listen_port: 17400,
            private_key: "cHJpdg==".into(),
            address_cidr: "10.66.0.1/16".into(),
            jc: 5,
            jmin: 50,
            jmax: 700,
            s1: 30,
            s2: 40,
            h1: 4_000_000_000,
            h2: 2,
            h3: 3,
            h4: 4,
            peers: vec![AwgPeer {
                public_key: "cHVi".into(),
                allowed_ip: "10.66.1.9/32".into(),
                user_tag: "user_-46".into(),
            }],
        };
        let body = serde_json::to_string(&section).expect("сериализация");
        let back: AwgSection = serde_json::from_str(&body).expect("разбор");
        assert_eq!(section, back);
        // h1..h4 обязаны пережить значения выше i32::MAX.
        assert!(body.contains("4000000000"), "{body}");
    }

    #[test]
    fn old_heartbeat_without_active_users_still_parses() {
        let raw = r#"{"version":"0.9.80","uptime":1,"status":"ok","config_hash":null,
            "traffic_up":0,"traffic_down":0,"certificates":null,"latency":null,
            "cpu_usage":null,"memory_usage":null,"max_ram":null,"cpu_cores":null,
            "cpu_model":null,"speed_mbps":null,"active_connections":null,"user_usage":null,
            "discovered_snis":null}"#;
        let parsed: HeartbeatRequest = serde_json::from_str(raw).expect("старый heartbeat");
        assert!(parsed.active_users.is_none());
    }
}
