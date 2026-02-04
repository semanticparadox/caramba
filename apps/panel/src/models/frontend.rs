use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FrontendServer {
    pub id: i64,
    pub domain: String,
    pub ip_address: String,
    pub region: String,
    /// Hashed authentication token (bcrypt) - never expose to client
    #[serde(skip_serializing)]  // Never send hash to client
    pub auth_token_hash: Option<String>,
    /// Legacy plaintext token - will be removed after migration
    #[serde(skip_serializing)]  // Never send to client
    #[allow(dead_code)]
    pub auth_token: Option<String>,
    pub is_active: bool,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub traffic_monthly: i64,
    /// Token expiration timestamp (default: 1 year from creation)
    pub token_expires_at: Option<DateTime<Utc>>,
    /// Last token rotation timestamp
    pub token_rotated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFrontendServer {
    pub domain: String,
    pub ip_address: String,
    pub region: String,
}

/// Response when creating a new frontend - includes token ONCE
#[derive(Serialize)]
pub struct FrontendCreatedResponse {
    pub frontend: FrontendServer,
    /// Plaintext token - shown only once, never stored or retrievable again
    pub auth_token: String,
    pub install_command: String,
}

/// Response when rotating a token
#[derive(Serialize)]
pub struct TokenRotateResponse {
    /// New plaintext token - shown only once
    pub token: String,
    /// New expiration timestamp
    pub expires_at: DateTime<Utc>,
    /// Instructions for updating the frontend
    pub instructions: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrontendHeartbeat {
    pub requests_count: u64,
    pub bandwidth_used: u64,
}
