use crate::AppState;
use crate::singbox::connection_variants::available_connection_variants_for_node;
use axum::{
    Router,
    extract::{Multipart, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
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
        .route(
            "/relay-countries",
            get(get_relay_countries).layer(middleware::from_fn_with_state(
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
            "/subscription/{id}/extend",
            post(extend_subscription).layer(middleware::from_fn_with_state(
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
            "/payment/session/{id}",
            get(get_payment_session_status).layer(middleware::from_fn_with_state(
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
            "/subscription/{id}/devices/kill-all",
            post(kill_all_subscription_devices).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/subscription/{sub_id}/devices/{device_id}/name",
            put(rename_subscription_device).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/user/notifications",
            get(get_notification_preferences)
                .put(update_notification_preferences)
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                )),
        )
        // ----------------------------------------------------------------
        // Система уведомлений (inbox пользователя)
        // ----------------------------------------------------------------
        .route(
            "/notifications",
            get(client_list_notifications).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/notifications/unread-count",
            get(client_notifications_unread_count).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/notifications/read-all",
            post(client_notifications_read_all).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/notifications/{id}/read",
            post(client_notification_mark_read).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/notification-preferences",
            get(client_get_notif_prefs)
                .put(client_set_notif_prefs)
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                )),
        )
        .route(
            "/user/language",
            get(client_get_language).put(client_set_language).layer(
                middleware::from_fn_with_state(state.clone(), auth_middleware),
            ),
        )
        // ----------------------------------------------------------------
        // Тикеты поддержки
        // ----------------------------------------------------------------
        .route(
            "/tickets",
            get(client_list_tickets).post(client_create_ticket).layer(
                middleware::from_fn_with_state(state.clone(), auth_middleware),
            ),
        )
        .route(
            "/tickets/{id}",
            get(client_get_ticket).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/tickets/{id}/messages",
            post(client_add_ticket_message).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/tickets/{id}/attach",
            post(client_attach_file).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/tickets/{id}/attachments/{attachment_id}",
            get(client_download_attachment).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
}

/// Извлекаем IP клиента из заголовков обратного прокси (Cloudflare / Caddy / Nginx),
/// с фолбэком на адрес сокета. Тот же порядок приоритета, что и в admin-логине
/// (handlers/admin/auth.rs::extract_login_ip), чтобы поведение было единообразным.
fn extract_client_ip(headers: &HeaderMap, addr: &std::net::SocketAddr) -> String {
    headers
        .get("cf-connecting-ip")
        .or_else(|| headers.get("x-real-ip"))
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.ip().to_string())
}

async fn auth_telegram(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(payload): Json<InitDataRequest>,
) -> impl IntoResponse {
    tracing::info!("Received auth request");
    ensure_jwt_crypto_provider();

    // Rate limiting (защита от брутфорса/DoS на auth-эндпоинт).
    // Fixed-window лимитер на Redis — тот же механизм, что и в admin-логине.
    // Лимит по IP: не более 20 попыток за 60 секунд. При успешной валидации
    // подписи Telegram легитимный клиент укладывается в этот предел с запасом.
    const AUTH_IP_LIMIT: usize = 20;
    const AUTH_WINDOW_SECS: usize = 60;
    let client_ip = extract_client_ip(&headers, &addr);
    let ip_rate_key = format!("rate:client_auth:ip:{}", client_ip);
    match state
        .redis
        .check_rate_limit(&ip_rate_key, AUTH_IP_LIMIT, AUTH_WINDOW_SECS)
        .await
    {
        Ok(allowed) => {
            if !allowed {
                tracing::warn!(ip = %client_ip, "Client auth rate limit exceeded (per-IP)");
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many authentication attempts. Please try again later.",
                )
                    .into_response();
            }
        }
        Err(e) => {
            // При недоступности Redis НЕ блокируем легитимных пользователей —
            // деградируем gracefully (fail-open), но логируем для наблюдаемости.
            tracing::error!(err = %e, "Redis rate-limit check failed for client auth (per-IP)");
        }
    }

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

    // 2b. Validate auth_date — защита от replay-атак с перехваченными initData.
    // Telegram допускает использование initData в течение 24 часов после выпуска.
    // Токены старше этого порога отклоняем.
    const MAX_AUTH_AGE_SECONDS: i64 = 86_400; // 24 hours
    if let Some(auth_date_str) = params.get("auth_date") {
        match auth_date_str.parse::<i64>() {
            Ok(auth_date) => {
                let now = chrono::Utc::now().timestamp();
                let age = now - auth_date;
                if !(0..=MAX_AUTH_AGE_SECONDS).contains(&age) {
                    tracing::warn!(
                        auth_date,
                        age_seconds = age,
                        "Auth failed: initData expired (auth_date too old or in future)"
                    );
                    return (StatusCode::UNAUTHORIZED, "InitData expired").into_response();
                }
            }
            Err(_) => {
                tracing::warn!("Auth failed: auth_date is not a valid integer");
                return (StatusCode::BAD_REQUEST, "Invalid auth_date").into_response();
            }
        }
    } else {
        // auth_date отсутствует — Telegram всегда включает его в initData
        tracing::warn!("Auth failed: missing auth_date field in initData");
        return (StatusCode::BAD_REQUEST, "Missing auth_date").into_response();
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

    // Дополнительный rate-limit по tg_id — троттлит подбор/replay против
    // конкретного аккаунта даже при ротации IP. Лимит щедрее, чем per-IP,
    // т.к. сюда мы попадаем только после успешной проверки HMAC-подписи.
    {
        const AUTH_TGID_LIMIT: usize = 30;
        const AUTH_TGID_WINDOW_SECS: usize = 60;
        let tgid_rate_key = format!("rate:client_auth:tg:{}", tg_id);
        match state
            .redis
            .check_rate_limit(&tgid_rate_key, AUTH_TGID_LIMIT, AUTH_TGID_WINDOW_SECS)
            .await
        {
            Ok(allowed) => {
                if !allowed {
                    tracing::warn!(tg_id, "Client auth rate limit exceeded (per-tg_id)");
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        "Too many authentication attempts. Please try again later.",
                    )
                        .into_response();
                }
            }
            Err(e) => {
                // fail-open при недоступности Redis (как и для per-IP лимита)
                tracing::error!(err = %e, "Redis rate-limit check failed for client auth (per-tg_id)");
            }
        }
    }

    // 4. Look up user by tg_id
    let user_row =
        sqlx::query("SELECT id, username, full_name, balance FROM users WHERE tg_id = $1")
            .bind(tg_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    let user_row = match user_row {
        Some(row) => row,
        None => {
            return (
                StatusCode::FORBIDDEN,
                "User not found. Start the bot first.",
            )
                .into_response();
        }
    };
    let user_id: i64 = user_row.get("id");
    let username: String = user_row.try_get("username").unwrap_or_default();
    let db_full_name: Option<String> = user_row.try_get("full_name").unwrap_or(None);
    let balance: i64 = user_row.try_get("balance").unwrap_or(0);

    // Fallback to Telegram initData first_name + last_name if DB full_name is empty
    let full_name = if db_full_name.as_ref().is_none_or(|n| n.trim().is_empty()) {
        let first = user_json
            .get("first_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let last = user_json
            .get("last_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let combined = format!("{} {}", first, last).trim().to_string();
        if !combined.is_empty() {
            // Persist to DB so it's available everywhere
            let _ = sqlx::query("UPDATE users SET full_name = $1 WHERE id = $2")
                .bind(&combined)
                .bind(user_id)
                .execute(&state.pool)
                .await;
            Some(combined)
        } else {
            None
        }
    } else {
        db_full_name
    };

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

    // Token revocation deny-list (additive, inert-by-default).
    // Если для данного клиента выставлен ключ `revoked:client:{sub}` в Redis —
    // отклоняем токен. Сегодня этот ключ никем не устанавливается, поэтому для
    // всех действующих сессий поведение не меняется; механизм даёт безопасную
    // точку отзыва (например, admin «kill sessions» или будущий logout могут
    // выполнить SETEX revoked:client:{tg_id} <ttl≈оставшееся время жизни JWT>).
    // Fail-open при недоступности Redis — выход из строя кэша не должен
    // блокировать легитимных пользователей.
    let revoke_key = format!("revoked:client:{}", token_data.claims.sub);
    match state.redis.exists(&revoke_key).await {
        Ok(true) => {
            tracing::warn!(sub = %token_data.claims.sub, "Rejected revoked client token");
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(false) => {}
        Err(e) => {
            tracing::error!(err = %e, "Redis revocation check failed; allowing request (fail-open)");
        }
    }

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
        brand_name: String,
        // URL поддержки — берётся из настроек, чтобы Mini App не хардкодил его
        support_url: String,
        // Username бота (без @) — Mini App строит по нему кнопку «Открыть чат»
        // после отправки платёжной ссылки в личку. Пустая строка = не настроен.
        bot_username: String,
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
                        -- Потолок = лимит тарифа + бонусный трафик пользователя,
                        -- ровно как в квотных гейтах (services/bonus_traffic.rs).
                        -- Иначе цифра в приложении расходилась бы с тем, по чему
                        -- реально отключают.
                        WHEN COALESCE(p.traffic_limit_gb, 0) > 0
                            THEN CAST(p.traffic_limit_gb AS BIGINT) * 1073741824
                                 + COALESCE(u.bonus_traffic_mb, 0) * 1048576
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
          AND s.status IN ('active', 'throttled')
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
        brand_name: state.settings.get_or_default("brand_name", "CARAMBA").await,
        support_url: state
            .settings
            .get_or_default("support_url", "https://t.me/")
            .await,
        bot_username: state
            .settings
            .get_or_default("bot_username", "")
            .await
            .trim()
            .trim_start_matches('@')
            .to_string(),
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

    // Бонусный трафик пользователя — общий для всех подписок. Отдаём его в
    // JSON вместе с готовым потолком в байтах, чтобы приложение показывало ту
    // же цифру, по которой работает энфорсмент (traffic_limit_gb в ГБ не может
    // выразить бонус в МБ без потери точности).
    let bonus_traffic_mb = crate::services::bonus_traffic::balance_mb(&state.pool, user_id)
        .await
        .unwrap_or(0);

    let plan_ids: Vec<i64> = subs.iter().map(|s| s.sub.plan_id).collect();
    let mut device_limits_by_plan: HashMap<i64, i64> = HashMap::new();
    // Мета-данные плана: is_free и daily_traffic_mb для UX бесплатного плана
    let mut is_free_by_plan: HashMap<i64, bool> = HashMap::new();
    let mut daily_traffic_mb_by_plan: HashMap<i64, i32> = HashMap::new();
    if !plan_ids.is_empty() {
        let rows = sqlx::query_as::<_, (i64, i64, bool, i32)>(
            "SELECT id, COALESCE(device_limit, 0)::BIGINT,
             COALESCE(is_free, FALSE), COALESCE(daily_traffic_mb, 0)
             FROM plans WHERE id = ANY($1)",
        )
        .bind(&plan_ids)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        for (plan_id, device_limit, is_free, daily_mb) in rows {
            device_limits_by_plan.insert(plan_id, device_limit);
            is_free_by_plan.insert(plan_id, is_free);
            daily_traffic_mb_by_plan.insert(plan_id, daily_mb);
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
        let is_free = is_free_by_plan
            .get(&s.sub.plan_id)
            .copied()
            .unwrap_or(false);
        let daily_traffic_mb = daily_traffic_mb_by_plan
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
            "plan_id": s.sub.plan_id,
            "plan_name": s.plan_name,
            "plan_description": s.plan_description,
            "status": s.sub.status,
            "used_traffic_bytes": s.sub.used_traffic,
            "used_traffic_gb": format!("{:.2}", used_gb),
            "traffic_limit_gb": traffic_limit_gb,
            // Потолок энфорсмента в байтах (лимит тарифа + бонус); null =
            // безлимит. Клиент должен предпочитать его traffic_limit_gb.
            "traffic_limit_bytes": crate::services::bonus_traffic::quota_limit_bytes(
                traffic_limit_gb as i64,
                bonus_traffic_mb,
            ),
            "bonus_traffic_mb": bonus_traffic_mb,
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
            "is_free": is_free,
            "daily_traffic_mb": daily_traffic_mb,
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
    flag: String,
    latency: Option<i32>,
    status: String,
    distance_km: Option<i32>,
    name: String,
    available_variant_ids: Vec<String>,
    recommended_variant_id: Option<String>,
    active_connections: i32,
    max_users: i32,
    is_full: bool,
}

#[derive(Deserialize)]
struct ServersQuery {
    sub_id: Option<i64>,
    lat: Option<f64>,
    lon: Option<f64>,
}

// Helper for flag — безопасная версия без unwrap() на данных из БД
fn get_flag(country: &str) -> String {
    let country = country.to_uppercase();
    // Ограничиваем ASCII-буквами, чтобы арифметика с кодовыми точками была предсказуемой
    let chars: Vec<char> = country
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    if chars.len() != 2 {
        return "🌐".to_string();
    }
    let offset = 127397u32;
    let first = chars[0] as u32 + offset;
    let second = chars[1] as u32 + offset;
    match (char::from_u32(first), char::from_u32(second)) {
        (Some(f), Some(s)) => format!("{}{}", f, s),
        _ => "🌐".to_string(),
    }
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

    // 1. Get Client IP/Location (consistent with subscription.rs)
    let client_ip = headers
        .get("cf-connecting-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    let user_coords = if let (Some(lat), Some(lon)) = (params.lat, params.lon) {
        Some((lat, lon))
    } else {
        get_client_coordinates(state.clone(), client_ip).await
    };

    // 2. Map to ClientNode & Calculate Distance & Load Score
    let mut client_nodes: Vec<ClientNode> = nodes
        .into_iter()
        .filter(|n| {
            // Hide relay infrastructure nodes from users
            if n.is_relay {
                return false;
            }

            n.last_cpu.unwrap_or(0.0) < 95.0 && n.last_ram.unwrap_or(0.0) < 98.0
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

            let connections = n.active_connections.unwrap_or(0);
            let max = n.max_users;
            let is_full = max > 0 && connections >= max;

            ClientNode {
                id: n.id,
                country_code: n.country_code.clone(),
                flag: get_flag(n.country_code.as_deref().unwrap_or("US")),
                latency: n.last_latency.map(|l| l as i32),
                status: if is_full {
                    "full".to_string()
                } else {
                    status_label
                },
                distance_km: dist,
                name: format!("Node #{} ({} Mbps)", n.id, speed),
                available_variant_ids: variants_by_node.get(&n.ip).cloned().unwrap_or_default(),
                recommended_variant_id: recommended_by_node.get(&n.id).cloned().flatten(),
                active_connections: connections,
                max_users: max,
                is_full,
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

// Relay Countries Endpoint
async fn get_relay_countries(State(state): State<AppState>) -> impl IntoResponse {
    #[derive(Serialize)]
    struct RelayCountry {
        code: String,
        flag: String,
        name: String,
    }

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT country_code FROM nodes WHERE is_relay = TRUE AND status = 'active' AND country_code IS NOT NULL ORDER BY country_code",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let countries: Vec<RelayCountry> = rows
        .into_iter()
        .map(|(code,)| {
            let flag = get_flag(&code);
            let name = match code.to_uppercase().as_str() {
                "RU" => "Россия",
                "US" => "США",
                "DE" => "Германия",
                "NL" => "Нидерланды",
                "FI" => "Финляндия",
                "FR" => "Франция",
                "GB" | "UK" => "Великобритания",
                "TR" => "Турция",
                "KZ" => "Казахстан",
                "UA" => "Украина",
                "CA" => "Канада",
                "JP" => "Япония",
                "SG" => "Сингапур",
                _ => &code,
            };
            RelayCountry {
                code: code.to_uppercase(),
                flag,
                name: name.to_string(),
            }
        })
        .collect();

    Json(countries)
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
        /// Сколько получит реферрер при каждой регистрации по его ссылке (центы)
        referrer_signup_bonus_cents: i64,
        /// Сколько получит новый пользователь при регистрации по реферальной ссылке (центы)
        referred_signup_bonus_cents: i64,
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

        // Индивидуальные ставки реферрера имеют приоритет над глобальными
        let custom_rates: Option<(Option<i32>, Option<i32>, Option<i32>)> = sqlx::query_as(
            "SELECT bonus_percent, referrer_signup_bonus_cents, referred_signup_bonus_cents FROM user_referral_rates WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

        let global_referrer_signup: i64 = state
            .settings
            .get_or_default("referral_referrer_signup_bonus_cents", "0")
            .await
            .parse()
            .unwrap_or(0);
        let global_referred_signup: i64 = state
            .settings
            .get_or_default("referral_referred_signup_bonus_cents", "0")
            .await
            .parse()
            .unwrap_or(0);

        let referrer_signup_bonus_cents: i64 = custom_rates
            .as_ref()
            .and_then(|(_, r, _)| r.map(|v| v as i64))
            .unwrap_or(global_referrer_signup);
        let referred_signup_bonus_cents: i64 = custom_rates
            .as_ref()
            .and_then(|(_, _, r)| r.map(|v| v as i64))
            .unwrap_or(global_referred_signup);

        Json(ReferralStats {
            referral_code: code,
            referred_count: count,
            referral_link: link,
            total_earned_cents,
            total_earned_usd: total_earned_cents as f64 / 100.0,
            bonus_percent,
            referrer_signup_bonus_cents,
            referred_signup_bonus_cents,
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

/// Витрина товаров — снята с маршрутизации, см. комментарий в main.rs.
/// Оставлена в коде намеренно: вернуть раздел = снова зарегистрировать маршрут,
/// а не писать обработчик заново. Удалить, если магазин так и не появится.
#[allow(dead_code)]
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
    /// Если true — списываем баланс и создаём подарочный код вместо активной подписки
    #[serde(default)]
    as_gift: bool,
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
        .purchase_plan(user_id, body.duration_id, body.as_gift)
        .await
    {
        Ok(crate::services::store_service::PurchaseResult::Subscription(sub)) => {
            Json(serde_json::json!({
                "ok": true,
                "type": "subscription",
                "subscription_id": sub.id,
                "status": sub.status,
                "message": "Purchase successful! Your subscription is now active."
            }))
            .into_response()
        }
        Ok(crate::services::store_service::PurchaseResult::GiftCode(code)) => {
            Json(serde_json::json!({
                "ok": true,
                "type": "gift",
                "gift_code": code,
                "message": "Gift code created! Share this code with someone."
            }))
            .into_response()
        }
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
    /// Effective price in minor units for the requested target (duration/order), if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<i64>,
    /// ISO currency (or "USD" fallback) for `amount`, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    currency: Option<String>,
}

/// Human-friendly label (with emoji) for a registered payment provider id.
pub fn provider_label(name: &str) -> String {
    match name {
        "manual" => "💳 Card / Manual transfer",
        "cryptobot" => "🪙 CryptoBot",
        "nowpayments" => "🪙 NOWPayments",
        "cryptomus" => "🪙 Cryptomus",
        "lava" => "💎 Lava.top",
        "aaio" => "🇷🇺 AAIO",
        "stripe" => "💳 Stripe",
        "wata" => "🇷🇺 WATA",
        "crystalpay" => "🇷🇺 CrystalPay",
        "tribute" => "🇷🇺 Tribute (RUB → crypto)",
        "btcpay" => "₿ BTCPay Server",
        "oxapay" => "🪙 OxaPay",
        "coinbase_commerce" => "🪙 Coinbase Commerce",
        "plisio" => "🪙 Plisio",
        "paypalych" => "🇷🇺 Paypalych (СБП / USDT)",
        "stars" => "⭐️ Telegram Stars",
        other => other,
    }
    .to_string()
}

#[derive(Deserialize)]
struct ProviderListQuery {
    duration_id: Option<i64>,
    order_id: Option<i64>,
}

async fn get_payment_providers(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    axum::extract::Query(q): axum::extract::Query<ProviderListQuery>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let mut providers = Vec::new();

    // Resolve the base price (USD, minor units) for the requested target once, so each
    // provider entry can advertise its effective amount/currency (override or base).
    let base_amount: Option<i64> = if let Some(did) = q.duration_id {
        sqlx::query_scalar("SELECT price FROM plan_durations WHERE id = $1")
            .bind(did)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
    } else if let Some(oid) = q.order_id {
        sqlx::query_scalar("SELECT total_amount FROM orders WHERE id = $1")
            .bind(oid)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
    } else {
        None
    };

    // Per-provider overrides for the target (empty when no target supplied).
    let overrides: std::collections::HashMap<String, (i64, String)> =
        if let Some(did) = q.duration_id {
            state.catalog_service.list_duration_overrides(did).await
        } else if let Some(oid) = q.order_id {
            // For orders the override is per single product line; resolve lazily below.
            // We still pre-fetch nothing here and compute per provider via resolver.
            let _ = oid;
            std::collections::HashMap::new()
        } else {
            std::collections::HashMap::new()
        };

    // Effective (amount, currency) for a provider given the resolved base + overrides.
    let price_for = |name: &str| -> (Option<i64>, Option<String>) {
        if let Some((a, c)) = overrides.get(name) {
            (Some(*a), Some(c.clone()))
        } else if let Some(b) = base_amount {
            (Some(b), Some("USD".to_string()))
        } else {
            (None, None)
        }
    };

    // ---- Special-case methods (not part of the MarketplaceService registry) ----

    // Balance: only offered when the user actually has a positive balance.
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
            amount: base_amount,
            currency: base_amount.map(|_| "USD".to_string()),
        });
    }

    // NOTE: Telegram Stars is NOT special-cased here any more. It used to be
    // listed off the `telegram_stars_enabled` setting alone while
    // MarketplaceService had no "stars" provider, so every Stars purchase from
    // the Mini App died with "Payment provider not found or disabled". Stars is
    // now a real registered provider and flows through the loop below, which
    // makes registration (credentials actually present) the source of truth for
    // what the app may advertise.

    // ---- Registered providers (source of truth = MarketplaceService) ----
    for name in state.marketplace_service.provider_names() {
        // "balance" is handled above with its dynamic label; never list it twice.
        if name == "balance" {
            continue;
        }
        // Honor the per-provider enable toggle the admin panel writes. Opt-in
        // providers (wata/tribute/…) default off; legacy providers default on once
        // configured. `provider_enable_setting` is the shared source of truth.
        let (enable_key, default_on) =
            crate::services::marketplace_service::provider_enable_setting(&name);
        let default = if default_on { "true" } else { "false" };
        if state.settings.get_or_default(&enable_key, default).await != "true" {
            continue;
        }

        let (amount, currency) = if q.order_id.is_some() {
            match state
                .catalog_service
                .resolve_order_price(q.order_id.unwrap_or(0), &name)
                .await
            {
                Ok(Some((a, c))) => (Some(a), Some(c)),
                _ => (base_amount, base_amount.map(|_| "USD".to_string())),
            }
        } else {
            price_for(&name)
        };

        providers.push(PaymentProviderInfo {
            id: name.clone(),
            label: provider_label(&name),
            amount,
            currency,
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

    // Resolve the effective amount + currency for this provider (per-method override
    // when configured, otherwise the base price in USD). For plan durations we also
    // carry `duration_days` so fulfillment extends the subscription by the purchased
    // term regardless of the (possibly overridden) charged amount.
    let (product_id, duration_days, amount, currency): (i64, Option<i32>, i64, String) =
        if let Some(duration_id) = body.duration_id {
            match state
                .catalog_service
                .resolve_duration_price(duration_id, &body.provider)
                .await
            {
                Ok(Some((plan_id, days, amt, cur))) => (plan_id, Some(days), amt, cur),
                Ok(None) => {
                    return (StatusCode::BAD_REQUEST, "Invalid duration ID").into_response();
                }
                Err(e) => {
                    tracing::error!("resolve_duration_price failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Price resolution failed")
                        .into_response();
                }
            }
        } else if let Some(order_id) = body.order_id {
            // Verify the order belongs to the requesting user before pricing it.
            let owns: Option<i64> =
                sqlx::query_scalar("SELECT id FROM orders WHERE id = $1 AND user_id = $2")
                    .bind(order_id)
                    .bind(u.id)
                    .fetch_optional(&state.pool)
                    .await
                    .unwrap_or(None);
            if owns.is_none() {
                return (StatusCode::BAD_REQUEST, "Invalid order ID").into_response();
            }
            match state
                .catalog_service
                .resolve_order_price(order_id, &body.provider)
                .await
            {
                Ok(Some((amt, cur))) => (order_id, None, amt, cur),
                Ok(None) => return (StatusCode::BAD_REQUEST, "Invalid order ID").into_response(),
                Err(e) => {
                    tracing::error!("resolve_order_price failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Price resolution failed")
                        .into_response();
                }
            }
        } else {
            return (StatusCode::BAD_REQUEST, "Missing duration_id or order_id").into_response();
        };

    let mut metadata = HashMap::new();
    if body.duration_id.is_some() {
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("plan".to_string()),
        );
        if let Some(days) = duration_days {
            metadata.insert("duration_days".to_string(), serde_json::Value::from(days));
        }
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
            &currency,
            Some(serde_json::to_value(metadata).unwrap_or_default()),
        )
        .await
    {
        Ok((session, invoice_payload)) => {
            if body.provider == "balance" {
                // Charge the wallet FIRST with an atomic, conditional deduction so a
                // user can never spend more than they hold (the `balance >= $1` guard
                // makes the row update a no-op when funds are insufficient). Only after
                // a successful charge do we fulfill; if fulfillment fails we refund.
                // Charge session.amount, not the caller's pre-resolved amount:
                // create_session may have applied the referee first-purchase discount,
                // and the wallet charge, the recorded session, and the referrer reward
                // base must all agree on the discounted figure.
                let charged = sqlx::query(
                    "UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
                )
                .bind(session.amount)
                .bind(u.id)
                .execute(&state.pool)
                .await;

                match charged {
                    Ok(res) if res.rows_affected() == 1 => {}
                    Ok(_) => {
                        let _ = state
                            .marketplace_service
                            .mark_session_failed(session.id)
                            .await;
                        return (StatusCode::BAD_REQUEST, "Insufficient balance").into_response();
                    }
                    Err(e) => {
                        tracing::error!("Balance charge failed for user {}: {}", u.id, e);
                        let _ = state
                            .marketplace_service
                            .mark_session_failed(session.id)
                            .await;
                        return (StatusCode::INTERNAL_SERVER_ERROR, "Balance charge failed")
                            .into_response();
                    }
                }

                if let Err(e) = state.marketplace_service.fulfill_payment(session.id).await {
                    tracing::error!("Immediate balance fulfillment failed: {}", e);
                    // Refund the charge we just made so the user isn't billed for nothing.
                    let _ = sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                        .bind(session.amount)
                        .bind(u.id)
                        .execute(&state.pool)
                        .await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Fulfillment failed: {}", e),
                    )
                        .into_response();
                }

                return Json(serde_json::json!({
                    "ok": true,
                    "invoice_url": "SUCCESS",
                    "provider": "balance",
                    "session_id": session.id,
                    "fulfilled": true
                }))
                .into_response();
            }

            // Внешний http(s)-чекаут: дублируем ссылку на оплату в личку бота
            // (fire-and-forget) — из Mini App удобнее оплачивать по кнопке в чате.
            // manual/balance сюда не попадают: у них invoice_url не http(s) либо
            // ветка выше. Stars исключаем ЯВНО по имени провайдера: их ссылка
            // (`https://t.me/$<slug>` из createInvoiceLink) выглядит как обычный
            // http(s)-чекаут, но открывается нативно через `WebApp.openInvoice`,
            // поэтому дубль в чат только сбивал бы с толку.
            // `delivered_via: bot` сообщает приложению, что ссылка отправлена в
            // чат и редирект не нужен.
            let delivered_via_bot = body.provider != "stars"
                && (invoice_payload.starts_with("http://")
                    || invoice_payload.starts_with("https://"))
                && !invoice_payload.contains("t.me/invoice");
            if delivered_via_bot {
                state
                    .marketplace_service
                    .notify_invoice_created(&u, &session, &invoice_payload);
            }

            Json(serde_json::json!({
                "ok": true,
                "invoice_url": invoice_payload,
                "provider": body.provider,
                "session_id": session.id,
                "delivered_via": if delivered_via_bot { Some("bot") } else { None },
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Invoice generation failed for user {}: {}", u.id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)).into_response()
        }
    }
}

/// GET /api/client/payment/session/{id} — статус платёжной сессии текущего
/// пользователя. Mini App поллит его после того, как ссылка на оплату ушла в
/// чат бота, чтобы показать успех без ручного обновления. Чужие/несуществующие
/// сессии (и кривые UUID) отвечают 404, не раскрывая, существует ли сессия.
async fn get_payment_session_status(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let session_id = match uuid::Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
    };

    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);
    let user_id = match user_id {
        Some(v) => v,
        None => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
    };

    match state
        .marketplace_service
        .session_repo
        .get_by_id(session_id)
        .await
    {
        Ok(Some(session)) if session.user_id == user_id => {
            Json(serde_json::json!({ "status": session.status })).into_response()
        }
        Ok(_) => (StatusCode::NOT_FOUND, "Session not found").into_response(),
        Err(e) => {
            tracing::error!("Payment session status lookup failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Session lookup failed").into_response()
        }
    }
}

#[derive(Deserialize)]
struct PinNodeReq {
    node_id: Option<i64>,
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
            .bind(variants_by_node_ip.keys().cloned().collect::<Vec<String>>())
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
                .update_subscription_node(sub_id, body.node_id)
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
        Ok(_) => {
            // Notify the referrer about new referral
            let referrer_tg_id: Option<i64> = sqlx::query_scalar(
                "SELECT u2.tg_id FROM users u1 JOIN users u2 ON u2.id = u1.referrer_id WHERE u1.id = $1"
            )
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);
            if let Some(ref_tg_id) = referrer_tg_id {
                // Язык реферрера, а не того, кто ввёл код.
                let lang = crate::bot::utils::lang_by_tg_id(&state, ref_tg_id).await;
                let payload = crate::bot_manager::NotificationPayload::plain(
                    crate::bot::translations::t(lang, "referral.new_referral_dm"),
                );
                let _ = state
                    .bot_manager
                    .send_rich_notification(ref_tg_id, payload)
                    .await;
            }

            Json(serde_json::json!({
                "ok": true,
                "message": "Referrer linked successfully",
            }))
            .into_response()
        }
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

    // Same filter as SubscriptionService::get_active_ips so the device list
    // here and the active_devices counter on /user/subscriptions agree.
    // Excludes infrastructure IPs (own nodes, frontend servers) which can
    // appear in leases when traffic transits relays.
    // COALESCE(NULLIF(display_name,''), device_name) — возвращает пользовательское имя если задано,
    // иначе авто-сгенерированное из User-Agent.
    let leases = sqlx::query_as::<_, DeviceLease>(
        r#"SELECT id,
                  COALESCE(NULLIF(display_name, ''), device_name) AS device_name,
                  last_ip, last_seen_at, first_seen_at
           FROM subscription_device_leases
           WHERE subscription_id = $1
             AND last_seen_at > NOW() - INTERVAL '15 minutes'
             AND last_ip <> '0.0.0.0'
             AND last_ip NOT IN (SELECT ip FROM nodes WHERE ip IS NOT NULL)
             AND last_ip NOT IN (SELECT ip_address FROM frontend_servers WHERE ip_address IS NOT NULL)
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
    let _ = sqlx::query(
        "DELETE FROM subscription_device_leases WHERE id = $1 AND subscription_id = $2",
    )
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

    // Actively close all subscription connections via Clash API so the kicked
    // device's TCP session is terminated immediately, not on the next 5-min poll.
    // Other devices will transparently re-establish; this is the cost of
    // sing-box not exposing per-IP-targeted connection close in our setup.
    // Spawned so the HTTP response returns fast.
    let conn_service = state.connection_service.clone();
    tokio::spawn(async move {
        if let Err(e) = conn_service.kill_subscription_connections(sub_id).await {
            tracing::warn!(sub_id, error = %e, "kill_subscription_connections after kick failed");
        }
    });

    let _ = crate::services::activity_service::ActivityService::log(
        &state.pool,
        "Device:Kicked",
        &format!(
            "User kicked device {} (IP {}) from sub #{}",
            device_id,
            mask_ip(&ip),
            sub_id
        ),
    )
    .await;

    Json(serde_json::json!({ "ok": true, "message": "Device disconnected" })).into_response()
}

// POST /api/client/subscription/{id}/devices/kill-all
// Отключает все активные устройства подписки одной командой.
async fn kill_all_subscription_devices(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Проверяем принадлежность подписки пользователю
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

    // Удаляем все lease-записи по подписке
    let deleted = sqlx::query_scalar::<_, i64>(
        "WITH del AS (DELETE FROM subscription_device_leases WHERE subscription_id = $1 RETURNING id) SELECT COUNT(*) FROM del",
    )
    .bind(sub_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    // Чистим legacy IP tracking
    let _ = sqlx::query("DELETE FROM subscription_ip_tracking WHERE subscription_id = $1")
        .bind(sub_id)
        .execute(&state.pool)
        .await;

    // Закрываем активные соединения через Clash API (неблокирующий spawn)
    let conn_service = state.connection_service.clone();
    tokio::spawn(async move {
        if let Err(e) = conn_service.kill_subscription_connections(sub_id).await {
            tracing::warn!(sub_id, error = %e, "kill_subscription_connections after kill-all failed");
        }
    });

    let _ = crate::services::activity_service::ActivityService::log(
        &state.pool,
        "Device:KilledAll",
        &format!("User killed all {} devices for sub #{}", deleted, sub_id),
    )
    .await;

    Json(serde_json::json!({ "ok": true, "disconnected": deleted })).into_response()
}

// PUT /api/client/subscription/{sub_id}/devices/{device_id}/name
// Устанавливает пользовательское имя устройства (display_name).
// device_name (авто из User-Agent) остаётся нетронутым как fallback.
async fn rename_subscription_device(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path((sub_id, device_id)): Path<(i64, i64)>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Проверяем принадлежность подписки пользователю
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

    // Извлекаем и валидируем имя из тела запроса
    let raw_name = match payload.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return (StatusCode::BAD_REQUEST, "Missing 'name' field").into_response(),
    };

    // Очищаем управляющие символы, обрезаем пробелы
    let name: String = raw_name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();

    if name.is_empty() {
        // Пустое имя — сбрасываем display_name, вернётся авто-имя
        let updated = sqlx::query_scalar::<_, i64>(
            "WITH upd AS (UPDATE subscription_device_leases SET display_name = NULL WHERE id = $1 AND subscription_id = $2 RETURNING id) SELECT COUNT(*) FROM upd",
        )
        .bind(device_id)
        .bind(sub_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

        if updated == 0 {
            return (StatusCode::NOT_FOUND, "Device not found").into_response();
        }
        return Json(serde_json::json!({ "ok": true })).into_response();
    }

    if name.chars().count() > 32 {
        return (StatusCode::BAD_REQUEST, "Name too long (max 32 chars)").into_response();
    }

    let updated = sqlx::query_scalar::<_, i64>(
        "WITH upd AS (UPDATE subscription_device_leases SET display_name = $1 WHERE id = $2 AND subscription_id = $3 RETURNING id) SELECT COUNT(*) FROM upd",
    )
    .bind(&name)
    .bind(device_id)
    .bind(sub_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    if updated == 0 {
        return (StatusCode::NOT_FOUND, "Device not found").into_response();
    }

    Json(serde_json::json!({ "ok": true })).into_response()
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

// ============================================================================
// Extend Subscription Endpoint
// POST /api/client/subscription/{id}/extend
// Тело: { "duration_id": i64 }
// Списывает средства с баланса и продлевает активную подписку пользователя.
// ============================================================================

#[derive(Deserialize)]
struct ExtendSubscriptionReq {
    duration_id: i64,
}

async fn extend_subscription(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(sub_id): Path<i64>,
    Json(body): Json<ExtendSubscriptionReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    // Получаем внутренний user_id по tg_id
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Проверяем, что подписка принадлежит этому пользователю
    let owner_check: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    match owner_check {
        None => return (StatusCode::NOT_FOUND, "Subscription not found").into_response(),
        Some(owner_id) if owner_id != user_id => {
            return (StatusCode::FORBIDDEN, "Subscription does not belong to you").into_response();
        }
        _ => {}
    }

    // Вызываем сервисный метод — он проверяет баланс, списывает и продлевает
    match state
        .store_service
        .extend_subscription(user_id, body.duration_id)
        .await
    {
        Ok(sub) => {
            let expires_at = sub.expires_at.to_rfc3339();
            tracing::info!(
                user_id,
                sub_id,
                duration_id = body.duration_id,
                expires_at = %expires_at,
                "Subscription extended"
            );
            Json(serde_json::json!({
                "ok": true,
                "subscription_id": sub.id,
                "expires_at": expires_at,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::warn!(user_id, sub_id, error = %e, "extend_subscription failed");
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

// ============================================================================
// Язык интерфейса
//
// Переключатель RU/EN в Mini App меняет только свой i18n и localStorage — то
// есть уведомления бота остались бы на прежнем языке. Эти два ручки
// синхронизируют выбор с сервером: `users.language_code` — единственный
// источник правды для бота, уведомлений и Mini App.
// ============================================================================

#[derive(Serialize, Deserialize)]
struct LanguagePref {
    /// "ru" | "en"
    language: String,
}

/// GET /api/client/user/language
///
/// Отдаёт РАЗРЕШЁННЫЙ язык (`users.language_code` → настройка
/// `default_language` → "ru"), а не сырое поле: приложению нужен тот же ответ,
/// который получит бот, иначе переключатель и уведомления разъедутся.
async fn client_get_language(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let stored: Option<String> =
        sqlx::query_scalar("SELECT language_code FROM users WHERE tg_id = $1")
            .bind(tg_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
            .flatten();

    let lang = crate::bot::translations::lang_for(&state.settings, stored.as_deref()).await;
    Json(LanguagePref {
        language: lang.as_str().to_string(),
    })
    .into_response()
}

/// PUT /api/client/user/language — тело `{"language":"ru"|"en"}`.
///
/// Неподдерживаемый код отклоняется (400), а не молча сохраняется: в
/// `language_code` должно лежать значение, которое резолвер точно понимает.
async fn client_set_language(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<LanguagePref>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);

    let Some(lang) = crate::bot::translations::Lang::parse(&body.language) else {
        return (StatusCode::BAD_REQUEST, "Unsupported language").into_response();
    };

    let updated = sqlx::query("UPDATE users SET language_code = $1 WHERE tg_id = $2")
        .bind(lang.as_str())
        .bind(tg_id)
        .execute(&state.pool)
        .await;

    match updated {
        Ok(res) if res.rows_affected() == 1 => Json(LanguagePref {
            language: lang.as_str().to_string(),
        })
        .into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, "User not found").into_response(),
        Err(e) => {
            tracing::error!(err = %e, tg_id, "Failed to persist user language");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save language").into_response()
        }
    }
}

// ============================================================================
// Helpers — получить user_id из JWT-токена
// ============================================================================

async fn resolve_user_id(state: &AppState, tg_id: i64) -> Option<i64> {
    sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None)
}

// ============================================================================
// Уведомления пользователя (inbox Mini App)
// ============================================================================

#[derive(Deserialize)]
struct NotifListQuery {
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// GET /api/client/notifications
async fn client_list_notifications(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Query(q): Query<NotifListQuery>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);
    let status_ref = q.status.as_deref();

    match state
        .notifications_svc
        .list(user_id, status_ref, limit, offset)
        .await
    {
        Ok(items) => Json(items).into_response(),
        Err(e) => {
            tracing::error!("client_list_notifications error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /api/client/notifications/unread-count
async fn client_notifications_unread_count(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.notifications_svc.unread_count(user_id).await {
        Ok(count) => Json(serde_json::json!({ "count": count })).into_response(),
        Err(e) => {
            tracing::error!("client_notifications_unread_count error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// POST /api/client/notifications/read-all
async fn client_notifications_read_all(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.notifications_svc.mark_all_read(user_id).await {
        Ok(count) => Json(serde_json::json!({ "count": count })).into_response(),
        Err(e) => {
            tracing::error!("client_notifications_read_all error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// POST /api/client/notifications/{id}/read
async fn client_notification_mark_read(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(notif_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.notifications_svc.mark_read(user_id, notif_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("client_notification_mark_read error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ============================================================================
// Настройки каналов уведомлений
// ============================================================================

#[derive(Deserialize, Serialize)]
struct NotifPrefItem {
    category: String,
    channel: String,
    enabled: bool,
}

/// GET /api/client/notification-preferences
async fn client_get_notif_prefs(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.notifications_svc.get_preferences(user_id).await {
        Ok(prefs) => Json(prefs).into_response(),
        Err(e) => {
            tracing::error!("client_get_notif_prefs error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// PUT /api/client/notification-preferences
async fn client_set_notif_prefs(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<Vec<NotifPrefItem>>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let prefs = body
        .into_iter()
        .map(
            |p| caramba_db::models::notifications::NotificationChannelPref {
                user_id,
                category: p.category,
                channel: p.channel,
                enabled: p.enabled,
            },
        )
        .collect();

    match state
        .notifications_svc
        .set_preferences(user_id, prefs)
        .await
    {
        Ok(_) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => {
            tracing::error!("client_set_notif_prefs error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ============================================================================
// Тикеты поддержки — клиентские эндпоинты
// ============================================================================

#[derive(Deserialize)]
struct CreateTicketReq {
    category: String,
    subject: String,
    body: String,
    related_payment_id: Option<i64>,
    related_subscription_id: Option<i64>,
}

#[derive(Deserialize)]
struct AddTicketMessageReq {
    body: String,
    #[serde(default)]
    attachment_ids: Vec<i64>,
}

/// GET /api/client/tickets
async fn client_list_tickets(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state.tickets_svc.list_user_tickets(user_id).await {
        Ok(tickets) => Json(tickets).into_response(),
        Err(e) => {
            tracing::error!("client_list_tickets error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /api/client/tickets/{id}
async fn client_get_ticket(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(ticket_id): Path<i64>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .tickets_svc
        .get_ticket(ticket_id, false, Some(user_id))
        .await
    {
        Ok((ticket, messages)) => {
            Json(serde_json::json!({ "ticket": ticket, "messages": messages })).into_response()
        }
        Err(e) => {
            use crate::services::tickets_service::TicketError;
            match e {
                TicketError::NotFound => {
                    (StatusCode::NOT_FOUND, "Ticket not found").into_response()
                }
                TicketError::Forbidden => (StatusCode::FORBIDDEN, "Access denied").into_response(),
                TicketError::Closed => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "Ticket is closed").into_response()
                }
                TicketError::Internal(_) => {
                    tracing::error!("client_get_ticket error: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
    }
}

/// POST /api/client/tickets
async fn client_create_ticket(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Json(body): Json<CreateTicketReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .tickets_svc
        .create_ticket(
            user_id,
            &body.category,
            &body.subject,
            &body.body,
            body.related_payment_id,
            body.related_subscription_id,
        )
        .await
    {
        Ok(ticket) => (StatusCode::CREATED, Json(ticket)).into_response(),
        Err(e) => {
            tracing::error!("client_create_ticket error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

/// POST /api/client/tickets/{id}/messages
async fn client_add_ticket_message(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(ticket_id): Path<i64>,
    Json(body): Json<AddTicketMessageReq>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .tickets_svc
        .add_user_message(ticket_id, user_id, &body.body, body.attachment_ids)
        .await
    {
        Ok(msg) => (StatusCode::CREATED, Json(msg)).into_response(),
        Err(e) => {
            use crate::services::tickets_service::TicketError;
            match e {
                TicketError::Forbidden => (StatusCode::FORBIDDEN, "Access denied").into_response(),
                TicketError::NotFound => {
                    (StatusCode::NOT_FOUND, "Ticket not found").into_response()
                }
                TicketError::Closed => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "Ticket is closed").into_response()
                }
                TicketError::Internal(_) => {
                    tracing::error!("client_add_ticket_message error: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Cannot reply to ticket").into_response()
                }
            }
        }
    }
}

/// POST /api/client/tickets/{id}/attach  — multipart upload
/// Возвращает 201 + {"attachment_id": N}
async fn client_attach_file(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path(ticket_id): Path<i64>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Проверяем права доступа к тикету
    let owner_id: Option<i64> = sqlx::query_scalar("SELECT user_id FROM tickets WHERE id = $1")
        .bind(ticket_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    match owner_id {
        None => return (StatusCode::NOT_FOUND, "Ticket not found").into_response(),
        Some(oid) if oid != user_id => {
            return (StatusCode::FORBIDDEN, "Access denied").into_response();
        }
        _ => {}
    }

    // Читаем первое поле multipart (файл)
    let field = match multipart.next_field().await {
        Ok(Some(f)) => f,
        Ok(None) => return (StatusCode::BAD_REQUEST, "No file provided").into_response(),
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let filename = field.file_name().unwrap_or("upload").to_string();

    let content_type = field.content_type().map(|s| s.to_string());

    let data = match field.bytes().await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    match state
        .tickets_svc
        .attach_file(ticket_id, None, &filename, content_type.as_deref(), &data)
        .await
    {
        Ok(att) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "attachment_id": att.id })),
        )
            .into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("слишком большой") || msg.contains("Недопустимый тип")
            {
                (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response()
            } else {
                tracing::error!("client_attach_file error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

/// GET /api/client/tickets/{id}/attachments/{attachment_id}
/// Streams the file with original mime type and Content-Disposition. The
/// service-layer canonicalises paths to prevent traversal — handler only
/// enforces ownership.
async fn client_download_attachment(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Claims>,
    Path((ticket_id, attachment_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let tg_id: i64 = claims.sub.parse().unwrap_or(0);
    let user_id = match resolve_user_id(&state, tg_id).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    match state
        .tickets_svc
        .verify_user_owns_ticket(ticket_id, user_id)
        .await
    {
        Ok(true) => {}
        Ok(false) => return (StatusCode::FORBIDDEN, "Access denied").into_response(),
        Err(e) => {
            tracing::error!("ownership check error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    }

    match state
        .tickets_svc
        .read_attachment(ticket_id, attachment_id)
        .await
    {
        Ok((meta, bytes)) => {
            let mime = meta
                .mime_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string());
            // RFC 5987 fallback for non-ASCII filenames.
            let safe_name = meta.filename.replace('"', "");
            let disposition = format!(
                "inline; filename=\"{}\"; filename*=UTF-8''{}",
                safe_name,
                urlencoding::encode(&meta.filename)
            );
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, mime),
                    (axum::http::header::CONTENT_DISPOSITION, disposition),
                    (
                        axum::http::header::CACHE_CONTROL,
                        "private, max-age=300".to_string(),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("не найдено") || msg.contains("не существует") {
                (StatusCode::NOT_FOUND, msg).into_response()
            } else {
                tracing::error!("download_attachment error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}
