use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NodeGroup {
    pub id: i64,
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NodeGroupMember {
    pub node_id: i64,
    pub group_id: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PlanGroup {
    pub plan_id: i64,
    pub group_id: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct InboundTemplate {
    pub id: i64,
    pub name: String,
    pub protocol: String,
    pub settings_template: String,
    pub stream_settings_template: String,
    pub target_group_id: Option<i64>,
    pub port_range_start: i64,
    pub port_range_end: i64,
    pub renew_interval_hours: i64,
    pub renew_interval_mins: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}
