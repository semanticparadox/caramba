use crate::AppState;
use crate::services::store_service::PurchaseResult;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        .upsert_user_with_new_flag(
            payload.tg_id,
            payload.username.as_deref(),
            payload.full_name.as_deref(),
            payload.referrer_id,
        )
        .await
    {
        Ok((user, was_new)) => {
            // First-touch onboarding: fire a welcome notification exactly once,
            // when this user row is brand-new. Soft-fail — sign-up must not be
            // blocked by a notification glitch.
            if was_new {
                let svc = state.notifications_svc.clone();
                let display_name = user
                    .full_name
                    .clone()
                    .or_else(|| user.username.clone())
                    .unwrap_or_else(|| "there".to_string());
                let user_id = user.id;
                // Раньше пустой language_code давал английский; теперь работает
                // общий порядок: language_code → default_language → ru.
                let lang = crate::bot::translations::lang_for(
                    &state.settings,
                    user.language_code.as_deref(),
                )
                .await;
                let title = crate::bot::translations::t(lang, "welcome.notif_title").to_string();
                let body =
                    crate::bot::translations::tf(lang, "welcome.notif_body", &[&display_name]);
                tokio::spawn(async move {
                    let _ = svc
                        .create(
                            user_id,
                            "system_maintenance",
                            "info",
                            &title,
                            &body,
                            Some(serde_json::json!({"url": "/"})),
                        )
                        .await;
                });
            }
            (StatusCode::OK, Json(Some(user))).into_response()
        }
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
    // Инструкции по подключению (Telegraph) — читаются ботом и мини-приложением.
    "guide_url_index",
    "guide_url_ios",
    "guide_url_android",
    "guide_url_windows",
    "guide_url_macos",
    "guide_url_linux",
    "guide_url_tv",
    "guide_url_router",
    // Brand k/v (contract B) — the six canonical brand_* keys the bot reads back
    // to render its brand admin menu and the panel branding endpoint serves.
    "brand_enabled",
    "brand_name",
    "brand_logo_url",
    "brand_accent_hex",
    "brand_support_url",
    "brand_bot_url",
    // Legacy/other read-only keys kept for backward compatibility.
    "support_url",
    "referral_enabled",
    "referral_bonus_percent",
    "referral_signup_bonus_cents",
    "referred_signup_bonus_cents",
    "payment_testnet",
    "bot_status",
    "bot_username",
    "panel_url",
    "subscription_domain",
];

/// Ключи, в которые боту РАЗРЕШЕНО писать через POST /settings/{key}.
/// Строго шесть brand_*-ключей контракта B — никаких секретов, никаких
/// прочих настроек панели. Запись в любой другой ключ → 403.
const BOT_SETTINGS_WRITE_ALLOWLIST: &[&str] = &[
    "brand_enabled",
    "brand_name",
    "brand_logo_url",
    "brand_accent_hex",
    "brand_support_url",
    "brand_bot_url",
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
        return (
            StatusCode::FORBIDDEN,
            "Settings key not accessible via bot API",
        )
            .into_response();
    }
    let val = state.settings.get_or_default(&key, "").await;
    Json(Some(val)).into_response()
}

#[derive(Deserialize)]
pub struct SetSettingRequest {
    pub value: String,
}

/// POST /api/v2/bot/settings/{key}
/// Записывает значение настройки от бота. Контракт B (общий k/v): бот пишет
/// brand_*-ключи, панель и клиент их читают.
///
/// Защита: разрешаем боту писать ТОЛЬКО шесть brand_*-ключей. Запись в любой
/// другой ключ (секреты, тарифы, флаги системы) отвергается 403 — даже если
/// PANEL_TOKEN утёк, боту нельзя перезаписать произвольную настройку.
///
/// `state.settings.set` пишет в БД И обновляет in-memory cache, поэтому
/// branding-эндпоинт сразу отражает изменение без рестарта панели.
pub async fn set_settings(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(payload): Json<SetSettingRequest>,
) -> impl IntoResponse {
    if !BOT_SETTINGS_WRITE_ALLOWLIST.contains(&key.as_str()) {
        tracing::warn!(key = %key, "Bot API: settings write key not in allowlist — rejected");
        return (
            StatusCode::FORBIDDEN,
            "Settings key not writable via bot API",
        )
            .into_response();
    }

    match state.settings.set(&key, &payload.value).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => {
            tracing::error!(key = %key, "Bot API: settings write failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to persist setting",
            )
                .into_response()
        }
    }
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
    let user_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
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
        None => return (StatusCode::NOT_FOUND, "No active free plan configured").into_response(),
    };

    // Проверяем: уже есть подписка на этот план? 'throttled' (суточная
    // квота исчерпана) и 'pending' (ждёт одобрения) тоже считаются —
    // иначе затроттленный юзер повторным нажатием кнопки получал бы
    // свежую бесплатную подписку в обход суточного лимита.
    let existing_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM subscriptions WHERE user_id = $1 AND plan_id = $2 AND status IN ('active', 'pending', 'throttled') LIMIT 1",
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
            }))
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn admin_gift(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let username = payload
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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
    let plan_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM plans WHERE is_active = TRUE ORDER BY id LIMIT 1")
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
    let username = payload
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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
    let username = payload
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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

    let items: Vec<serde_json::Value> = promos
        .iter()
        .map(|(code, count)| serde_json::json!({ "code": code, "use_count": count }))
        .collect();

    Json(items)
}

pub async fn admin_create_promo(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let code = payload.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let promo_type = payload
        .get("promo_type")
        .and_then(|v| v.as_str())
        .unwrap_or("balance");
    let value = payload.get("value").and_then(|v| v.as_i64()).unwrap_or(0);

    if code.is_empty() || value <= 0 {
        return (StatusCode::BAD_REQUEST, "Invalid code or value").into_response();
    }

    let result = sqlx::query(
        "INSERT INTO promo_codes (code, promo_type, value, is_active) VALUES ($1, $2, $3, TRUE)",
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
                    let lang = crate::bot::utils::lang_by_tg_id(&state, tid).await;
                    let amount = format!("{:.2}", referrer_bonus as f64 / 100.0);
                    let msg = crate::bot::translations::tf(lang, "referral.bonus_dm", &[&amount]);
                    let _ = state
                        .bot_manager
                        .send_rich_notification(
                            tid,
                            crate::bot_manager::NotificationPayload::html(msg),
                        )
                        .await;
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
                    let lang = crate::bot::utils::lang_by_tg_id(&state, tid).await;
                    let amount = format!("{:.2}", referred_bonus as f64 / 100.0);
                    let msg =
                        crate::bot::translations::tf(lang, "referral.welcome_bonus_dm", &[&amount]);
                    let _ = state
                        .bot_manager
                        .send_rich_notification(
                            tid,
                            crate::bot_manager::NotificationPayload::html(msg),
                        )
                        .await;
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to apply signup bonus",
            )
                .into_response()
        }
    }
}

// ============================================================================
// Корзина и чекаут
// ============================================================================

/// POST /api/v2/bot/users/{user_id}/checkout-cart
/// Оплачивает содержимое корзины пользователя, списывая баланс и создавая заказ.
/// Идемпотентность: если корзина пуста — 422 Unprocessable Entity (не 500).
pub async fn checkout_cart(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    // Проверяем, что пользователь существует, чтобы избежать создания заказа-сироты
    let user_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !user_exists {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "ok": false,
                "error": "user_not_found",
                "message": "User not found"
            })),
        )
            .into_response();
    }

    match state.store_service.checkout_cart(user_id).await {
        Ok(messages) => Json(serde_json::json!({
            "ok": true,
            "messages": messages,
        }))
        .into_response(),
        Err(e) => {
            let msg = e.to_string();
            // Отличаем бизнес-ошибки (пустая корзина, нет денег) от системных сбоев,
            // чтобы бот мог показать пользователю понятный текст.
            let (status, error_code) = if msg.contains("Cart is empty") {
                (StatusCode::UNPROCESSABLE_ENTITY, "cart_empty")
            } else if msg.contains("Insufficient balance") {
                (StatusCode::PAYMENT_REQUIRED, "insufficient_balance")
            } else {
                tracing::error!(user_id, "checkout_cart failed: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            };
            (
                status,
                Json(serde_json::json!({
                    "ok": false,
                    "error": error_code,
                    "message": msg,
                })),
            )
                .into_response()
        }
    }
}

/// DELETE /api/v2/bot/users/{user_id}/cart
/// Очищает корзину пользователя. Идемпотентен: повторный вызов на пустой корзине возвращает 200.
pub async fn clear_cart(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    match state.store_service.clear_cart(user_id).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => {
            tracing::error!(user_id, "clear_cart failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "ok": false,
                    "error": "internal_error",
                    "message": e.to_string(),
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// Сессии подписки
// ============================================================================

/// POST /api/v2/bot/subs/{sub_id}/kill-sessions
/// Принудительно сбрасывает все активные сессии подписки, удаляя записи IP-трекинга.
/// Ожидаемый эффект: клиенты должны повторно загрузить конфигурацию (новый запрос к /sub/uuid).
/// Тело запроса: { "user_id": i64 } — обязательно для проверки владельца подписки.
///
/// Архитектурная заметка: в текущей схеме нет серверной таблицы сессий с push-инвалидацией.
/// Sing-box core на узле не поддерживает принудительный разрыв соединения без перезагрузки конфига.
/// Удаление строк из subscription_ip_tracking освобождает слоты устройств так, что при
/// следующем pull клиента новый IP будет принят как «первый» и не заблокирован лимитом устройств.
pub async fn kill_subscription_sessions(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = match payload.get("user_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "ok": false,
                    "error": "missing_user_id",
                    "message": "Request body must contain user_id"
                })),
            )
                .into_response();
        }
    };

    // Проверяем, что подписка принадлежит указанному пользователю.
    // Это предотвращает использование эндпоинта для чужих подписок через подбор sub_id.
    let owner_check: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    match owner_check {
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "ok": false,
                    "error": "subscription_not_found",
                    "message": "Subscription not found"
                })),
            )
                .into_response();
        }
        Some(owner_id) if owner_id != user_id => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "ok": false,
                    "error": "forbidden",
                    "message": "Subscription does not belong to this user"
                })),
            )
                .into_response();
        }
        _ => {}
    }

    // 1. Drop legacy IP tracking rows so the next connection re-authenticates.
    if let Err(e) = state
        .store_service
        .kill_subscription_connections(sub_id)
        .await
    {
        tracing::error!(
            sub_id,
            "kill_subscription_sessions (ip tracking) failed: {}",
            e
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "ok": false,
                "error": "internal_error",
                "message": e.to_string(),
            })),
        )
            .into_response();
    }

    // 2. Force-disconnect every active TCP session through the Clash API on
    //    each node — without this the lost/hostile device the user is trying
    //    to kick stays online with full traffic until its tunnel naturally
    //    terminates. Mirrors the Mini App kill-all path in api/client.rs.
    let conn_service = state.connection_service.clone();
    tokio::spawn(async move {
        if let Err(e) = conn_service.kill_subscription_connections(sub_id).await {
            tracing::warn!(sub_id, error = %e, "kill_subscription_connections (clash api) failed");
        }
    });

    Json(serde_json::json!({
        "ok": true,
        "message": "Active sessions cleared. Clients must re-authenticate on next connect.",
    }))
    .into_response()
}

// ============================================================================
// Конфиг-файл для отдельной подписки
// ============================================================================

#[derive(Deserialize)]
pub struct SubConfigParams {
    pub user_id: i64,
    /// Тип клиента: "singbox" | "v2ray" | "clash" (по умолчанию "singbox")
    pub client: Option<String>,
}

/// GET /api/v2/bot/subs/{sub_id}/config-file?user_id={uid}&client={type}
/// Возвращает конфигурацию sing-box/v2ray/clash только для одной подписки.
/// Бот использует это вместо полного профиля пользователя, чтобы не утечить
/// ссылки всех подписок в один конфиг.
pub async fn get_sub_config_file(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    Query(params): Query<SubConfigParams>,
) -> impl IntoResponse {
    let user_id = params.user_id;

    // Загружаем подписку и проверяем владельца
    let sub = match state.subscription_service.get_by_id(sub_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Subscription not found").into_response();
        }
        Err(e) => {
            tracing::error!(sub_id, "get_sub_config_file: db error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Проверка владельца — бот передаёт user_id, мы проверяем соответствие
    if sub.user_id != user_id {
        return (
            StatusCode::FORBIDDEN,
            "Subscription does not belong to this user",
        )
            .into_response();
    }

    // Подписка должна быть активной
    if sub.status != "active" {
        return (StatusCode::FORBIDDEN, "Subscription is not active").into_response();
    }

    // Получаем ключи пользователя
    let user_keys = match state.subscription_service.get_user_keys(&sub).await {
        Ok(k) => k,
        Err(e) => {
            tracing::error!(sub_id, "get_sub_config_file: get_user_keys failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get user keys").into_response();
        }
    };

    // Получаем узлы для подписки (через план пользователя, как в /sub/:uuid)
    let nodes_raw = match state.store_service.get_user_nodes(sub.user_id).await {
        Ok(nodes) if !nodes.is_empty() => nodes,
        _ => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
            }
        },
    };

    // Фильтруем: только exit-узлы (не relay-инфраструктура) и только закреплённый узел подписки (если есть)
    let mut filtered = nodes_raw
        .into_iter()
        .filter(|n| !n.is_relay)
        .collect::<Vec<_>>();

    if let Some(pinned_node) = sub.node_id {
        let pinned: Vec<_> = filtered
            .iter()
            .filter(|n| n.id == pinned_node)
            .cloned()
            .collect();
        if !pinned.is_empty() {
            filtered = pinned;
        }
    }

    if filtered.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "No servers available for this subscription",
        )
            .into_response();
    }

    let node_infos = match state
        .subscription_service
        .get_node_infos_with_relays(&filtered)
        .await
    {
        Ok(infos) => infos,
        Err(e) => {
            tracing::error!(sub_id, "get_sub_config_file: get_node_infos failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to process nodes").into_response();
        }
    };

    // Relay-узлы для гео-цепочек (без фильтрации по стране — бот не знает страну клиента)
    let relay_nodes = state
        .subscription_service
        .get_all_active_relay_infos()
        .await
        .unwrap_or_default();

    let client_type = match params.client.as_deref().unwrap_or("singbox") {
        "hiddify" => "singbox",
        other => other,
    };

    match client_type {
        "clash" => {
            match state.subscription_service.generate_clash(
                &sub,
                &node_infos,
                &user_keys,
                &relay_nodes,
            ) {
                Ok(content) => (
                    StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "text/yaml; charset=utf-8")],
                    content,
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!(sub_id, "clash generation failed: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Config generation failed",
                    )
                        .into_response()
                }
            }
        }
        "v2ray" => {
            match state.subscription_service.generate_v2ray(
                &sub,
                &node_infos,
                &user_keys,
                &relay_nodes,
            ) {
                Ok(content) => (
                    StatusCode::OK,
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/plain; charset=utf-8",
                    )],
                    content,
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!(sub_id, "v2ray generation failed: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Config generation failed",
                    )
                        .into_response()
                }
            }
        }
        _ => {
            // По умолчанию — singbox JSON
            match state.subscription_service.generate_singbox(
                &sub,
                &node_infos,
                &user_keys,
                None,
                &relay_nodes,
            ) {
                Ok(content) => (
                    StatusCode::OK,
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "application/json; charset=utf-8",
                    )],
                    content,
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!(sub_id, "singbox generation failed: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Config generation failed",
                    )
                        .into_response()
                }
            }
        }
    }
}

// ============================================================================
// Bot API — Тикеты поддержки
// ============================================================================

#[derive(Deserialize)]
pub struct TicketListQuery {
    pub status: Option<String>,
    pub assignee_tg_id: Option<i64>,
    #[serde(default = "default_ticket_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_ticket_limit() -> i64 {
    20
}

/// GET /api/v2/bot/tickets  — список тикетов для бота
pub async fn bot_list_tickets(
    State(state): State<AppState>,
    Query(q): Query<TicketListQuery>,
) -> impl IntoResponse {
    let limit = q.limit.clamp(1, 100);
    let offset = q.offset.max(0);

    match state
        .tickets_svc
        .list_admin_tickets(q.status.as_deref(), q.assignee_tg_id, limit, offset)
        .await
    {
        Ok(tickets) => Json(tickets).into_response(),
        Err(e) => {
            tracing::error!("bot_list_tickets error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /api/v2/bot/tickets/{id}
pub async fn bot_get_ticket(
    State(state): State<AppState>,
    Path(ticket_id): Path<i64>,
) -> impl IntoResponse {
    match state.tickets_svc.get_ticket(ticket_id, true, None).await {
        Ok((ticket, messages)) => {
            // Получаем данные пользователя для расширенного ответа
            let user_row: Option<(Option<String>, Option<i64>)> =
                sqlx::query_as("SELECT username, tg_id FROM users WHERE id = $1")
                    .bind(ticket.user_id)
                    .fetch_optional(&state.pool)
                    .await
                    .unwrap_or(None);

            let (username, tg_id) = user_row.unwrap_or((None, None));

            Json(serde_json::json!({
                "ticket": ticket,
                "user": { "username": username, "tg_id": tg_id },
                "messages": messages
            }))
            .into_response()
        }
        Err(e) => {
            use crate::services::tickets_service::TicketError;
            match e {
                TicketError::NotFound => {
                    (StatusCode::NOT_FOUND, "Ticket not found").into_response()
                }
                // is_admin=true: Forbidden/Closed здесь не возникают, но мапим
                // на безопасные коды ради полноты match.
                TicketError::Forbidden => (StatusCode::FORBIDDEN, "Access denied").into_response(),
                TicketError::Closed => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "Ticket is closed").into_response()
                }
                TicketError::Internal(_) => {
                    tracing::error!("bot_get_ticket error: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
    }
}

#[derive(Deserialize)]
pub struct BotAddMessageReq {
    pub admin_tg_id: i64,
    pub body: String,
}

/// POST /api/v2/bot/tickets/{id}/messages
pub async fn bot_add_ticket_message(
    State(state): State<AppState>,
    Path(ticket_id): Path<i64>,
    Json(body): Json<BotAddMessageReq>,
) -> impl IntoResponse {
    match state
        .tickets_svc
        .add_admin_message(ticket_id, body.admin_tg_id, &body.body, vec![])
        .await
    {
        Ok(msg) => (StatusCode::CREATED, Json(msg)).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("не найден") {
                (StatusCode::NOT_FOUND, msg).into_response()
            } else {
                tracing::error!("bot_add_ticket_message error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

#[derive(Deserialize)]
pub struct BotAssignReq {
    pub admin_tg_id: i64,
}

/// POST /api/v2/bot/tickets/{id}/assign
pub async fn bot_assign_ticket(
    State(state): State<AppState>,
    Path(ticket_id): Path<i64>,
    Json(body): Json<BotAssignReq>,
) -> impl IntoResponse {
    match state.tickets_svc.assign(ticket_id, body.admin_tg_id).await {
        Ok(ticket) => Json(ticket).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("не найден") {
                (StatusCode::NOT_FOUND, msg).into_response()
            } else {
                tracing::error!("bot_assign_ticket error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

#[derive(Deserialize)]
pub struct BotSetStatusReq {
    pub admin_tg_id: i64,
    pub status: String,
}

/// POST /api/v2/bot/tickets/{id}/status
pub async fn bot_set_ticket_status(
    State(state): State<AppState>,
    Path(ticket_id): Path<i64>,
    Json(body): Json<BotSetStatusReq>,
) -> impl IntoResponse {
    match state
        .tickets_svc
        .set_status(ticket_id, &body.status, body.admin_tg_id)
        .await
    {
        Ok(ticket) => Json(ticket).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("не найден") {
                (StatusCode::NOT_FOUND, msg).into_response()
            } else if msg.contains("Недопустимый статус") {
                (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response()
            } else {
                tracing::error!("bot_set_ticket_status error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

// ============================================================================
// Bot API — Broadcast уведомлений
// ============================================================================

#[derive(Deserialize)]
pub struct BroadcastReq {
    pub category: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub payload: Option<Value>,
    #[serde(default = "default_segment")]
    pub segment: String,
}

fn default_segment() -> String {
    "all".to_string()
}

/// GET /api/v2/bot/tickets/{id}/attachments/{attachment_id}
/// Streams the attachment back to the bot. Auth is X-Bot-Token + the bot
/// itself must enforce its own admin gate before fetching — there's no
/// per-admin tg_id check at this layer (any caller with the bot token
/// is trusted).
pub async fn bot_download_attachment(
    State(state): State<AppState>,
    axum::extract::Path((ticket_id, attachment_id)): axum::extract::Path<(i64, i64)>,
) -> impl IntoResponse {
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
            let disposition = format!(
                "inline; filename=\"{}\"; filename*=UTF-8''{}",
                meta.filename.replace('"', ""),
                urlencoding::encode(&meta.filename)
            );
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, mime),
                    (axum::http::header::CONTENT_DISPOSITION, disposition),
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
                tracing::error!("bot_download_attachment error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}

/// POST /api/v2/bot/notifications/broadcast
/// Создаёт уведомления для всех пользователей выбранного сегмента.
pub async fn bot_broadcast_notification(
    State(state): State<AppState>,
    Json(body): Json<BroadcastReq>,
) -> impl IntoResponse {
    match state
        .notifications_svc
        .broadcast(
            &body.category,
            &body.severity,
            &body.title,
            &body.body,
            body.payload,
            &body.segment,
        )
        .await
    {
        Ok(queued) => Json(serde_json::json!({ "queued": queued })).into_response(),
        Err(e) => {
            tracing::error!("bot_broadcast_notification error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
