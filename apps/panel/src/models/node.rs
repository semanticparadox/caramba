use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Node {
    pub id: i64,
    pub name: String,
    pub ip: String,
    pub status: String,
    pub reality_pub: Option<String>,
    pub reality_priv: Option<String>,
    pub short_id: Option<String>,
    pub domain: Option<String>,
    pub root_password: Option<String>, // Added to match Schema (was missing)
    pub vpn_port: i64,
    pub last_seen: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub join_token: Option<String>,
    pub auto_configure: bool,
    pub is_enabled: bool, // Added in migration 004
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub flag: Option<String>,
    pub reality_sni: Option<String>,
    pub load_stats: Option<String>,
    pub check_stats_json: Option<String>,
    pub sort_order: i32,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,

    // Bandwidth Shaping & Policies
    #[sqlx(default)]
    pub config_qos_enabled: bool,
    #[sqlx(default)]
    pub config_block_torrent: bool,
    #[sqlx(default)]
    pub config_block_ads: bool,
    #[sqlx(default)]
    #[sqlx(default)]
    pub config_block_porn: bool,
    
    // Telemetry & Load Balancing (Added Phase 1.5)
    pub last_latency: Option<f64>,
    pub last_cpu: Option<f64>,
    pub last_ram: Option<f64>,
    #[sqlx(default)]
    pub speed_limit_mbps: i32,
    #[sqlx(default)]
    pub max_users: i32,
    #[sqlx(default)]
    pub current_speed_mbps: i32,
}
