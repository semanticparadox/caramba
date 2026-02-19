use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Organization {
    pub id: i64,
    pub name: String,
    pub slug: Option<String>,
    pub balance: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrganizationMember {
    pub organization_id: i64,
    pub user_id: i64,
    pub role: String, // 'owner', 'admin', 'member'
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrgRole {
    Owner,
    Admin,
    Member,
}

impl From<String> for OrgRole {
    fn from(s: String) -> Self {
        match s.as_str() {
            "owner" => OrgRole::Owner,
            "admin" => OrgRole::Admin,
            _ => OrgRole::Member,
        }
    }
}

impl ToString for OrgRole {
    fn to_string(&self) -> String {
        match self {
            OrgRole::Owner => "owner".to_string(),
            OrgRole::Admin => "admin".to_string(),
            OrgRole::Member => "member".to_string(),
        }
    }
}
