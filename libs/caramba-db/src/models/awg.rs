use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Серверная сторона AmneziaWG на конкретной ноде (таблица `node_awg`).
///
/// Живёт отдельно от inbounds потому, что AWG на ноде это не инбаунд sing-box,
/// а отдельный процесс amneziawg-go с интерфейсом awg0: стоковый sing-box
/// wireguard-inbound с полями обфускации не понимает и падает на проверке.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NodeAwg {
    pub node_id: i64,
    pub listen_port: i32,
    /// X25519 в стандартном base64 (формат wg). В UAPI уходит hex.
    pub private_key: String,
    pub public_key: String,
    pub address_cidr: String,
    pub jc: i32,
    pub jmin: i32,
    pub jmax: i32,
    pub s1: i32,
    pub s2: i32,
    /// h1..h4 это 32-битные маркеры типов пакетов, в i32 они не влезают.
    pub h1: i64,
    pub h2: i64,
    pub h3: i64,
    pub h4: i64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Клиентская пара ключей подписки и её адрес в пуле AWG.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SubscriptionAwgKey {
    pub subscription_id: i64,
    pub public_key: String,
    pub private_key: String,
    pub allowed_ip: String,
    pub created_at: DateTime<Utc>,
}

/// Онлайн и дельты трафика по пользователю глазами одной ноды.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NodeUserActivity {
    pub node_id: i64,
    pub user_tag: String,
    pub last_seen_at: DateTime<Utc>,
    pub online: bool,
    pub rx_delta: i64,
    pub tx_delta: i64,
}
