use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct TelegramAuthRequest {
    pub init_data: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub active_subscriptions: usize,
    pub balance: f64,
}

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
    _username: Option<String>,
    // other fields like first_name, last_name, etc.
}

/// Helper to validate Telegram initData
fn validate_init_data(init_data: &str, bot_token: &str) -> Option<TelegramUser> {
    // 1. Parse query string into map
    let mut params: HashMap<String, String> = url::form_urlencoded::parse(init_data.as_bytes())
        .into_owned()
        .collect();

    // 2. Extract hash
    let hash = params.remove("hash")?;
    
    // 3. Sort keys and build data-check-string
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    
    let mut data_check_string = String::new();
    for key in keys {
        let value = params.get(key).unwrap();
        if !data_check_string.is_empty() {
            data_check_string.push('\n');
        }
        data_check_string.push_str(&format!("{}={}", key, value));
    }

    // 4. Calculate secret key (HMAC-SHA256 of bot_token using "WebAppData" as key)
    let secret_key = Hmac::<Sha256>::new_from_slice(b"WebAppData")
        .expect("HMAC can take key of any size")
        .chain_update(bot_token.as_bytes())
        .finalize()
        .into_bytes();

    // 5. Calculate HMAC-SHA256 of data-check-string using secret_key
    let calculated_hash = Hmac::<Sha256>::new_from_slice(&secret_key)
        .expect("HMAC can take key of any size")
        .chain_update(data_check_string.as_bytes())
        .finalize()
        .into_bytes();
    
    let calculated_hex = hex::encode(calculated_hash);

    // 6. Compare hashes
    if calculated_hex != hash {
        return None;
    }

    // 7. Parse user object
    if let Some(user_json) = params.get("user") {
        return serde_json::from_str(user_json).ok();
    }

    None
}

#[derive(Serialize)]
pub struct UserStatsResponse {
    pub traffic_used: i64,
    pub traffic_limit: i64,
    pub days_left: i64,
    pub plan_name: String,
    pub total_download: i64,
    pub total_upload: i64,
}

/// Simple helper to sign user_id
fn create_token(user_id: i64, secret: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(user_id.to_string().as_bytes());
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());
    format!("{}.{}", user_id, signature)
}

/// Simple helper to verify token and get user_id
fn verify_token(token: &str, secret: &str) -> Option<i64> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let user_id_str = parts[0];
    let signature = parts[1];
    let user_id = user_id_str.parse::<i64>().ok()?;
    
    let expected_token = create_token(user_id, secret);
    let expected_parts: Vec<&str> = expected_token.split('.').collect();
    
    if expected_parts[1] == signature {
        Some(user_id)
    } else {
        None
    }
}

pub async fn get_auth_user_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<i64> {
    let auth_header = headers.get("Authorization")?
        .to_str().ok()?
        .strip_prefix("Bearer ")?;
        
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    verify_token(auth_header, &bot_token)
}

pub async fn auth_telegram(
    State(state): State<AppState>,
    Json(payload): Json<TelegramAuthRequest>,
) -> impl IntoResponse {
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    if bot_token.is_empty() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Bot token not configured").into_response();
    }

    if let Some(tg_user) = validate_init_data(&payload.init_data, &bot_token) {
        // Find user by Telegram ID
        let user_res = sqlx::query_as::<_, caramba_db::models::store::User>("SELECT * FROM users WHERE tg_id = ?")
            .bind(tg_user.id)
            .fetch_optional(&state.pool)
            .await;

        let user = match user_res {
            Ok(Some(u)) => u,
            Ok(None) => {
                return (StatusCode::FORBIDDEN, "User not found. Please start the bot first.").into_response();
            },
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        };

        // Create simple signed token using bot_token as secret
        let token = create_token(user.id, &bot_token);
        
        let active_subs = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM subscriptions WHERE user_id = ? AND status = 'active'")
            .bind(user.id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

        (StatusCode::OK, Json(AuthResponse {
            token,
            user: UserInfo {
                id: user.id,
                username: user.username.clone().unwrap_or_default(),
                active_subscriptions: active_subs as usize,
                balance: user.balance as f64 / 100.0,
            }
        })).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "Invalid initData").into_response()
    }
}

pub async fn get_user_stats(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let user_id = match get_auth_user_id(&state, &headers).await {
        Some(uid) => uid,
        None => return (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    };
    
    // Calculate aggregate stats from all subscriptions
    // In a real app, we might want to show specific subscription stats, 
    // but for the home widget, aggregate is good.
    
    #[derive(sqlx::FromRow)]
    struct SubStats {
        upload: i64,
        download: i64,
        traffic_limit: i64,
        expire: i64,
    }
    
    let stats: Vec<SubStats> = sqlx::query_as("SELECT upload, download, traffic_limit, expire FROM subscriptions WHERE user_id = ? AND status = 'active'")
        .bind(user_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        
    let mut total_upload = 0;
    let mut total_download = 0;
    let mut total_limit = 0;
    let mut max_expire = 0;
    
    for s in stats {
        total_upload += s.upload;
        total_download += s.download;
        total_limit += s.traffic_limit;
        if s.expire > max_expire {
            max_expire = s.expire;
        }
    }
    
    let days_left = if max_expire > 0 {
        let now = chrono::Utc::now().timestamp();
        if max_expire > now {
            (max_expire - now) / 86400
        } else {
            0
        }
    } else {
        0
    };

    (StatusCode::OK, Json(UserStatsResponse {
        traffic_used: total_upload + total_download,
        traffic_limit: total_limit,
        days_left,
        plan_name: "Standard".to_string(), // TODO: Fetch from plan
        total_download,
        total_upload,
    })).into_response()
}

#[derive(Serialize)]
pub struct SubInfo {
    pub id: i64,
    pub uuid: String,
    pub status: String,
    pub plan_name: String,
    pub traffic_limit_gb: Option<i64>,
    pub created_at: i64,
    pub expires_at: i64,
}

pub async fn get_user_subscriptions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let user_id = match get_auth_user_id(&state, &headers).await {
        Some(uid) => uid,
        None => return (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    };
    
    // Fetch active subscriptions
    #[derive(sqlx::FromRow)]
    struct SubRow {
        id: i64,
        uuid: String,
        status: String,
        traffic_limit: i64,
        created_at: i64,
        expire: i64,
        plan_id: i64,
    }
    
    let subs: Vec<SubRow> = sqlx::query_as("SELECT id, uuid, status, traffic_limit, created_at, expire, plan_id FROM subscriptions WHERE user_id = ? AND status = 'active'")
        .bind(user_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        
    let mut response_subs = Vec::new();
    
    for s in subs {
        let plan_name: String = sqlx::query_scalar("SELECT name FROM plans WHERE id = ?")
            .bind(s.plan_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or("Unknown Plan".to_string());
            
        response_subs.push(SubInfo {
            id: s.id,
            uuid: s.uuid,
            status: s.status,
            plan_name,
            traffic_limit_gb: if s.traffic_limit > 0 { Some(s.traffic_limit / 1024 / 1024 / 1024) } else { None },
            created_at: s.created_at,
            expires_at: s.expire,
        });
    }

    (StatusCode::OK, Json(response_subs)).into_response()
}

#[derive(Serialize)]
pub struct ClientNode {
    pub id: i64,
    pub name: String,
    pub country_code: String,
    pub flag: String,
    pub status: String,
}

pub async fn get_client_nodes(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    match get_auth_user_id(&state, &headers).await {
        Some(_) => {},
        None => return (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    };
    
    // Fetch active nodes
    let nodes: Vec<caramba_db::models::node::Node> = sqlx::query_as("SELECT * FROM nodes WHERE is_enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
     
    let client_nodes: Vec<ClientNode> = nodes.into_iter().map(|n| {
        let cc = n.country_code.unwrap_or("UN".to_string());
        let flag = country_code_to_flag(&cc);
        ClientNode {
            id: n.id,
            name: n.name,
            country_code: cc,
            flag,
            status: n.status,
        }
    }).collect();

    (StatusCode::OK, Json(client_nodes)).into_response()
}

fn country_code_to_flag(country_code: &str) -> String {
    let country_code = country_code.to_uppercase();
    if country_code.len() != 2 {
        return "🌐".to_string();
    }
    
    let mut flag = String::new();
    for c in country_code.chars() {
        if let Some(offset) = (c as u32).checked_sub('A' as u32) {
            if let Some(c) = std::char::from_u32(0x1F1E6 + offset) {
                flag.push(c);
            }
        }
    }
    
    if flag.is_empty() { "🌐".to_string() } else { flag }
}

#[derive(Serialize)]
pub struct PaymentHistoryItem {
    pub id: i64,
    pub amount: f64,
    pub method: String,
    pub status: String,
    pub created_at: i64,
}

pub async fn get_user_payments(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let user_id = match get_auth_user_id(&state, &headers).await {
        Some(uid) => uid,
        None => return (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    };
    
    // Fetch payments
    let payments: Vec<caramba_db::models::store::Payment> = sqlx::query_as("SELECT * FROM payments WHERE user_id = ? ORDER BY created_at DESC LIMIT 50")
        .bind(user_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        
    let history: Vec<PaymentHistoryItem> = payments.into_iter().map(|p| {
        PaymentHistoryItem {
            id: p.id,
            amount: p.amount as f64 / 100.0,
            method: p.method,
            status: p.status,
            created_at: p.created_at.timestamp(),
        }
    }).collect();

    (StatusCode::OK, Json(history)).into_response()
}

#[derive(Serialize)]
pub struct ReferralStats {
    pub referral_code: String,
    pub referred_count: i64,
    pub referral_link: String,
}

pub async fn get_user_referrals(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let user_id = match get_auth_user_id(&state, &headers).await {
        Some(uid) => uid,
        None => return (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    };
    
    // Get user to check code
    let user = match sqlx::query_as::<_, caramba_db::models::store::User>("SELECT * FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(u)) => u,
        _ => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };
    
    // Generate code if missing
    let code = if let Some(c) = user.referral_code {
        c
    } else {
        let new_code = format!("REF-{}", user_id); // Simple code for now
        let _ = sqlx::query("UPDATE users SET referral_code = ? WHERE id = ?")
            .bind(&new_code)
            .bind(user_id)
            .execute(&state.pool)
            .await;
        new_code
    };
    
    // Count referrals
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE referrer_id = ?")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        
    let bot_username = state.settings.get_or_default("bot_username", "UnknownBot").await;
    let link = format!("https://t.me/{}?start={}", bot_username, code);

    (StatusCode::OK, Json(ReferralStats {
        referral_code: code,
        referred_count: count,
        referral_link: link,
    })).into_response()
}
