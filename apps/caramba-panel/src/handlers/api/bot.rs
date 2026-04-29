use crate::services::store_service::PurchaseResult;
use crate::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

// Helper struct for bot verification (stub)
#[derive(Deserialize)]
pub struct VerifyUserRequest {
    pub telegram_id: i64,
}

#[derive(Serialize)]
pub struct VerifyUserResponse {
    pub verified: bool,
    pub user_id: Option<i64>,
    pub username: Option<String>,
}

pub async fn verify_user(
    State(state): State<AppState>,
    Json(payload): Json<VerifyUserRequest>,
) -> impl IntoResponse {
    let user: Option<caramba_db::models::store::User> = state
        .store_service
        .get_user_by_tg_id(payload.telegram_id)
        .await
        .unwrap_or(None);

    if let Some(user) = user {
        Json(VerifyUserResponse {
            verified: true,
            user_id: Some(user.id),
            username: user.username,
        })
    } else {
        Json(VerifyUserResponse {
            verified: false,
            user_id: None,
            username: None,
        })
    }
}

#[derive(Deserialize)]
pub struct UpsertUserRequest {
    pub tg_id: i64,
    pub username: Option<String>,
    pub full_name: Option<String>,
    pub referrer_id: Option<i64>,
}

pub async fn upsert_user(
    State(state): State<AppState>,
    Json(payload): Json<UpsertUserRequest>,
) -> impl IntoResponse {
    match state
        .store_service
        .upsert_user(
            payload.tg_id,
            payload.username.as_deref(),
            payload.full_name.as_deref(),
            payload.referrer_id,
        )
        .await
    {
        Ok(user) => (StatusCode::OK, Json(Some(user))).into_response(),
        Err(e) => {
            tracing::error!(
                "bot upsert_user failed for tg_id {}: {:?}",
                payload.tg_id,
                e
            );
            tracing::error!("bot upsert_user inner error: {:?}", e.source());
            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

pub async fn get_user_by_tg(
    State(state): State<AppState>,
    Path(tg_id): Path<i64>,
) -> impl IntoResponse {
    match state.store_service.get_user_by_tg_id(tg_id).await {
        Ok(user) => Json(user).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn resolve_referrer(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    match state.store_service.resolve_referrer_id(&code).await {
        Ok(opt_id) => Json(opt_id).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn get_plans(State(state): State<AppState>) -> impl IntoResponse {
    match state.catalog_service.get_active_plans().await {
        Ok(plans) => Json(plans).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_user_subs(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    match state
        .subscription_service
        .get_user_subscriptions(user_id)
        .await
    {
        Ok(subs) => Json(subs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_categories(State(state): State<AppState>) -> impl IntoResponse {
    match state.catalog_service.get_categories().await {
        Ok(cats) => Json(cats).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_products_by_category(
    State(state): State<AppState>,
    Path(category_id): Path<i64>,
) -> impl IntoResponse {
    match state
        .catalog_service
        .get_products_by_category(category_id)
        .await
    {
        Ok(prods) => Json(prods).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct PurchasePlanRequest {
    pub duration_id: i64,
    /// Если true — создаётся подарочный код вместо активной подписки
    #[serde(default)]
    pub as_gift: bool,
}

pub async fn purchase_plan(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(payload): Json<PurchasePlanRequest>,
) -> impl IntoResponse {
    match state
        .store_service
        .purchase_plan(user_id, payload.duration_id, payload.as_gift)
        .await
    {
        Ok(PurchaseResult::Subscription(sub)) => Json(serde_json::json!({
            "ok": true,
            "type": "subscription",
            "subscription_id": sub.id,
            "status": sub.status,
        }))
        .into_response(),
        Ok(PurchaseResult::GiftCode(code)) => Json(serde_json::json!({
            "ok": true,
            "type": "gift",
            "gift_code": code,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PurchaseProductRequest {
    pub product_id: i64,
}

pub async fn purchase_product(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(payload): Json<PurchaseProductRequest>,
) -> impl IntoResponse {
    match state
        .store_service
        .purchase_product_with_balance(user_id, payload.product_id)
        .await
    {
        Ok(prod) => Json(prod).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

/// Разрешённые ключи настроек, доступные боту через API.
/// Ключи с секретами (токены, API-ключи, пароли) здесь намеренно отсутствуют,
/// чтобы утечка PANEL_TOKEN не давала доступ ко всем секретам.
const BOT_SETTINGS_ALLOWLIST: &[&str] = &[
    "brand_name",
    "support_url",
    "referral_enabled",
    "referral_bonus_percent",
    "referral_signup_bonus_cents",
    "referred_signup_bonus_cents",
    "free_plan_enabled",
    "payment_testnet",
    "bot_status",
    "bot_username",
    "panel_url",
    "subscription_domain",
];

pub async fn get_settings(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    // Защита: разрешаем боту читать только безопасные ключи настроек.
    // Произвольный доступ к settings по ключу позволил бы читать
    // stripe_secret_key, bot_token и другие секреты.
    if !BOT_SETTINGS_ALLOWLIST.contains(&key.as_str()) {
        tracing::warn!(key = %key, "Bot API: settings key not in allowlist — rejected");
        return (StatusCode::FORBIDDEN, "Settings key not accessible via bot API").into_response();
    }
    let val = state.settings.get_or_default(&key, "").await;
    Json(Some(val)).into_response()
}

pub async fn get_sub_links(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    match state
        .subscription_service
        .get_subscription_links(sub_id)
        .await
    {
        Ok(links) => Json(links).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn activate_sub(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = payload.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0);
    match state
        .store_service
        .activate_subscription(sub_id, user_id)
        .await
    {
        Ok(sub) => Json(sub).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

// ============================================================================
// Бесплатная подписка
// ============================================================================

#[derive(Deserialize)]
pub struct CreateFreeSubscriptionRequest {
    pub user_id: i64,
}

#[derive(Serialize)]
pub struct CreateFreeSubscriptionResponse {
    pub subscription_id: i64,
    pub already_had_free: bool,
}

/// Автоматически выдаёт бесплатную подписку новому пользователю.
/// Идемпотентен: если активная бесплатная подписка уже существует — возвращает её ID.
pub async fn create_free_subscription(
    State(state): State<AppState>,
    Json(payload): Json<CreateFreeSubscriptionRequest>,
) -> impl IntoResponse {
    let user_id = payload.user_id;

    // Проверяем, что пользователь с таким ID реально существует в БД,
    // чтобы не создавать подписки-сироты для несуществующих user_id
    let user_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(false);

    if !user_exists {
        return (StatusCode::NOT_FOUND, "User not found").into_response();
    }

    // Находим план с is_free = TRUE
    let free_plan: Option<(i64, i32)> = sqlx::query_as(
        "SELECT id, COALESCE(traffic_limit_gb, 2) FROM plans WHERE is_free = TRUE AND is_active = TRUE LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let (plan_id, traffic_limit_gb) = match free_plan {
        Some(row) => row,
        None => {
            return (
                StatusCode::NOT_FOUND,
                "No active free plan configured",
            )
                .into_response()
        }
    };

    // Проверяем: уже есть активная подписка на этот план?
    let existing_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM subscriptions WHERE user_id = $1 AND plan_id = $2 AND status = 'active' LIMIT 1",
    )
    .bind(user_id)
    .bind(plan_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    if let Some(id) = existing_id {
        return Json(CreateFreeSubscriptionResponse {
            subscription_id: id,
            already_had_free: true,
        })
        .into_response();
    }

    // Создаём бесплатную подписку без даты истечения:
    // expires_at = далёкое будущее (year 9999 — никогда не истечёт через check_expirations).
    // Трафик ограничен traffic_limit_gb плана; ежедневное пополнение восстанавливает его.
    let sub_id: Result<i64, _> = sqlx::query_scalar(
        "INSERT INTO subscriptions
         (user_id, plan_id, status, expires_at, subscription_uuid, used_traffic, activated_at)
         VALUES ($1, $2, 'active', '9999-12-31 23:59:59+00', gen_random_uuid()::TEXT, 0, CURRENT_TIMESTAMP)
         RETURNING id",
    )
    .bind(user_id)
    .bind(plan_id)
    .fetch_one(&state.pool)
    .await;

    match sub_id {
        Ok(id) => {
            tracing::info!(
                user_id,
                plan_id,
                traffic_limit_gb,
                subscription_id = id,
                "Бесплатная подписка создана"
            );
            Json(CreateFreeSubscriptionResponse {
                subscription_id: id,
                already_had_free: false,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create free subscription: {}", e),
        )
            .into_response(),
    }
}

// ============================================================================
// Admin Bot Endpoints
// ============================================================================

pub async fn admin_check(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let tg_id = payload.get("tg_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let is_admin = crate::bot_manager::BotManager::is_admin_tg_id(&state.pool, tg_id).await;
    Json(serde_json::json!({ "is_admin": is_admin }))
}

pub async fn admin_stats(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.analytics_service.get_system_stats().await;
    match stats {
        Ok(s) => {
            let traffic_30d_gb = s.total_traffic_30d_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            Json(serde_json::json!({
                "active_nodes": s.active_nodes,
                "total_users": s.total_users,
                "active_subs": s.active_subs,
                "total_revenue": s.total_revenue,
                "traffic_30d_gb": traffic_30d_gb,
            })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn admin_gift(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let username = payload.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let days = payload.get("days").and_then(|v| v.as_i64()).unwrap_or(0);

    // Ограничиваем days разумным диапазоном: 1..3650 (10 лет макс.)
    if username.is_empty() || days <= 0 || days > 3650 {
        return (StatusCode::BAD_REQUEST, "Invalid username or days (1-3650)").into_response();
    }

    // Find user by username
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user_id = match user_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // Find default plan
    let plan_id: Option<i64> = sqlx::query_scalar("SELECT id FROM plans WHERE is_active = TRUE ORDER BY id LIMIT 1")
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let plan_id = match plan_id {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "No active plan found").into_response(),
    };

    // Create subscription
    let sub_id: Result<i64, _> = sqlx::query_scalar(
        "INSERT INTO subscriptions (user_id, plan_id, status, duration_days, expires_at, subscription_uuid) \
         VALUES ($1, $2, 'pending', $3, CURRENT_TIMESTAMP + ($3 || ' days')::INTERVAL, gen_random_uuid()::TEXT) RETURNING id"
    )
    .bind(user_id)
    .bind(plan_id)
    .bind(days)
    .fetch_one(&state.pool)
    .await;

    match sub_id {
        Ok(id) => Json(serde_json::json!({ "subscription_id": id })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn admin_ban(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let username = payload.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let result = sqlx::query("UPDATE users SET is_banned = TRUE WHERE username = $1")
        .bind(username)
        .execute(&state.pool)
        .await;
    match result {
        Ok(r) if r.rows_affected() > 0 => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, "User not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn admin_unban(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let username = payload.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let result = sqlx::query("UPDATE users SET is_banned = FALSE WHERE username = $1")
        .bind(username)
        .execute(&state.pool)
        .await;
    match result {
        Ok(r) if r.rows_affected() > 0 => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, "User not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn admin_list_promos(State(state): State<AppState>) -> impl IntoResponse {
    let promos = sqlx::query_as::<_, (String, i64)>(
        "SELECT code, COALESCE(used_count, 0) as use_count FROM promo_codes WHERE is_active = TRUE ORDER BY created_at DESC LIMIT 20"
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = promos.iter().map(|(code, count)| {
        serde_json::json!({ "code": code, "use_count": count })
    }).collect();

    Json(items)
}

pub async fn admin_create_promo(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let code = payload.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let promo_type = payload.get("promo_type").and_then(|v| v.as_str()).unwrap_or("balance");
    let value = payload.get("value").and_then(|v| v.as_i64()).unwrap_or(0);

    if code.is_empty() || value <= 0 {
        return (StatusCode::BAD_REQUEST, "Invalid code or value").into_response();
    }

    let result = sqlx::query(
        "INSERT INTO promo_codes (code, promo_type, value, is_active) VALUES ($1, $2, $3, TRUE)"
    )
    .bind(code)
    .bind(promo_type)
    .bind(value)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

/// POST /api/v2/bot/referral/signup-bonus
/// Начисляет бонусы за регистрацию по реферальной ссылке.
/// Вызывается ботом после успешной привязки реферрера к новому пользователю.
pub async fn referral_signup_bonus(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let referrer_id = match payload.get("referrer_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Missing referrer_id").into_response(),
    };
    let referred_user_id = match payload.get("referred_user_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Missing referred_user_id").into_response(),
    };

    use crate::services::referral_service::ReferralService;
    match ReferralService::apply_signup_bonus(&state.pool, referrer_id, referred_user_id).await {
        Ok((referrer_bonus, referred_bonus)) => {
            // Уведомляем реферрера о бонусе
            if referrer_bonus > 0 {
                let tg_id: Option<i64> =
                    sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                        .bind(referrer_id)
                        .fetch_optional(&state.pool)
                        .await
                        .unwrap_or(None);
                if let Some(tid) = tg_id {
                    let amount = format!("{:.2}", referrer_bonus as f64 / 100.0);
                    let msg = format!(
                        "🎉 *Referral Bonus*\n\nYour friend joined via your referral link\\! \\+*${}* added to your balance\\.",
                        amount
                    );
                    let _ = state.bot_manager.send_notification(tid, &msg).await;
                }
            }

            // Уведомляем нового пользователя о welcome-бонусе
            if referred_bonus > 0 {
                let tg_id: Option<i64> =
                    sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                        .bind(referred_user_id)
                        .fetch_optional(&state.pool)
                        .await
                        .unwrap_or(None);
                if let Some(tid) = tg_id {
                    let amount = format!("{:.2}", referred_bonus as f64 / 100.0);
                    let msg = format!(
                        "🎁 *Welcome Bonus*\n\n\\+*${}* has been added to your balance as a referral welcome gift\\!",
                        amount
                    );
                    let _ = state.bot_manager.send_notification(tid, &msg).await;
                }
            }

            Json(serde_json::json!({
                "ok": true,
                "referrer_bonus_cents": referrer_bonus,
                "referred_bonus_cents": referred_bonus,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to apply signup bonus: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to apply signup bonus").into_response()
        }
    }
}
