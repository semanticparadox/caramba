use crate::AppState;
use crate::singbox::connection_variants::available_connection_variants_for_node;
use axum::{
    Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{delete, get, post},
};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use sqlx::Row;
use std::collections::HashMap;
use std::env;
use tracing::warn;

#[inline]
fn ensure_jwt_crypto_provider() {
    // Idempotent process-wide install for jsonwebtoken 10.x.
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
}

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
    pub full_name: Option<String>,
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
            "/subscription/{id}/variant-event",
            post(track_subscription_variant_event).layer(middleware::from_fn_with_state(
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
        // Marketplace (Miniapp Integration)
        .route(
            "/payment/providers",
            get(get_payment_providers).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/payment/invoice",
            post(create_payment_invoice).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{id}/devices",
            get(get_subscription_devices).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{sub_id}/devices/{device_id}",
            delete(kick_subscription_device).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/notifications",
            get(get_notification_preferences)
                .put(update_notification_preferences)
                .layer(middleware::from_fn_with_state(state.clone(), auth_middleware)),
        )
}

async fn auth_telegram(
    State(state): State<AppState>,
    Json(payload): Json<InitDataRequest>,
) -> impl IntoResponse {
    tracing::info!("Received auth request");
    ensure_jwt_crypto_provider();

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
    let user_row = sqlx::query("SELECT id, username, full_name, balance FROM users WHERE tg_id = $1")
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
    let full_name: Option<String> = user_row.try_get("full_name").unwrap_or(None);
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

    let session_secret = state.session_secret.clone();
    let token = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(session_secret.as_bytes()),
        )
    })) {
        Ok(Ok(token)) => token,
        Ok(Err(_)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Token encode failed").into_response();
        }
        Err(_) => {
            tracing::error!("jsonwebtoken panicked while encoding token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Auth subsystem unavailable",
            )
                .into_response();
        }
    };

    Json(AuthResponse {
        token,
        user: AuthUserInfo {
            id: user_id,
            username,
            full_name,
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
    ensure_jwt_crypto_provider();

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    let token = match auth_header {
        Some(auth_header) if auth_header.starts_with("Bearer ") => &auth_header[7..],
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let session_secret = state.session_secret.clone();
    let token_data = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(session_secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
    })) {
        Ok(Ok(data)) => data,
        Ok(Err(_)) => return Err(StatusCode::UNAUTHORIZED),
        Err(_) => {
            tracing::error!("jsonwebtoken panicked while decoding token");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

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

    #[derive(sqlx::FromRow)]
    struct AggregatedUsage {
        traffic_used: i64,
        total_traffic: i64,
        days_left: i64,
        active_subscriptions: i64,
        plan_names: Option<String>,
    }

    #[derive(Serialize)]
    struct UserStats {
        traffic_used: i64,
        total_traffic: i64,
        days_left: i64,
        plan_name: String,
        active_subscriptions: i64,
        balance: i64,
        total_download: i64,
        total_upload: i64,
        simple_mode_enabled: bool,
        simple_mode_plan_id: i64,
        brand_name: String,
    }

    let balance_opt: Option<i64> = sqlx::query_scalar("SELECT balance FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let balance = match balance_opt {
        Some(v) => v,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let usage = match sqlx::query_as::<_, AggregatedUsage>(
        r#"
        SELECT
            COALESCE(SUM(COALESCE(s.used_traffic, 0)), 0)::BIGINT AS traffic_used,
            COALESCE(
                SUM(
                    CASE
                        WHEN COALESCE(p.traffic_limit_gb, 0) > 0
                            THEN CAST(p.traffic_limit_gb AS BIGINT) * 1073741824
                        ELSE 0
                    END
                ),
                0
            )::BIGINT AS total_traffic,
            COALESCE(
                MIN(
                    GREATEST(
                        0,
                        (EXTRACT(EPOCH FROM (COALESCE(s.expires_at, CURRENT_TIMESTAMP) - CURRENT_TIMESTAMP)) / 86400)::BIGINT
                    )
                ),
                0
            )::BIGINT AS days_left,
            COUNT(*)::BIGINT AS active_subscriptions,
            STRING_AGG(DISTINCT p.name, ', ' ORDER BY p.name) AS plan_names
        FROM subscriptions s
        JOIN plans p ON s.plan_id = p.id
        JOIN users u ON s.user_id = u.id
        WHERE u.tg_id = $1
          AND s.status = 'active'
        "#,
    )
    .bind(tg_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(row) => row,
        Err(e) => {
            tracing::error!("Failed to fetch aggregated user stats for {}: {}", tg_id, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch user statistics",
            )
                .into_response();
        }
    };

    let plan_name = if usage.active_subscriptions <= 0 {
        "No active subscription".to_string()
    } else if usage.active_subscriptions == 1 {
        usage
            .plan_names
            .clone()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "Active subscription".to_string())
    } else {
        format!("{} active subscriptions", usage.active_subscriptions)
    };

    Json(UserStats {
        traffic_used: usage.traffic_used,
        total_traffic: usage.total_traffic,
        days_left: usage.days_left,
        plan_name,
        active_subscriptions: usage.active_subscriptions,
        balance,
        total_download: usage.traffic_used,
        total_upload: 0,
        simple_mode_enabled: state
            .settings
            .get_or_default("simple_mode_enabled", "false")
            .await
            == "true",
        simple_mode_plan_id: state
            .settings
            .get_or_default("simple_mode_plan_id", "0")
            .await
            .parse()
            .unwrap_or(0),
        brand_name: state
            .settings
            .get_or_default("brand_name", "CARAMBA")
            .await,
    })
    .into_response()
}

// Subscriptions Endpoint — returns ALL user subscriptions with full details
async fn get_user_subscriptions(
    State(state): State<AppState>,
    headers: HeaderMap,
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

    let base_url = resolve_subscription_base_url(&state, &headers).await;

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
    let singbox_variants = state.subscription_service.get_singbox_connection_variants();
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
        let active_devices = state
            .subscription_service
            .get_active_ips(s.sub.id)
            .await
            .map(|ips| ips.len() as i64)
            .unwrap_or(0);
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
            "singbox_variants": singbox_variants.clone(),
        }));
    }

    Json(result).into_response()
}

async fn resolve_subscription_base_url(state: &AppState, headers: &HeaderMap) -> String {
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
            // Try Host header
            if let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) {
                host.to_string()
            } else {
                env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
            }
        }
    };

    if base_domain.starts_with("http") {
        base_domain
    } else {
        // Decide protocol based on domain/host
        let proto = if base_domain.contains("localhost") || base_domain.contains("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        format!("{}://{}", proto, base_domain)
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
            "simple_mode_enabled": state.settings.get_or_default("simple_mode_enabled", "false").await == "true",
            "simple_mode_plan_id": state.settings.get_or_default("simple_mode_plan_id", "0").await.parse::<i64>().unwrap_or(0),
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
    available_variant_ids: Vec<String>,
    recommended_variant_id: Option<String>,
}

#[derive(Deserialize)]
struct ServersQuery {
    sub_id: Option<i64>,
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
    Query(params): Query<ServersQuery>,
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
    let node_infos = state
        .subscription_service
        .get_node_infos_with_relays(&nodes)
        .await
        .unwrap_or_default();
    let variants_by_node: HashMap<String, Vec<String>> = node_infos
        .into_iter()
        .map(|node| {
            let variants = available_connection_variants_for_node(&node)
                .into_iter()
                .map(|variant| variant.id.to_string())
                .collect();
            (node.address, variants)
        })
        .collect();
    let recommended_by_node =
        build_recommended_variants_by_node(&state.pool, user_id, params.sub_id, &variants_by_node)
            .await;

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
                available_variant_ids: variants_by_node.get(&n.ip).cloned().unwrap_or_default(),
                recommended_variant_id: recommended_by_node.get(&n.id).cloned().flatten(),
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
        bonus_percent: i64,
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

        let bonus_percent: i64 = state
            .settings
            .get_or_default("referral_bonus_percent", "10")
            .await
            .parse()
            .unwrap_or(10);

        Json(ReferralStats {
            referral_code: code,
            referred_count: count,
            referral_link: link,
            total_earned_cents,
            total_earned_usd: total_earned_cents as f64 / 100.0,
            bonus_percent,
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

// ============================================================
// Marketplace Endpoints (Phase 3)
// ============================================================

#[derive(Serialize)]
struct PaymentProviderInfo {
    id: String,
    label: String,
}

fn has_value(value: &str) -> bool {
    !value.trim().is_empty()
}

async fn get_payment_providers(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let mut providers = Vec::new();

    // Check balance
    let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0);

    if balance > 0 {
        providers.push(PaymentProviderInfo {
            id: "balance".to_string(),
            label: format!("💰 Pay with Balance (${:.2})", balance as f64 / 100.0),
        });
    }

    if state
        .settings
        .get_or_default("manual_enabled", "false")
        .await
        == "true"
    {
        providers.push(PaymentProviderInfo {
            id: "manual".to_string(),
            label: "💳 Pay with Card/Manual".to_string(),
        });
    }

    if state
        .settings
        .get_or_default("telegram_stars_enabled", "false")
        .await
        == "true"
    {
        providers.push(PaymentProviderInfo {
            id: "stars".to_string(),
            label: "⭐️ Pay with Telegram Stars".to_string(),
        });
    }

    if has_value(&state.settings.get_or_default("payment_api_key", "").await) {
        providers.push(PaymentProviderInfo {
            id: "cryptobot".to_string(),
            label: "🪙 Pay with CryptoBot".to_string(),
        });
    }

    if has_value(
        &state
            .settings
            .get_or_default("nowpayments_api_key", "")
            .await,
    ) && has_value(
        &state
            .settings
            .get_or_default("nowpayments_ipn_secret", "")
            .await,
    ) {
        providers.push(PaymentProviderInfo {
            id: "nowpayments".to_string(),
            label: "🪙 Pay with NowPayments".to_string(),
        });
    }

    if has_value(
        &state
            .settings
            .get_or_default("cryptomus_payment_merchant_id", "")
            .await,
    ) && has_value(
        &state
            .settings
            .get_or_default("cryptomus_payment_api_key", "")
            .await,
    ) {
        providers.push(PaymentProviderInfo {
            id: "cryptomus".to_string(),
            label: "🪙 Pay with Cryptomus".to_string(),
        });
    }

    if has_value(&state.settings.get_or_default("lava_project_id", "").await)
        && has_value(&state.settings.get_or_default("lava_secret_key", "").await)
    {
        providers.push(PaymentProviderInfo {
            id: "lava".to_string(),
            label: "🪙 Pay with Lava.top".to_string(),
        });
    }

    if has_value(&state.settings.get_or_default("aaio_merchant_id", "").await)
        && has_value(&state.settings.get_or_default("aaio_secret_1", "").await)
    {
        providers.push(PaymentProviderInfo {
            id: "aaio".to_string(),
            label: "🪙 Pay with AAIO".to_string(),
        });
    }

    Json(serde_json::json!({ "providers": providers })).into_response()
}

#[derive(Deserialize)]
struct CreateInvoiceReq {
    provider: String,
    duration_id: Option<i64>,
    order_id: Option<i64>,
}

async fn create_payment_invoice(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<CreateInvoiceReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let user_db: Option<caramba_db::models::store::User> = state
        .store_service
        .get_user_by_tg_id(tg_id)
        .await
        .ok()
        .flatten();
    let u = match user_db {
        Some(user) => user,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let (product_id, amount) = if let Some(duration_id) = body.duration_id {
        // Handle Plan Duration
        let duration_row: Option<(i64, i64)> = sqlx::query_as::<_, (i64, i64)>(
            "SELECT plan_id, price FROM plan_durations WHERE id = $1",
        )
        .bind(duration_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

        match duration_row {
            Some(row) => row, // (plan_id, price)
            None => return (StatusCode::BAD_REQUEST, "Invalid duration ID").into_response(),
        }
    } else if let Some(order_id) = body.order_id {
        // Handle Store Order
        let order_row: Option<(i64, i64)> = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, total_amount FROM orders WHERE id = $1 AND user_id = $2",
        )
        .bind(order_id)
        .bind(u.id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

        match order_row {
            Some(row) => row, // (order_id as product_id conceptually, total_amount)
            None => return (StatusCode::BAD_REQUEST, "Invalid order ID").into_response(),
        }
    } else {
        return (StatusCode::BAD_REQUEST, "Missing duration_id or order_id").into_response();
    };

    let currency = "USD";
    let mut metadata = HashMap::new();
    if body.duration_id.is_some() {
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("plan".to_string()),
        );
    } else if body.order_id.is_some() {
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("order".to_string()),
        );
    }

    match state
        .marketplace_service
        .create_session(
            &u,
            product_id,
            &body.provider,
            amount,
            currency,
            Some(serde_json::to_value(metadata).unwrap_or_default()),
        )
        .await
    {
        Ok((session, invoice_payload)) => {
            if body.provider == "balance" {
                // Synchronous fulfillment for balance
                if let Err(e) = state.marketplace_service.fulfill_payment(session.id).await {
                    tracing::error!("Immediate balance fulfillment failed: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Fulfillment failed: {}", e),
                    )
                        .into_response();
                }

                // Deduct balance manually if BalanceProvider didn't do it (it doesn't in our implementation to keep MarketplaceService pure)
                let _ = sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
                    .bind(amount)
                    .bind(u.id)
                    .execute(&state.pool)
                    .await;

                return Json(serde_json::json!({
                    "ok": true,
                    "invoice_url": "SUCCESS",
                    "provider": "balance",
                    "fulfilled": true
                }))
                .into_response();
            }

            Json(serde_json::json!({
                "ok": true,
                "invoice_url": invoice_payload,
                "provider": body.provider,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Invoice generation failed for user {}: {}", u.id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
struct PinNodeReq {
    node_id: i64,
}

#[derive(Deserialize)]
struct VariantEventReq {
    node_id: i64,
    variant_id: String,
    event: String,
    client: Option<String>,
}

fn allowed_variant_event(event: &str) -> bool {
    matches!(
        event,
        "variant_selected"
            | "config_opened"
            | "config_copied"
            | "connection_succeeded"
            | "connection_failed"
    )
}

fn variant_event_weight(event: &str) -> i32 {
    match event {
        "connection_succeeded" => 8,
        "connection_failed" => -6,
        "config_copied" => 4,
        "config_opened" => 2,
        "variant_selected" => 1,
        _ => 0,
    }
}

fn choose_recommended_variant(
    scores: &HashMap<String, i32>,
    available_variant_ids: &[String],
) -> Option<String> {
    available_variant_ids
        .iter()
        .filter_map(|variant_id| {
            scores
                .get(variant_id)
                .map(|score| (variant_id.clone(), *score))
        })
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
        .map(|entry| entry.0)
        .or_else(|| available_variant_ids.first().cloned())
}

async fn build_recommended_variants_by_node(
    pool: &sqlx::PgPool,
    user_id: i64,
    sub_id: Option<i64>,
    variants_by_node_ip: &HashMap<String, Vec<String>>,
) -> HashMap<i64, Option<String>> {
    let Some(subscription_id) = sub_id else {
        return HashMap::new();
    };

    let node_rows =
        match sqlx::query_as::<_, (i64, String)>("SELECT id, ip FROM nodes WHERE ip = ANY($1)")
            .bind(&variants_by_node_ip.keys().cloned().collect::<Vec<String>>())
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return HashMap::new(),
        };

    let node_ids: Vec<i64> = node_rows.iter().map(|(node_id, _)| *node_id).collect();
    if node_ids.is_empty() {
        return HashMap::new();
    }

    let details_rows = match sqlx::query_scalar::<_, String>(
        "SELECT details FROM activity_log WHERE user_id = $1 AND action = $2 ORDER BY created_at DESC LIMIT 500",
    )
    .bind(user_id)
    .bind("subscription_variant_event")
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return HashMap::new(),
    };

    let mut scores_by_node: HashMap<i64, HashMap<String, i32>> = HashMap::new();
    for details in details_rows {
        let Ok(parsed) = serde_json::from_str::<Value>(&details) else {
            continue;
        };

        let event_sub_id = parsed.get("subscription_id").and_then(Value::as_i64);
        let event_node_id = parsed.get("node_id").and_then(Value::as_i64);
        let event_name = parsed.get("event").and_then(Value::as_str);
        let variant_id = parsed.get("variant_id").and_then(Value::as_str);

        let (Some(event_sub_id), Some(event_node_id), Some(event_name), Some(variant_id)) =
            (event_sub_id, event_node_id, event_name, variant_id)
        else {
            continue;
        };

        if event_sub_id != subscription_id || !node_ids.contains(&event_node_id) {
            continue;
        }

        let weight = variant_event_weight(event_name);
        if weight == 0 {
            continue;
        }

        let entry = scores_by_node.entry(event_node_id).or_default();
        *entry.entry(variant_id.to_string()).or_insert(0) += weight;
    }

    node_rows
        .into_iter()
        .map(|(node_id, node_ip)| {
            let available_variant_ids = variants_by_node_ip
                .get(&node_ip)
                .cloned()
                .unwrap_or_default();
            let recommended = scores_by_node
                .get(&node_id)
                .and_then(|scores| choose_recommended_variant(scores, &available_variant_ids));
            (
                node_id,
                recommended.or_else(|| available_variant_ids.first().cloned()),
            )
        })
        .collect()
}

#[cfg(test)]
mod client_variant_event_tests {
    use super::{allowed_variant_event, choose_recommended_variant, variant_event_weight};
    use std::collections::HashMap;

    #[test]
    fn accepts_supported_variant_events() {
        assert!(allowed_variant_event("variant_selected"));
        assert!(allowed_variant_event("config_opened"));
        assert!(allowed_variant_event("config_copied"));
        assert!(allowed_variant_event("connection_succeeded"));
        assert!(allowed_variant_event("connection_failed"));
    }

    #[test]
    fn rejects_unknown_variant_events() {
        assert!(!allowed_variant_event("opened"));
        assert!(!allowed_variant_event("success"));
    }

    #[test]
    fn scores_variant_events_by_strength() {
        assert_eq!(variant_event_weight("variant_selected"), 1);
        assert_eq!(variant_event_weight("config_opened"), 2);
        assert_eq!(variant_event_weight("config_copied"), 4);
        assert_eq!(variant_event_weight("connection_succeeded"), 8);
        assert_eq!(variant_event_weight("connection_failed"), -6);
        assert_eq!(variant_event_weight("unknown"), 0);
    }

    #[test]
    fn chooses_highest_scoring_available_variant() {
        let mut scores = HashMap::new();
        scores.insert("grpc-direct".to_string(), 3);
        scores.insert("vless-httpupgrade-relay".to_string(), 9);

        let available = vec![
            "vless-httpupgrade-relay".to_string(),
            "grpc-direct".to_string(),
        ];

        assert_eq!(
            choose_recommended_variant(&scores, &available).as_deref(),
            Some("vless-httpupgrade-relay")
        );
    }

    #[test]
    fn falls_back_to_first_available_variant_when_no_score_exists() {
        let scores = HashMap::new();
        let available = vec![
            "vless-reality-direct".to_string(),
            "grpc-direct".to_string(),
        ];

        assert_eq!(
            choose_recommended_variant(&scores, &available).as_deref(),
            Some("vless-reality-direct")
        );
    }
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
    headers: HeaderMap,
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
    let base_url = resolve_subscription_base_url(&state, &headers).await;

    Json(serde_json::json!({
        "subscription_url": format!("{}/sub/{}", base_url, subscription_uuid),
        "links": links,
        "vless_links": vless_links,
        "primary_vless_link": vless_links.first().cloned(),
        "singbox_variants": state.subscription_service.get_singbox_connection_variants(),
    }))
    .into_response()
}

async fn track_subscription_variant_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
    Json(body): Json<VariantEventReq>,
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

    if !allowed_variant_event(body.event.as_str()) {
        return (StatusCode::BAD_REQUEST, "Unsupported event").into_response();
    }

    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("cf-connecting-ip"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .unwrap_or("")
        .trim()
        .to_string();

    let details = serde_json::json!({
        "subscription_id": sub_id,
        "node_id": body.node_id,
        "variant_id": body.variant_id,
        "event": body.event,
        "client": body.client.unwrap_or_else(|| "singbox".to_string()),
    })
    .to_string();

    match sqlx::query(
        "INSERT INTO activity_log (user_id, action, details, ip_address) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind("subscription_variant_event")
    .bind(details)
    .bind(if client_ip.is_empty() {
        None
    } else {
        Some(client_ip)
    })
    .execute(&state.pool)
    .await
    {
        Ok(_) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => {
            tracing::warn!("Failed to record subscription variant event: {}", err);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to record event").into_response()
        }
    }
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

// ============================================================================
// Device Management
// ============================================================================

#[derive(Serialize)]
struct DeviceInfo {
    id: i64,
    device_name: String,
    last_ip: String,
    last_seen_at: String,
    first_seen_at: String,
    is_current: bool,
}

fn mask_ip(ip: &str) -> String {
    if let Some(dot_pos) = ip.rfind('.') {
        format!("{}.*", &ip[..dot_pos])
    } else if let Some(colon_pos) = ip.rfind(':') {
        format!("{}:*", &ip[..colon_pos])
    } else {
        ip.to_string()
    }
}

async fn get_subscription_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Verify subscription belongs to this user
    let owner_tg_id = sqlx::query_scalar::<_, i64>(
        "SELECT u.tg_id FROM subscriptions s JOIN users u ON s.user_id = u.id WHERE s.id = $1",
    )
    .bind(sub_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if owner_tg_id != Some(tg_id) {
        return (StatusCode::FORBIDDEN, "Not your subscription").into_response();
    }

    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("cf-connecting-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    #[derive(sqlx::FromRow)]
    struct DeviceLease {
        id: i64,
        device_name: Option<String>,
        last_ip: String,
        last_seen_at: chrono::DateTime<chrono::Utc>,
        first_seen_at: chrono::DateTime<chrono::Utc>,
    }

    let leases = sqlx::query_as::<_, DeviceLease>(
        r#"SELECT id, device_name, last_ip, last_seen_at, first_seen_at
           FROM subscription_device_leases
           WHERE subscription_id = $1
             AND last_seen_at > NOW() - INTERVAL '15 minutes'
             AND last_ip <> '0.0.0.0'
           ORDER BY last_seen_at DESC"#,
    )
    .bind(sub_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let devices: Vec<DeviceInfo> = leases
        .into_iter()
        .map(|l| DeviceInfo {
            id: l.id,
            device_name: l.device_name.unwrap_or_else(|| "Unknown".to_string()),
            is_current: l.last_ip == client_ip,
            last_ip: mask_ip(&l.last_ip),
            last_seen_at: l.last_seen_at.to_rfc3339(),
            first_seen_at: l.first_seen_at.to_rfc3339(),
        })
        .collect();

    Json(devices).into_response()
}

async fn kick_subscription_device(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path((sub_id, device_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Verify subscription belongs to this user
    let owner_tg_id = sqlx::query_scalar::<_, i64>(
        "SELECT u.tg_id FROM subscriptions s JOIN users u ON s.user_id = u.id WHERE s.id = $1",
    )
    .bind(sub_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if owner_tg_id != Some(tg_id) {
        return (StatusCode::FORBIDDEN, "Not your subscription").into_response();
    }

    // Fetch device lease — must belong to this subscription
    let device_ip = sqlx::query_scalar::<_, String>(
        "SELECT last_ip FROM subscription_device_leases WHERE id = $1 AND subscription_id = $2",
    )
    .bind(device_id)
    .bind(sub_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let ip = match device_ip {
        Some(ip) => ip,
        None => return (StatusCode::NOT_FOUND, "Device not found").into_response(),
    };

    // Delete from device leases
    let _ = sqlx::query("DELETE FROM subscription_device_leases WHERE id = $1 AND subscription_id = $2")
        .bind(device_id)
        .bind(sub_id)
        .execute(&state.pool)
        .await;

    // Delete from legacy IP tracking
    let _ = sqlx::query(
        "DELETE FROM subscription_ip_tracking WHERE subscription_id = $1 AND client_ip = $2",
    )
    .bind(sub_id)
    .bind(&ip)
    .execute(&state.pool)
    .await;

    // Best-effort: kill active connections on nodes via Clash API
    // The connection_service handles this in background — we just clean the DB records
    // and the device will be blocked on next config request (15-min window expired)

    let _ = crate::services::activity_service::ActivityService::log(
        &state.pool,
        "Device:Kicked",
        &format!("User kicked device {} (IP {}) from sub #{}", device_id, mask_ip(&ip), sub_id),
    )
    .await;

    Json(serde_json::json!({ "ok": true, "message": "Device disconnected" })).into_response()
}

// ============================================================================
// Notification Preferences
// ============================================================================

#[derive(Serialize, Deserialize)]
struct NotificationPrefs {
    notify_new_device: bool,
    notify_traffic_warnings: bool,
    notify_expiry_reminders: bool,
}

async fn get_notification_preferences(
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

    let prefs = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT notify_new_device, notify_traffic_warnings, notify_expiry_reminders FROM notification_preferences WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let (nd, tw, er) = prefs.unwrap_or((true, true, true));

    Json(NotificationPrefs {
        notify_new_device: nd,
        notify_traffic_warnings: tw,
        notify_expiry_reminders: er,
    })
    .into_response()
}

async fn update_notification_preferences(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<NotificationPrefs>,
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

    let _ = sqlx::query(
        "INSERT INTO notification_preferences (user_id, notify_new_device, notify_traffic_warnings, notify_expiry_reminders, updated_at)
         VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)
         ON CONFLICT (user_id) DO UPDATE SET
            notify_new_device = $2, notify_traffic_warnings = $3, notify_expiry_reminders = $4, updated_at = CURRENT_TIMESTAMP"
    )
    .bind(user_id)
    .bind(body.notify_new_device)
    .bind(body.notify_traffic_warnings)
    .bind(body.notify_expiry_reminders)
    .execute(&state.pool)
    .await;

    Json(serde_json::json!({ "ok": true })).into_response()
}
