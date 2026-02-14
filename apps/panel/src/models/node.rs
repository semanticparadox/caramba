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
    
    // Relay Support (Phase 8)
    #[sqlx(default)]
    pub relay_id: Option<i64>,

    #[sqlx(default)]
    pub active_connections: Option<i32>,

    #[sqlx(default)]
    pub total_ingress: i64,
    #[sqlx(default)]
    pub total_egress: i64,
    #[sqlx(default)]
    pub uptime: i64,
    #[sqlx(default)]
    pub last_session_ingress: i64,
    #[sqlx(default)]
    pub last_session_egress: i64,
    #[sqlx(default)]
    pub doomsday_password: Option<String>,
}

impl Node {
    pub fn cpu_rounded(&self) -> String {
        format!("{:.0}", self.last_cpu.unwrap_or(0.0))
    }
    pub fn ram_rounded(&self) -> String {
        format!("{:.0}", self.last_ram.unwrap_or(0.0))
    }
    pub fn latency_rounded(&self) -> String {
        format!("{:.0}", self.last_latency.unwrap_or(0.0))
    }

    pub fn format_uptime(&self) -> String {
        let total_seconds = self.uptime;
        if total_seconds == 0 { return "0s".to_string(); }
        
        let days = total_seconds / 86400;
        let hours = (total_seconds % 86400) / 3600;
        let minutes = (total_seconds % 3600) / 60;
        
        if days > 0 {
            format!("{}d {}h", days, hours)
        } else if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }

    pub fn format_traffic_ingress(&self) -> String {
        crate::utils::format_bytes_str(self.total_ingress as u64)
    }

    pub fn format_traffic_egress(&self) -> String {
        crate::utils::format_bytes_str(self.total_egress as u64)
    }
}
