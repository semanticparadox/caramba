use axum::{
    routing::{post, get},
    Router,
    response::{IntoResponse, Json},
    extract::{State, Request, Path},
    http::{StatusCode, header},
    middleware::{self, Next},
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use sqlx::Row;
use std::env;


#[derive(Deserialize)]
pub struct InitDataRequest {
    /// Accept both "initData" (from AuthProvider.tsx) and "init_data" (from AuthContext.tsx)
    #[serde(alias = "initData", alias = "init_data")]
    pub init_data: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: AuthUserInfo,
}

#[derive(Serialize)]
pub struct AuthUserInfo {
    pub id: i64,
    pub username: String,
    pub active_subscriptions: i64,
    pub balance: f64,
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
        .route("/user/subscriptions", get(get_user_subscriptions).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/user/subscription", get(get_user_subscriptions).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/user/payments", get(get_user_payments).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/user/profile", get(get_user_profile).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/user/referrals", get(get_user_referrals).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/referrals", get(get_user_referrals).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/plans", get(get_plans).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/leaderboard", get(get_leaderboard).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/servers", get(get_active_servers).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/nodes", get(get_active_servers).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        // Store endpoints
        .route("/store/categories", get(get_store_categories).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/store/products/{category_id}", get(get_store_products).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/store/cart", get(get_cart).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/store/cart/add", post(add_to_cart).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/store/checkout", post(checkout_cart).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        // Purchase
        .route("/plans/purchase", post(purchase_plan).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
        .route("/subscription/{id}/server", post(pin_subscription_node).layer(middleware::from_fn_with_state(state.clone(), auth_middleware)))
}

async fn auth_telegram(
    State(state): State<AppState>,
    Json(payload): Json<InitDataRequest>,
) ->  impl IntoResponse {
    tracing::info!("Received auth request");

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

    // 2. Validate Signature — get bot_token from settings DB
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    if bot_token.is_empty() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Bot token not configured").into_response();
    }
    
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

    // 4. Look up user by tg_id
    let user_row = sqlx::query("SELECT id, username, balance FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    if user_row.is_none() {
        return (StatusCode::FORBIDDEN, "User not found. Start the bot first.").into_response();
    }
    let user_row = user_row.unwrap();
    let user_id: i64 = user_row.get("id");
    let username: String = user_row.try_get("username").unwrap_or_default();
    let balance: i64 = user_row.try_get("balance").unwrap_or(0);

    // Count active subscriptions
    let active_subs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions WHERE user_id = ? AND status = 'active'")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

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

    Json(AuthResponse {
        token,
        user: AuthUserInfo {
            id: user_id,
            username,
            active_subscriptions: active_subs,
            balance: balance as f64 / 100.0,
        }
    }).into_response()
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
            CAST(p.traffic_limit_gb AS INTEGER) * 1073741824 as total_traffic,
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

// Subscriptions Endpoint — returns ALL user subscriptions with full details
async fn get_user_subscriptions(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Get user_id from tg_id
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Use store_service for consistency with bot
    let subs = match state.store_service.get_user_subscriptions(user_id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to fetch subscriptions: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch subscriptions").into_response();
        }
    };

    // Build subscription URL base
    let sub_domain = state.settings.get_or_default("subscription_domain", "").await;
    let base_domain = if !sub_domain.is_empty() {
        sub_domain
    } else {
        let panel = state.settings.get_or_default("panel_url", "").await;
        if !panel.is_empty() { panel } else { env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string()) }
    };
    let base_url = if base_domain.starts_with("http") {
        base_domain
    } else {
        format!("https://{}", base_domain)
    };

    // Map to JSON-friendly format
    let result: Vec<serde_json::Value> = subs.iter().map(|s| {
        let used_gb = s.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
        let traffic_limit_gb = s.traffic_limit_gb.unwrap_or(0);
        let sub_url = format!("{}/sub/{}", base_url, s.sub.subscription_uuid);
        let days_left = (s.sub.expires_at - chrono::Utc::now()).num_days().max(0);
        let duration_days = (s.sub.expires_at - s.sub.created_at).num_days();

        serde_json::json!({
            "id": s.sub.id,
            "plan_name": s.plan_name,
            "plan_description": s.plan_description,
            "status": s.sub.status,
            "used_traffic_bytes": s.sub.used_traffic,
            "used_traffic_gb": format!("{:.2}", used_gb),
            "traffic_limit_gb": traffic_limit_gb,
            "expires_at": s.sub.expires_at.to_rfc3339(),
            "created_at": s.sub.created_at.to_rfc3339(),
            "days_left": days_left,
            "duration_days": duration_days,
            "note": s.sub.note,
            "auto_renew": s.sub.auto_renew.unwrap_or(false),
            "is_trial": s.sub.is_trial.unwrap_or(false),
            "subscription_uuid": s.sub.subscription_uuid,
            "subscription_url": sub_url,
        })
    }).collect();

    Json(result).into_response()
}

// User Profile Endpoint
async fn get_user_profile(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let row = sqlx::query("SELECT id, username, tg_id, balance, referral_code FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    if let Some(r) = row {
        let balance: i64 = r.try_get("balance").unwrap_or(0);
        let referral_code: String = r.try_get("referral_code").unwrap_or_default();

        // Count active + pending subs
        let user_id: i64 = r.get("id");
        let active_subs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions WHERE user_id = ? AND status = 'active'")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
        let pending_subs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions WHERE user_id = ? AND status = 'pending'")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

        Json(serde_json::json!({
            "id": user_id,
            "tg_id": tg_id,
            "username": r.try_get::<String, _>("username").unwrap_or_default(),
            "balance": balance as f64 / 100.0,
            "referral_code": referral_code,
            "active_subscriptions": active_subs,
            "pending_subscriptions": pending_subs,
        })).into_response()
    } else {
        (StatusCode::NOT_FOUND, "User not found").into_response()
    }
}

// Plans Endpoint — list available plans
async fn get_plans(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    match state.store_service.get_active_plans().await {
        Ok(plans) => {
            let result: Vec<serde_json::Value> = plans.iter().map(|p| {
                let durations: Vec<serde_json::Value> = p.durations.iter().map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "duration_days": d.duration_days,
                        "price": d.price as f64 / 100.0,
                        "price_cents": d.price,
                    })
                }).collect();

                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "traffic_limit_gb": p.traffic_limit_gb,
                    "device_limit": p.device_limit,
                    "is_trial": p.is_trial.unwrap_or(false),
                    "durations": durations,
                })
            }).collect();
            Json(result).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch plans: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch plans").into_response()
        }
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

async fn get_client_coordinates(state: AppState, ip: String) -> Option<(f64, f64)> {
    match state.geo_service.get_location(&ip).await {
        Some(data) => Some((data.lat, data.lon)),
        None => None,
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
     axum::Extension(claims): axum::Extension<Claims>,
     headers: axum::http::HeaderMap,
     axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    let user_id = claims.sub.parse::<i64>().unwrap_or(0);
    
    // (Refactored Phase 1.8: Use Plan Groups)
    let nodes: Vec<crate::models::node::Node> = state.store_service.get_user_nodes(user_id)
        .await
        .unwrap_or_default();
    
    // 1. Get Client IP/Location
    let client_ip = headers
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    let user_coords = get_client_coordinates(state.clone(), client_ip).await;
    
    // 2. Map to ClientNode & Calculate Distance & Load Score
    let mut client_nodes: Vec<ClientNode> = nodes.into_iter()
        .filter(|n| {
            let users_ok = n.max_users == 0 || (n.max_users > 0 && n.config_block_ads); 
            let load_ok = n.last_cpu.unwrap_or(0.0) < 95.0 && n.last_ram.unwrap_or(0.0) < 98.0;
            users_ok && load_ok
        })
        .map(|n| {
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

        // Calculate Status Label based on Load
        let mut status_label = n.status.clone();
        let cpu = n.last_cpu.unwrap_or(0.0);
        let speed = n.current_speed_mbps;
        
        if cpu > 80.0 {
            status_label = "busy".to_string();
        } else if speed > 500 {
             status_label = "fast".to_string(); // fast badge
        }

        ClientNode {
            id: n.id,
            country_code: n.country_code.clone(),
            flag: get_flag(n.country_code.as_deref().unwrap_or("US")),
            latency: n.last_latency.map(|l| l as i32), // Use last reported latency
            status: status_label,
            distance_km: dist,
            name: format!("Node #{} ({} Mbps)", n.id, speed), 
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
        use crate::services::referral_service::ReferralService;
        let count = ReferralService::get_referral_count(&state.pool, user_id).await.unwrap_or(0);

        let bot_username = "caramba_bot"; // TODO: Config
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
        Ok(leaderboard) => Json::<Vec<crate::services::referral_service::LeaderboardDisplayEntry>>(leaderboard).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch leaderboard: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch leaderboard").into_response()
        }
    }
}

// ============================================================
// Store Endpoints
// ============================================================

async fn get_store_categories(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    match state.catalog_service.get_categories().await {
        Ok(cats) => Json::<Vec<crate::models::store::StoreCategory>>(cats).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch categories: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch categories").into_response()
        }
    }
}

async fn get_store_products(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<Claims>,
    Path(category_id): Path<i64>,
) -> impl IntoResponse {
    match state.catalog_service.get_products_by_category(category_id).await {
        Ok(products) => {
            let result: Vec<serde_json::Value> = products.iter().map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "price": p.price as f64 / 100.0,
                    "price_raw": p.price,
                    "product_type": p.product_type,
                })
            }).collect();
            Json(result).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch products: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch products").into_response()
        }
    }
}

#[derive(Deserialize)]
struct AddToCartReq {
    product_id: i64,
    quantity: Option<i64>,
}

async fn add_to_cart(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<AddToCartReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.catalog_service.add_to_cart(user_id, body.product_id, body.quantity.unwrap_or(1)).await {
        Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("Failed to add to cart: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {}", e)).into_response()
        }
    }
}

async fn get_cart(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.catalog_service.get_user_cart(user_id).await {
        Ok(items) => {
            let result: Vec<serde_json::Value> = items.iter().map(|i| {
                serde_json::json!({
                    "id": i.id,
                    "product_id": i.product_id,
                    "product_name": i.product_name,
                    "quantity": i.quantity,
                    "price": i.price as f64 / 100.0,
                    "price_raw": i.price,
                    "total": (i.price * i.quantity) as f64 / 100.0,
                })
            }).collect();
            Json(result).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch cart: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch cart").into_response()
        }
    }
}

async fn checkout_cart(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.catalog_service.checkout_cart(user_id).await {
        Ok(order_id) => Json(serde_json::json!({"ok": true, "order_id": order_id})).into_response(),
        Err(e) => {
            (StatusCode::BAD_REQUEST, format!("{}", e)).into_response()
        }
    }
}

// ============================================================
// Purchase Plan Endpoint
// ============================================================

#[derive(Deserialize)]
struct PurchaseReq {
    duration_id: i64,
}

async fn purchase_plan(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<PurchaseReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.store_service.purchase_plan(user_id, body.duration_id).await {
        Ok(sub) => {
            Json(serde_json::json!({
                "ok": true,
                "subscription_id": sub.id,
                "status": sub.status,
                "message": "Purchase successful! Your subscription is now pending."
            })).into_response()
        }
        Err(e) => {
            tracing::error!("Purchase failed for user {}: {}", user_id, e);
            (StatusCode::BAD_REQUEST, format!("{}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
struct PinNodeReq {
    node_id: i64,
}

async fn pin_subscription_node(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
    Json(body): Json<PinNodeReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = ?")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Verify ownership
    let sub_owner_id: Option<i64> = sqlx::query_scalar("SELECT user_id FROM subscriptions WHERE id = ?")
        .bind(sub_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    match sub_owner_id {
        Some(owner_id) if owner_id == user_id => {
            // Update
            match state.store_service.update_subscription_node(sub_id, Some(body.node_id)).await {
                Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
                Err(e) => {
                    tracing::error!("Failed to pin node: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to update subscription").into_response()
                }
            }
        }
        _ => (StatusCode::FORBIDDEN, "Subscription not found or access denied").into_response(),
    }
}
