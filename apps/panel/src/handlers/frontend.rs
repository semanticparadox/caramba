use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use crate::AppState;
use crate::models::frontend::{
    FrontendServer, CreateFrontendServer, FrontendHeartbeat,
    FrontendCreatedResponse, TokenRotateResponse,
};

/// List all frontend servers
pub async fn list_frontends(
    State(state): State<AppState>,
) -> Result<Json<Vec<FrontendServer>>, StatusCode> {
    let frontends = sqlx::query_as::<_, FrontendServer>(
        "SELECT * FROM frontend_servers ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to fetch frontends: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    Ok(Json(frontends))
}

/// Get active frontends for a region
pub async fn get_active_frontends(
    Path(region): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<FrontendServer>>, StatusCode> {
    let frontends = sqlx::query_as::<_, FrontendServer>(
        "SELECT * FROM frontend_servers 
         WHERE region = ? AND is_active = 1 
         ORDER BY last_heartbeat DESC"
    )
    .bind(&region)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to fetch active frontends: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    Ok(Json(frontends))
}

/// Create new frontend server
pub async fn create_frontend(
    State(state): State<AppState>,
    Json(payload): Json<CreateFrontendServer>,
) -> Result<Json<FrontendCreatedResponse>, StatusCode> {
    // Generate token and hash (secure)
    let (token, token_hash) = generate_frontend_token_with_hash(&payload.domain)?;
    let expires_at = calculate_token_expiration();
    
    let result = sqlx::query(
        "INSERT INTO frontend_servers 
         (domain, ip_address, region, auth_token_hash, token_expires_at, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
    )
    .bind(&payload.domain)
    .bind(&payload.ip_address)
    .bind(&payload.region)
    .bind(&token_hash)  // Store hash, not plaintext
   .bind(&expires_at)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create frontend: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    let frontend_id = result.last_insert_rowid();
    
    // Get the created frontend
    let frontend = sqlx::query_as::<_, FrontendServer>(
        "SELECT * FROM frontend_servers WHERE id = ?"
    )
    .bind(frontend_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(FrontendCreatedResponse {
        frontend,
        auth_token: token.clone(),  // Clone to use in next line
        install_command: generate_install_command(&payload.domain, &token, &payload.region),
    }))
}

/// Delete frontend server
pub async fn delete_frontend(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query("DELETE FROM frontend_servers WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete frontend: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(StatusCode::NO_CONTENT)
}

/// Handle heartbeat from frontend
pub async fn frontend_heartbeat(
    Path(domain): Path<String>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,  // Get headers for auth
    Json(data): Json<FrontendHeartbeat>,
) -> Result<StatusCode, StatusCode> {
    // Extract and validate bearer token
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            tracing::warn!("Missing Authorization header for frontend: {}", domain);
            StatusCode::UNAUTHORIZED
        })?;
    
    // Get frontend from database
    let frontend = sqlx::query_as::<_, FrontendServer>(
        "SELECT * FROM frontend_servers WHERE domain = ? AND is_active = 1"
    )
    .bind(&domain)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Database error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or_else(|| {
        tracing::warn!("Frontend not found or inactive: {}", domain);
        StatusCode::NOT_FOUND
    })?;
    
    // Validate token hash
    let token_hash = frontend.auth_token_hash
        .ok_or_else(|| {
            tracing::warn!("Frontend {} has no token hash (needs rotation)", domain);
            StatusCode::UNAUTHORIZED
        })?;
    
    let valid = bcrypt::verify(token, &token_hash)
        .map_err(|e| {
            tracing::error!("Token verification error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    if !valid {
        tracing::warn!("Invalid token for frontend: {}", domain);
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    // Check expiration
    if let Some(expires) = frontend.token_expires_at {
        if expires < chrono::Utc::now() {
            tracing::warn!("Expired token for frontend: {} (expired: {})", domain, expires);
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    
    // Token is valid - update heartbeat
    sqlx::query(
        "UPDATE frontend_servers 
         SET last_heartbeat = CURRENT_TIMESTAMP,
             traffic_monthly = traffic_monthly + ?
         WHERE domain = ?"
    )
    .bind(data.bandwidth_used as i64)
    .bind(&domain)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to update heartbeat: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    // Record stats
    sqlx::query(
        "INSERT INTO frontend_server_stats (frontend_id, requests_count, bandwidth_used)
         VALUES (?, ?, ?)"
    )
    .bind(frontend.id)
    .bind(data.requests_count as i64)
    .bind(data.bandwidth_used as i64)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to record stats: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    Ok(StatusCode::OK)
}

/// Rotate frontend server auth token
pub async fn rotate_token(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Json<TokenRotateResponse>, StatusCode> {
    // 1. Get frontend
    let frontend = sqlx::query_as::<_, FrontendServer>(
        "SELECT * FROM frontend_servers WHERE id = ?"
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;

    // 2. Generate new token
    let (token, token_hash) = generate_frontend_token_with_hash(&frontend.domain)?;
    let expires_at = calculate_token_expiration();

    // 3. Update DB
    sqlx::query(
        "UPDATE frontend_servers 
         SET auth_token_hash = ?, 
             token_expires_at = ?,
             updated_at = CURRENT_TIMESTAMP 
         WHERE id = ?"
    )
    .bind(&token_hash)
    .bind(&expires_at)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to rotate token: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(TokenRotateResponse {
        token: token.clone(),
        expires_at: expires_at,
        instructions: generate_install_command(&frontend.domain, &token, &frontend.region),
    }))
}


// Helper functions
/// Generate frontend token with bcrypt hash
/// Returns (plaintext_token, bcrypt_hash) - plaintext shown ONCE, hash stored in DB
/// 
/// Security improvements:
/// - Uses bcrypt for one-way hashing (cost 12)
/// - Token includes 256 bits of cryptographically random data
/// - Domain prefix for easy identification
fn generate_frontend_token_with_hash(domain: &str) -> Result<(String, String), StatusCode> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    
    // Generate 32 bytes of randomness (256 bits)
    let random_bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    
    // Create token with domain prefix for identification
    let token = format!(
        "fe_{}_{}",
        domain.replace('.', "_"),
        hex::encode(&random_bytes)
    );
    
    // Hash token for storage (bcrypt cost 12 for good security/performance balance)
    let hash = bcrypt::hash(&token, 12)
        .map_err(|e| {
            tracing::error!("Failed to hash frontend token: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok((token, hash))
}

/// Calculate token expiration (1 year from now)
fn calculate_token_expiration() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() + chrono::Duration::days(365)
}

fn generate_install_command(domain: &str, token: &str, region: &str) -> String {
    // Get panel URL from environment (SERVER_DOMAIN) or use fallback
    let panel_url = std::env::var("SERVER_DOMAIN")
        .map(|d| {
            if d.starts_with("http://") || d.starts_with("https://") {
                d
            } else {
                format!("https://{}", d)
            }
        })
        .unwrap_or_else(|_| "https://panel.example.com".to_string());
    
    format!(
        "curl -sSL {}/install.sh | \\\\\\n  sudo bash -s -- \\\\\\n  --role frontend \\\\\\n  --domain {} \\\\\\n  --token {} \\\\\\n  --region {} \\\\\\n  --panel {}",
        panel_url, domain, token, region, panel_url
    )
}

// Response types
#[derive(Serialize)]
pub struct FrontendServerResponse {
    pub frontend: FrontendServer,
    pub install_command: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub instructions: String,
}
