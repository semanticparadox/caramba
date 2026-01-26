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
    pub ssh_user: String,
    pub ssh_port: i64,
    pub vpn_port: i64,
    pub ssh_password: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub join_token: Option<String>,
    pub auto_configure: bool,
    pub is_enabled: bool, // Added in migration 004
    pub country_code: Option<String>,
}
