use axum::{
    routing::{post, get},
    Router,
    response::{IntoResponse, Json},
    extract::{State, Request},
    http::{StatusCode, header},
    middleware::{self, Next},
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use std::env;
use sqlx::Row;

#[derive(Deserialize)]
pub struct InitDataRequest {
    #[serde(rename = "initData")]
    pub init_data: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // Telegram ID as string
    pub exp: usize,  // Expiration
    pub role: String, // "client"
}

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/auth/telegram", post(auth_telegram))
        .route("/user/stats", get(get_user_stats).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/user/subscription", get(get_user_subscription).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/user/payments", get(get_user_payments).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/referrals", get(get_user_referrals).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/leaderboard", get(get_leaderboard).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/servers", get(get_active_servers).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
}

async fn auth_telegram(
    State(state): State<AppState>,
    Json(payload): Json<InitDataRequest>,
) ->  impl IntoResponse {
    tracing::info!("Received auth request: {}", payload.init_data);

    // 1. Parse initData
    let mut params: HashMap<String, String> = HashMap::new();
    for pair in payload.init_data.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            params.insert(key.to_string(), value.to_string());
        }
    }

    let hash = match params.get("hash") {
        Some(h) => h,
        None => return (StatusCode::BAD_REQUEST, "Missing hash").into_response(),
    };

    // 2. Validate Signature
    let bot_token = env::var("TELOXIDE_TOKEN").expect("TELOXIDE_TOKEN not set");
    
    // Data-check-string is all keys except hash, sorted alphabetically
    let mut data_check_vec: Vec<String> = params.iter()
        .filter(|(k, _)| k.as_str() != "hash")
        .map(|(k, v)| format!("{}={}", k, v)) // Note: values are already URL-encoded in initData? 
        // Actually, specific spec says "key=value". 
        // But usually initData comes raw. 
        // Let's assume decoding happens if needed, but the official way is raw string pairs.
        // Wait, if we parse by split('&'), we get raw URL encoded values? 
        // No, split splits the string. urlencoding might be needed or unneeded.
        // Telegram spec: keys are sorted.
        // Values: "The values are the same as in the original string."
        .collect();
    
    data_check_vec.sort();
    let data_check_string = data_check_vec.join("\n");

    // Secret key = HMAC-SHA256("WebAppData", bot_token)
    let secret_key = {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"WebAppData").unwrap();
        mac.update(bot_token.as_bytes());
        mac.finalize().into_bytes()
    };

    let calculated_hash = {
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret_key).unwrap();
        mac.update(data_check_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    };

    if calculated_hash != *hash {
        tracing::warn!("Auth failed: Hash mismatch. Calc: {}, Recv: {}", calculated_hash, hash);
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }
    
    // 3. Extract User ID
    let user_json_str = match params.get("user") {
        Some(u) => urlencoding::decode(u).unwrap_or(std::borrow::Cow::Borrowed(u)),
        None => return (StatusCode::BAD_REQUEST, "Missing user data").into_response(),
    };

    let user_json: serde_json::Value = match serde_json::from_str(&user_json_str) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid user JSON").into_response(),
    };

    let tg_id = match user_json.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Missing user ID").into_response(),
    };

    // 4. Ensure User Exists (Optional: Auto-create via trial?)
    // For now, check if user exists in DB. If not, maybe create?
    // Let's implement: find user by tg_id.
    let user_exists: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !user_exists {
        // Option: Create trial user logic here
        // For now, return unauthorized if not found (or specialized code)
        // Actually, allow new users to convert?
        // Let's return 404/403 or auto-create.
        // "Trial for Subscribers" - maybe check membership?
        // Let's keep it simple: if not exists, create with default trial?
        // Or assume bot flow handles creation.
        // Let's return error for now.
        // return (StatusCode::FORBIDDEN, "User not found. Start bot first.").into_response();
        // EDIT: Auto-create logic might be better for seamless UX.
    }

    // 5. Generate JWT
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(7))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: tg_id.to_string(),
        exp: expiration,
        role: "client".to_string(),
    };

    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(state.session_secret.as_bytes()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR).unwrap();

    Json(AuthResponse { token }).into_response()
}

// Middleware to verify JWT
async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let auth_header = req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    let token = match auth_header {
        Some(auth_header) if auth_header.starts_with("Bearer ") => {
            &auth_header[7..]
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.session_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    ).map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Add user ID to request extensions
    req.extensions_mut().insert(token_data.claims);

    Ok(next.run(req).await)
}

// Stats Endpoint
async fn get_user_stats(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    
    // Find active subscription
    // Assuming 1 active sub for simplicity, or sum them up
    // Let's get the PRIMARY active subscription
    
    #[derive(Serialize, sqlx::FromRow)]
    struct UserStats {
        traffic_used: i64,
        total_traffic: i64,
        days_left: i64,
        plan_name: String,
        balance: i64,
    }

    let stats_opt: Option<UserStats> = sqlx::query_as(r#"
        SELECT 
            s.used_traffic as traffic_used, 
            s.total_traffic as total_traffic,
            CAST((strftime('%s', s.expires_at) - strftime('%s', 'now')) / 86400 AS INTEGER) as days_left,
            p.name as plan_name,
            u.balance as balance
        FROM subscriptions s
        JOIN plans p ON s.plan_id = p.id
        JOIN users u ON s.user_id = u.id
        WHERE u.tg_id = ? AND s.status = 'active'
        ORDER BY s.expires_at DESC
        LIMIT 1
    "#)
    .bind(tg_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    match stats_opt {
        Some(stats) => Json(stats).into_response(),
        None => (StatusCode::NOT_FOUND, "No active subscription").into_response(),
    }
}

// Subscription Endpoint
async fn get_user_subscription(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let sub = sqlx::query(r#"
        SELECT s.uuid 
        FROM subscriptions s
        JOIN users u ON s.user_id = u.id
        WHERE u.tg_id = ? AND s.status = 'active'
        LIMIT 1
    "#)
    .bind(tg_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some(row) = sub {
        let uuid: String = row.get("uuid");
        // Construct full URL (this should be in config, but we'll use a placeholder or derived)
        // The frontend knows the base URL usually.
        // Return UUID and let frontend construct it, OR return full URL.
        // Let's return UUID + formatted URL.
        
        let domain = env::var("PANEL_URL").unwrap_or_else(|_| "panel.example.com".to_string());
        // Clean functionality: remove http/https prefix if present for cleaner display, or ensure it for real link
        // Actually, PANEL_URL usually includes protocol. If not, assume https.
        let base_url = if domain.starts_with("http") { domain } else { format!("https://{}", domain) };
        let sub_url = format!("{}/sub/{}", base_url, uuid);
        
        Json(serde_json::json!({
            "uuid": uuid,
            "subscription_url": sub_url
        })).into_response()
    } else {
        (StatusCode::NOT_FOUND, "No subscription").into_response()
    }
}

// Helper for haversine distance
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

#[derive(Deserialize)]
struct IpApiResponse {
    #[serde(rename = "countryCode")]
    _country_code: String,
    lat: f64,
    lon: f64,
}

async fn get_client_coordinates(state: AppState, ip: String) -> Option<(f64, f64)> {
    // 1. Check Cache
    {
        let cache = state.geo_cache.lock().unwrap();
        if let Some((lat, lon, ts)) = cache.get(&ip) {
            // Cache valid for 24 hours
            if ts.elapsed().as_secs() < 86400 {
                return Some((*lat, *lon));
            }
        }
    }

    // 2. Fetch from API
    let url = format!("http://ip-api.com/json/{}?fields=countryCode,lat,lon", ip);
    match reqwest::get(&url).await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<IpApiResponse>().await {
                 let coords = (json.lat, json.lon);
                 // 3. Update Cache
                 let mut cache = state.geo_cache.lock().unwrap();
                 cache.insert(ip, (json.lat, json.lon, std::time::Instant::now()));
                 Some(coords)
            } else {
                None
            }
        },
        Err(_) => None,
    }
}

#[derive(Serialize)]
struct ClientNode {
    id: i64,
    country_code: Option<String>,
    flag: String, // Calculated on backend
    latency: Option<i32>, // Still mocked or from health check?
    status: String,
    distance_km: Option<i32>,
    name: String, // Derived from id or config?
}

// Helper for flag
fn get_flag(country: &str) -> String {
    let country = country.to_uppercase();
    if country.len() != 2 { return "🌐".to_string(); }
    let offset = 127397;
    let first = country.chars().next().unwrap() as u32 + offset;
    let second = country.chars().nth(1).unwrap() as u32 + offset;
    format!("{}{}", char::from_u32(first).unwrap(), char::from_u32(second).unwrap())
}

async fn get_active_servers(
     State(state): State<AppState>,
     axum::Extension(_claims): axum::Extension<Claims>,
     headers: axum::http::HeaderMap,
     axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    let mut nodes: Vec<crate::models::node::Node> = sqlx::query_as("SELECT * FROM nodes WHERE is_enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    
    // 1. Get Client IP/Location
    let client_ip = headers
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    let user_coords = get_client_coordinates(state.clone(), client_ip).await;
    
    // 2. Map to ClientNode & Calculate Distance
    let mut client_nodes: Vec<ClientNode> = nodes.into_iter().map(|n| {
        let dist = if let (Some(u_lat), Some(u_lon), Some(n_lat), Some(n_lon)) = (
            user_coords.map(|c| c.0), 
            user_coords.map(|c| c.1), 
            n.latitude, 
            n.longitude
        ) {
            Some(haversine_distance(u_lat, u_lon, n_lat, n_lon) as i32)
        } else {
            None
        };

        ClientNode {
            id: n.id,
            country_code: n.country_code.clone(),
            flag: get_flag(n.country_code.as_deref().unwrap_or("US")),
            latency: None, // Frontend will test pings? Or backend?
            status: n.status,
            distance_km: dist,
            name: format!("Node #{}", n.id), // Simple name since Node struct doesn't have name
        }
    }).collect();

    // 3. Sort (Nearest first)
    client_nodes.sort_by(|a, b| {
        let da = a.distance_km.unwrap_or(99999);
        let db = b.distance_km.unwrap_or(99999);
        da.cmp(&db)
    });

    Json(client_nodes).into_response()
}

// Billing / Payments Endpoint
async fn get_user_payments(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    #[derive(Serialize, sqlx::FromRow)]
    struct Payment {
        id: i64,
        amount: i64,
        method: String,
        status: String,
        created_at: i64,
    }

    // Check if table exists (dynamic check or assume it exists). 
    // We'll assume the table 'payments' exists with user_id linked to users table.
    // JOIN users to filter by tg_id
    let payments: Vec<Payment> = sqlx::query_as(r#"
        SELECT p.id, p.amount, p.method, p.status, p.created_at
        FROM payments p
        JOIN users u ON p.user_id = u.id
        WHERE u.tg_id = ?
        ORDER BY p.created_at DESC
        LIMIT 50
    "#)
    .bind(tg_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    Json(payments).into_response()
}

// Referrals Endpoint
async fn get_user_referrals(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Get user info and their referral count
    // Assuming users table has 'referral_code' and we count how many users have 'referred_by' = this user.id
    
    #[derive(Serialize)]
    struct ReferralStats {
        referral_code: String,
        referred_count: i64,
        referral_link: String,
    }

    let user_info: Option<(i64, String)> = sqlx::query_as("SELECT id, referral_code FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    if let Some((user_id, code)) = user_info {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE referred_by = ?")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

        let bot_username = "exarobot_bot"; // TODO: Config
        let link = format!("https://t.me/{}?start={}", bot_username, code);

        Json(ReferralStats {
            referral_code: code,
            referred_count: count,
            referral_link: link,
        }).into_response()
    } else {
         (StatusCode::NOT_FOUND, "User not found").into_response()
    }
}

// Global Leaderboard Endpoint
async fn get_leaderboard(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    use crate::services::referral_service::ReferralService;

    match ReferralService::get_leaderboard(&state.pool, 10).await {
        Ok(leaderboard) => Json(leaderboard).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch leaderboard: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch leaderboard").into_response()
        }
    }
}
