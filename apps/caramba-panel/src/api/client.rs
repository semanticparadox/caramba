use crate::AppState;
use axum::{
    Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{delete, get, post},
};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::Row;
use std::collections::HashMap;
use std::env;
use tracing::warn;

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
    pub sub: String,  // Telegram ID as string
    pub exp: usize,   // Expiration
    pub role: String, // "client"
}

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/auth/telegram", post(auth_telegram))
        .route(
            "/user/stats",
            get(get_user_stats).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/subscriptions",
            get(get_user_subscriptions).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/subscription",
            get(get_user_subscriptions).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/payments",
            get(get_user_payments).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/profile",
            get(get_user_profile).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/referrals",
            get(get_user_referrals).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/referrals",
            get(get_user_referrals).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/plans",
            get(get_plans).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/leaderboard",
            get(get_leaderboard).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/servers",
            get(get_active_servers).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/nodes",
            get(get_active_servers).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        // Store endpoints
        .route(
            "/store/categories",
            get(get_store_categories).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/store/products/{category_id}",
            get(get_store_products).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/store/cart",
            get(get_cart).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/store/cart/add",
            post(add_to_cart).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/store/checkout",
            post(checkout_cart).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        // Purchase
        .route(
            "/plans/purchase",
            post(purchase_plan).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{id}/server",
            post(pin_subscription_node).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{id}/activate",
            post(activate_subscription).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{id}/gift",
            post(convert_subscription_to_gift).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{id}/links",
            get(get_subscription_links_for_user).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/promo/redeem",
            post(redeem_promo_code).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/promo/my-codes",
            get(get_my_gift_codes).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/promo/my-codes/{id}",
            delete(revoke_my_gift_code).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/referrer",
            post(set_referrer_code).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
}

async fn auth_telegram(
    State(state): State<AppState>,
    Json(payload): Json<InitDataRequest>,
) -> impl IntoResponse {
    tracing::info!("Received auth request");

    // 1. Parse initData
    let mut params: HashMap<String, String> = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(payload.init_data.as_bytes()) {
        params.insert(key.into_owned(), value.into_owned());
    }

    let hash = match params.get("hash") {
        Some(h) => h,
        None => return (StatusCode::BAD_REQUEST, "Missing hash").into_response(),
    };

    // 2. Validate Signature — get bot_token from settings DB
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    if bot_token.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Bot token not configured",
        )
            .into_response();
    }

    // Data-check-string is all keys except hash, sorted alphabetically
    let mut data_check_vec: Vec<String> = params
        .iter()
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
        tracing::warn!(
            "Auth failed: Hash mismatch. Calc: {}, Recv: {}",
            calculated_hash,
            hash
        );
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    // 3. Extract User ID
    let user_json_str = match params.get("user") {
        Some(u) => u,
        None => return (StatusCode::BAD_REQUEST, "Missing user data").into_response(),
    };

    let user_json: serde_json::Value = match serde_json::from_str(user_json_str) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid user JSON").into_response(),
    };

    let tg_id = match user_json.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Missing user ID").into_response(),
    };

    // 4. Look up user by tg_id
    let user_row = sqlx::query("SELECT id, username, balance FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    if user_row.is_none() {
        return (
            StatusCode::FORBIDDEN,
            "User not found. Start the bot first.",
        )
            .into_response();
    }
    let user_row = user_row.unwrap();
    let user_id: i64 = user_row.get("id");
    let username: String = user_row.try_get("username").unwrap_or_default();
    let balance: i64 = user_row.try_get("balance").unwrap_or(0);

    // Count active subscriptions
    let active_subs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscriptions WHERE user_id = $1 AND status = 'active'",
    )
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

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.session_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    .unwrap();

    Json(AuthResponse {
        token,
        user: AuthUserInfo {
            id: user_id,
            username,
            active_subscriptions: active_subs,
            balance: balance as f64 / 100.0,
        },
    })
    .into_response()
}

// Middleware to verify JWT
async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    let token = match auth_header {
        Some(auth_header) if auth_header.starts_with("Bearer ") => &auth_header[7..],
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.session_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

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
            CAST(p.traffic_limit_gb AS BIGINT) * 1073741824 as total_traffic,
            GREATEST(
                0,
                (EXTRACT(EPOCH FROM (COALESCE(s.expires_at, CURRENT_TIMESTAMP) - CURRENT_TIMESTAMP)) / 86400)::BIGINT
            ) as days_left,
            p.name as plan_name,
            u.balance as balance
        FROM subscriptions s
        JOIN plans p ON s.plan_id = p.id
        JOIN users u ON s.user_id = u.id
        WHERE u.tg_id = $1 AND s.status = 'active'
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
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Use subscription_service
    let subs: Vec<caramba_db::models::store::SubscriptionWithDetails> = match state
        .subscription_service
        .get_user_subscriptions(user_id)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to fetch subscriptions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch subscriptions",
            )
                .into_response();
        }
    };

    let base_url = resolve_subscription_base_url(&state).await;

    let sub_ids: Vec<i64> = subs.iter().map(|s| s.sub.id).collect();
    let mut active_devices_by_sub: HashMap<i64, i64> = HashMap::new();
    if !sub_ids.is_empty() {
        let rows = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT
                sip.subscription_id,
                COUNT(DISTINCT sip.client_ip)::BIGINT AS active_devices
            FROM subscription_ip_tracking sip
            WHERE sip.subscription_id = ANY($1)
              AND sip.last_seen_at > CURRENT_TIMESTAMP - interval '15 minutes'
              AND sip.client_ip <> '0.0.0.0'
              AND NOT EXISTS (SELECT 1 FROM nodes n WHERE n.ip = sip.client_ip)
            GROUP BY sip.subscription_id
            "#,
        )
        .bind(&sub_ids)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        for (sub_id, active_devices) in rows {
            active_devices_by_sub.insert(sub_id, active_devices);
        }
    }

    let plan_ids: Vec<i64> = subs.iter().map(|s| s.sub.plan_id).collect();
    let mut device_limits_by_plan: HashMap<i64, i64> = HashMap::new();
    if !plan_ids.is_empty() {
        let rows = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, COALESCE(device_limit, 0)::BIGINT FROM plans WHERE id = ANY($1)",
        )
        .bind(&plan_ids)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        for (plan_id, device_limit) in rows {
            device_limits_by_plan.insert(plan_id, device_limit);
        }
    }

    let node_ids: Vec<i64> = subs.iter().filter_map(|s| s.sub.node_id).collect();
    let mut node_by_id: HashMap<i64, (String, Option<String>)> = HashMap::new();
    if !node_ids.is_empty() {
        let rows = sqlx::query_as::<_, (i64, String, Option<String>)>(
            "SELECT id, name, flag FROM nodes WHERE id = ANY($1)",
        )
        .bind(&node_ids)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        for (node_id, node_name, node_flag) in rows {
            node_by_id.insert(node_id, (node_name, node_flag));
        }
    }

    let mut result: Vec<serde_json::Value> = Vec::with_capacity(subs.len());
    for s in &subs {
        let all_links = match state
            .subscription_service
            .get_subscription_links(s.sub.id)
            .await
        {
            Ok(links) => links,
            Err(err) => {
                warn!(
                    "Failed to build direct links for subscription {}: {}",
                    s.sub.id, err
                );
                Vec::new()
            }
        };
        let vless_links: Vec<String> = all_links
            .into_iter()
            .filter(|link| link.starts_with("vless://"))
            .collect();
        let primary_vless_link = vless_links.first().cloned();
        let used_gb = s.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
        let traffic_limit_gb = s.traffic_limit_gb.unwrap_or(0);
        let sub_url = format!("{}/sub/{}", base_url, s.sub.subscription_uuid);
        let days_left = (s.sub.expires_at - chrono::Utc::now()).num_days().max(0);
        let duration_days = (s.sub.expires_at - s.sub.created_at).num_days();
        let active_devices = active_devices_by_sub.get(&s.sub.id).copied().unwrap_or(0);
        let device_limit = device_limits_by_plan
            .get(&s.sub.plan_id)
            .copied()
            .unwrap_or(0);
        let (last_node_name, last_node_flag) = s
            .sub
            .node_id
            .and_then(|node_id| node_by_id.get(&node_id).cloned())
            .map(|(name, flag)| (Some(name), flag))
            .unwrap_or((None, None));

        result.push(serde_json::json!({
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
            "active_devices": active_devices,
            "device_limit": device_limit,
            "last_node_id": s.sub.node_id,
            "last_node_name": last_node_name,
            "last_node_flag": last_node_flag,
            "last_sub_access": s.sub.last_sub_access.as_ref().map(|dt| dt.to_rfc3339()),
            "subscription_url": sub_url,
            "vless_links": vless_links,
            "primary_vless_link": primary_vless_link,
        }));
    }

    Json(result).into_response()
}

async fn resolve_subscription_base_url(state: &AppState) -> String {
    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let base_domain = if !sub_domain.is_empty() {
        sub_domain
    } else {
        let panel = state.settings.get_or_default("panel_url", "").await;
        if !panel.is_empty() {
            panel
        } else {
            env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
        }
    };

    if base_domain.starts_with("http") {
        base_domain
    } else {
        format!("https://{}", base_domain)
    }
}

// User Profile Endpoint
async fn get_user_profile(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let row = sqlx::query(
        "SELECT id, username, tg_id, balance, referral_code FROM users WHERE tg_id = $1",
    )
    .bind(tg_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some(r) = row {
        let balance: i64 = r.try_get("balance").unwrap_or(0);
        let referral_code: String = r.try_get("referral_code").unwrap_or_default();

        // Count active + pending subs
        let user_id: i64 = r.get("id");
        let active_subs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscriptions WHERE user_id = $1 AND status = 'active'",
        )
        .bind(user_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        let pending_subs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscriptions WHERE user_id = $1 AND status = 'pending'",
        )
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
        }))
        .into_response()
    } else {
        (StatusCode::NOT_FOUND, "User not found").into_response()
    }
}

// Plans Endpoint — list available plans
async fn get_plans(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    match state.catalog_service.get_active_plans().await {
        Ok(plans) => {
            let result: Vec<serde_json::Value> = plans
                .iter()
                .map(|p| {
                    let durations: Vec<serde_json::Value> = p
                        .durations
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "id": d.id,
                                "duration_days": d.duration_days,
                                "price": d.price as f64 / 100.0,
                                "price_cents": d.price,
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "description": p.description,
                        "traffic_limit_gb": p.traffic_limit_gb,
                        "device_limit": p.device_limit,
                        "is_trial": p.is_trial.unwrap_or(false),
                        "durations": durations,
                    })
                })
                .collect();
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
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
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
    flag: String,         // Calculated on backend
    latency: Option<i32>, // Still mocked or from health check?
    status: String,
    distance_km: Option<i32>,
    name: String, // Derived from id or config?
}

// Helper for flag
fn get_flag(country: &str) -> String {
    let country = country.to_uppercase();
    if country.len() != 2 {
        return "🌐".to_string();
    }
    let offset = 127397;
    let first = country.chars().next().unwrap() as u32 + offset;
    let second = country.chars().nth(1).unwrap() as u32 + offset;
    format!(
        "{}{}",
        char::from_u32(first).unwrap(),
        char::from_u32(second).unwrap()
    )
}

async fn get_active_servers(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    headers: axum::http::HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    let tg_id = claims.sub.parse::<i64>().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);
    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // (Refactored Phase 1.8: Use Plan Groups)
    let nodes: Vec<caramba_db::models::node::Node> = state
        .store_service
        .get_user_nodes(user_id)
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
    let mut client_nodes: Vec<ClientNode> = nodes
        .into_iter()
        .filter(|n| {
            let users_ok = n.max_users <= 0 || n.active_connections.unwrap_or(0) < n.max_users;
            let load_ok = n.last_cpu.unwrap_or(0.0) < 95.0 && n.last_ram.unwrap_or(0.0) < 98.0;
            users_ok && load_ok
        })
        .map(|n| {
            let dist = if let (Some(u_lat), Some(u_lon), Some(n_lat), Some(n_lon)) = (
                user_coords.map(|c| c.0),
                user_coords.map(|c| c.1),
                n.latitude,
                n.longitude,
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
        })
        .collect();

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
    let payments: Vec<Payment> = sqlx::query_as(
        r#"
        SELECT p.id, p.amount, p.method, p.status, p.created_at
        FROM payments p
        JOIN users u ON p.user_id = u.id
        WHERE u.tg_id = $1
        ORDER BY p.created_at DESC
        LIMIT 50
    "#,
    )
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
        total_earned_cents: i64,
        total_earned_usd: f64,
        referrals: Vec<ReferralEntry>,
    }

    #[derive(Serialize)]
    struct ReferralEntry {
        id: i64,
        username: Option<String>,
        full_name: Option<String>,
        joined_at: String,
        total_earned_cents: i64,
    }

    let user_info: Option<(i64, String)> =
        sqlx::query_as("SELECT id, referral_code FROM users WHERE tg_id = $1")
            .bind(tg_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    if let Some((user_id, code)) = user_info {
        use crate::services::referral_service::ReferralService;
        let count = ReferralService::get_referral_count(&state.pool, user_id)
            .await
            .unwrap_or(0);
        let total_earned_cents = ReferralService::get_user_referral_earnings(&state.pool, user_id)
            .await
            .unwrap_or(0);
        let referrals_raw = ReferralService::get_user_referrals(&state.pool, user_id)
            .await
            .unwrap_or_default();

        let bot_username = state.settings.get_or_default("bot_username", "").await;
        let bot_username = bot_username.trim().trim_start_matches('@').to_string();
        let link = if bot_username.is_empty() {
            format!("https://t.me/YOUR_BOT_USERNAME?start={}", code)
        } else {
            format!("https://t.me/{}?start={}", bot_username, code)
        };

        let referrals = referrals_raw
            .into_iter()
            .map(|r| ReferralEntry {
                id: r.id,
                username: r.username,
                full_name: r.full_name,
                joined_at: r.created_at.to_rfc3339(),
                total_earned_cents: r.total_earned,
            })
            .collect::<Vec<_>>();

        Json(ReferralStats {
            referral_code: code,
            referred_count: count,
            referral_link: link,
            total_earned_cents,
            total_earned_usd: total_earned_cents as f64 / 100.0,
            referrals,
        })
        .into_response()
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
        Ok(leaderboard) => {
            Json::<Vec<crate::services::referral_service::LeaderboardDisplayEntry>>(leaderboard)
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch leaderboard: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch leaderboard",
            )
                .into_response()
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
        Ok(cats) => Json::<Vec<caramba_db::models::store::StoreCategory>>(cats).into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch categories: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch categories",
            )
                .into_response()
        }
    }
}

async fn get_store_products(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<Claims>,
    Path(category_id): Path<i64>,
) -> impl IntoResponse {
    match state
        .catalog_service
        .get_products_by_category(category_id)
        .await
    {
        Ok(products) => {
            let result: Vec<serde_json::Value> = products
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "description": p.description,
                        "price": p.price as f64 / 100.0,
                        "price_raw": p.price,
                        "product_type": p.product_type,
                    })
                })
                .collect();
            Json(result).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch products: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch products",
            )
                .into_response()
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
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .catalog_service
        .add_to_cart(user_id, body.product_id, body.quantity.unwrap_or(1))
        .await
    {
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
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
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
            let result: Vec<serde_json::Value> = items
                .iter()
                .map(|i| {
                    serde_json::json!({
                        "id": i.id,
                        "product_id": i.product_id,
                        "product_name": i.product_name,
                        "quantity": i.quantity,
                        "price": i.price as f64 / 100.0,
                        "price_raw": i.price,
                        "total": (i.price * i.quantity) as f64 / 100.0,
                    })
                })
                .collect();
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
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
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
        Err(e) => (StatusCode::BAD_REQUEST, format!("{}", e)).into_response(),
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
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .store_service
        .purchase_plan(user_id, body.duration_id)
        .await
    {
        Ok(sub) => Json(serde_json::json!({
            "ok": true,
            "subscription_id": sub.id,
            "status": sub.status,
            "message": "Purchase successful! Your subscription is now pending."
        }))
        .into_response(),
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
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Verify ownership
    let sub_owner_id: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    match sub_owner_id {
        Some(owner_id) if owner_id == user_id => {
            // Update
            match state
                .subscription_service
                .update_subscription_node(sub_id, Some(body.node_id))
                .await
            {
                Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
                Err(e) => {
                    tracing::error!("Failed to pin node: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to update subscription",
                    )
                        .into_response()
                }
            }
        }
        _ => (
            StatusCode::FORBIDDEN,
            "Subscription not found or access denied",
        )
            .into_response(),
    }
}

async fn activate_subscription(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let sub_owner_id: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    match sub_owner_id {
        Some(owner_id) if owner_id == user_id => {}
        _ => {
            return (
                StatusCode::FORBIDDEN,
                "Subscription not found or access denied",
            )
                .into_response();
        }
    }

    match state
        .store_service
        .activate_subscription(sub_id, user_id)
        .await
    {
        Ok(sub) => Json(serde_json::json!({
            "ok": true,
            "subscription_id": sub.id,
            "status": sub.status,
            "message": "Subscription activated",
        }))
        .into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn convert_subscription_to_gift(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .store_service
        .convert_subscription_to_gift(sub_id, user_id)
        .await
    {
        Ok(code) => Json(serde_json::json!({
            "ok": true,
            "code": code,
            "message": "Gift code created from pending subscription",
        }))
        .into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn get_subscription_links_for_user(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let sub_row: Option<(i64, String)> =
        sqlx::query_as("SELECT user_id, subscription_uuid FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    let (_owner_id, subscription_uuid) = match sub_row {
        Some((owner_id, sub_uuid)) if owner_id == user_id => (owner_id, sub_uuid),
        _ => {
            return (
                StatusCode::FORBIDDEN,
                "Subscription not found or access denied",
            )
                .into_response();
        }
    };

    let links = match state
        .subscription_service
        .get_subscription_links(sub_id)
        .await
    {
        Ok(v) => v,
        Err(err) => {
            warn!("Failed to build subscription links for {}: {}", sub_id, err);
            Vec::new()
        }
    };
    let vless_links: Vec<String> = links
        .iter()
        .filter(|link| link.starts_with("vless://"))
        .cloned()
        .collect();
    let base_url = resolve_subscription_base_url(&state).await;

    Json(serde_json::json!({
        "subscription_url": format!("{}/sub/{}", base_url, subscription_uuid),
        "links": links,
        "vless_links": vless_links,
        "primary_vless_link": vless_links.first().cloned(),
    }))
    .into_response()
}

async fn get_my_gift_codes(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<i64>,
            Option<String>,
            Option<i32>,
            Option<String>,
            chrono::DateTime<chrono::Utc>,
            Option<chrono::DateTime<chrono::Utc>>,
            Option<i64>,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        r#"
        SELECT
            gc.id,
            gc.code,
            gc.plan_id,
            p.name AS plan_name,
            gc.duration_days,
            gc.status,
            gc.created_at,
            gc.redeemed_at,
            gc.redeemed_by_user_id,
            gc.expires_at
        FROM gift_codes gc
        LEFT JOIN plans p ON p.id = gc.plan_id
        WHERE gc.created_by_user_id = $1
        ORDER BY gc.created_at DESC
        LIMIT 100
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let now = chrono::Utc::now();
    let payload: Vec<serde_json::Value> = rows
        .into_iter()
        .map(
            |(
                id,
                code,
                plan_id,
                plan_name,
                duration_days,
                status_raw,
                created_at,
                redeemed_at,
                redeemed_by_user_id,
                expires_at,
            )| {
                let status = if redeemed_by_user_id.is_some() {
                    "redeemed".to_string()
                } else if expires_at.is_some_and(|exp| exp <= now) {
                    "expired".to_string()
                } else {
                    status_raw
                        .unwrap_or_else(|| "active".to_string())
                        .to_ascii_lowercase()
                };
                let can_revoke = status == "active";

                serde_json::json!({
                    "id": id,
                    "code": code,
                    "plan_id": plan_id,
                    "plan_name": plan_name,
                    "duration_days": duration_days,
                    "status": status,
                    "created_at": created_at.to_rfc3339(),
                    "redeemed_at": redeemed_at.map(|dt| dt.to_rfc3339()),
                    "redeemed_by_user_id": redeemed_by_user_id,
                    "expires_at": expires_at.map(|dt| dt.to_rfc3339()),
                    "can_revoke": can_revoke,
                })
            },
        )
        .collect();

    Json(payload).into_response()
}

async fn revoke_my_gift_code(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(gift_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let updated = sqlx::query(
        r#"
        UPDATE gift_codes
        SET status = 'revoked',
            expires_at = COALESCE(expires_at, CURRENT_TIMESTAMP)
        WHERE id = $1
          AND created_by_user_id = $2
          AND redeemed_by_user_id IS NULL
          AND COALESCE(status, 'active') = 'active'
        "#,
    )
    .bind(gift_id)
    .bind(user_id)
    .execute(&state.pool)
    .await;

    match updated {
        Ok(done) if done.rows_affected() > 0 => Json(serde_json::json!({
            "ok": true,
            "message": "Gift code revoked",
        }))
        .into_response(),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            "Gift code not found or already inactive",
        )
            .into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct RedeemCodeReq {
    code: String,
}

async fn redeem_promo_code(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<RedeemCodeReq>,
) -> impl IntoResponse {
    let code = body.code.trim();
    if code.is_empty() {
        return (StatusCode::BAD_REQUEST, "Code cannot be empty").into_response();
    }

    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.promo_service.redeem_code(user_id, code).await {
        Ok(message) => Json(serde_json::json!({
            "ok": true,
            "message": message,
        }))
        .into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SetReferrerReq {
    code: String,
}

async fn set_referrer_code(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<SetReferrerReq>,
) -> impl IntoResponse {
    let code = body.code.trim();
    if code.is_empty() {
        return (StatusCode::BAD_REQUEST, "Referral code cannot be empty").into_response();
    }

    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.user_service.set_referrer(user_id, code).await {
        Ok(_) => Json(serde_json::json!({
            "ok": true,
            "message": "Referrer linked successfully",
        }))
        .into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}
