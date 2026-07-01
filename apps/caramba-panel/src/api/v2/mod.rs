pub mod app;
pub mod app_account;
pub mod app_auth;
pub mod app_billing;
pub mod app_branding;
pub mod app_enroll;
pub mod app_partner;
pub mod app_support;
pub mod bot_auth;
pub mod bot_rate_limit;
pub mod client;
pub mod node;

use crate::handlers;
use crate::AppState;
use axum::routing::{delete, get, patch, post};

/// Защищённый роутер для всех /api/v2/bot/* маршрутов бота.
///
/// Принимает `state` явно, чтобы передать его в `from_fn_with_state` для
/// `bot_rate_limit` — middleware требует `AppState` для доступа к RedisService.
///
/// Стек middleware (в порядке выполнения при входящем запросе, снаружи внутрь):
///   1. `require_bot_token` — проверяет X-Bot-Token; неавторизованные запросы
///      отсекаются до проверки rate limit (экономим Redis RTT).
///   2. `bot_rate_limit`   — ограничивает частоту через Redis:
///        • per-endpoint лимиты для дорогих операций
///        • глобальный лимит 50 req / 3 сек
///
/// В Axum 0.8: route_layer добавляет слои снаружи (LIFO), поэтому последний
/// `.route_layer()` в коде — первый выполняется.
pub fn bot_routes(state: AppState) -> axum::Router<AppState> {
    axum::Router::new()
        .route("/verify", post(handlers::api::bot::verify_user))
        .route("/users", post(handlers::api::bot::upsert_user))
        .route("/users/tg/{tg_id}", get(handlers::api::bot::get_user_by_tg))
        .route(
            "/referrers/resolve/{code}",
            get(handlers::api::bot::resolve_referrer),
        )
        .route("/users/{id}/subs", get(handlers::api::bot::get_user_subs))
        .route("/plans", get(handlers::api::bot::get_plans))
        .route(
            "/store/categories",
            get(handlers::api::bot::get_categories),
        )
        .route(
            "/store/categories/{id}/products",
            get(handlers::api::bot::get_products_by_category),
        )
        .route(
            "/users/{id}/purchase-plan",
            post(handlers::api::bot::purchase_plan),
        )
        .route(
            "/users/{id}/purchase-product",
            post(handlers::api::bot::purchase_product),
        )
        .route(
            "/settings/{key}",
            get(handlers::api::bot::get_settings).post(handlers::api::bot::set_settings),
        )
        .route("/subs/{id}/links", get(handlers::api::bot::get_sub_links))
        .route("/subs/{id}/activate", post(handlers::api::bot::activate_sub))
        // Бесплатная подписка
        .route(
            "/users/create-free-subscription",
            post(handlers::api::bot::create_free_subscription),
        )
        // Административные эндпоинты бота
        .route("/admin/check", post(handlers::api::bot::admin_check))
        .route("/admin/stats", get(handlers::api::bot::admin_stats))
        .route("/admin/gift", post(handlers::api::bot::admin_gift))
        .route("/admin/ban", post(handlers::api::bot::admin_ban))
        .route("/admin/unban", post(handlers::api::bot::admin_unban))
        .route(
            "/admin/promos",
            get(handlers::api::bot::admin_list_promos).post(handlers::api::bot::admin_create_promo),
        )
        // Бонусы за регистрацию по реферальной ссылке
        .route(
            "/referral/signup-bonus",
            post(handlers::api::bot::referral_signup_bonus),
        )
        // Корзина: оплата и очистка
        .route(
            "/users/{id}/checkout-cart",
            post(handlers::api::bot::checkout_cart),
        )
        .route(
            "/users/{id}/cart",
            axum::routing::delete(handlers::api::bot::clear_cart),
        )
        // Сессии подписки
        .route(
            "/subs/{id}/kill-sessions",
            post(handlers::api::bot::kill_subscription_sessions),
        )
        // Конфиг-файл одной подписки (без утечки остальных)
        .route(
            "/subs/{id}/config-file",
            get(handlers::api::bot::get_sub_config_file),
        )
        // Тикеты поддержки — управление из бота
        .route(
            "/tickets",
            get(handlers::api::bot::bot_list_tickets),
        )
        .route(
            "/tickets/{id}",
            get(handlers::api::bot::bot_get_ticket),
        )
        .route(
            "/tickets/{id}/messages",
            post(handlers::api::bot::bot_add_ticket_message),
        )
        .route(
            "/tickets/{id}/assign",
            post(handlers::api::bot::bot_assign_ticket),
        )
        .route(
            "/tickets/{id}/status",
            post(handlers::api::bot::bot_set_ticket_status),
        )
        .route(
            "/tickets/{id}/attachments/{attachment_id}",
            get(handlers::api::bot::bot_download_attachment),
        )
        // Broadcast уведомлений для сегмента пользователей
        .route(
            "/notifications/broadcast",
            post(handlers::api::bot::bot_broadcast_notification),
        )
        // Применяем rate limiting — внутренний слой (выполняется после авторизации)
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            bot_rate_limit::bot_rate_limit,
        ))
        // Применяем проверку токена — внешний слой (выполняется первым)
        .route_layer(axum::middleware::from_fn(bot_auth::require_bot_token))
}

/// Роутер аутентификации и API standalone-приложения (/api/v2/app/*).
///
/// Публичные эндпоинты (без JWT): регистрация и логины, refresh, logout.
/// Защищённые эндпоинты (require_app_jwt): профиль, подписка, серверы.
/// `state` передаётся явно ради единообразия с `bot_routes`, хотя сам
/// `require_app_jwt` состояния не требует (валидирует JWT по env-секрету).
pub fn app_routes(_state: AppState) -> axum::Router<AppState> {
    // Публичные маршруты — без middleware.
    let public = axum::Router::new()
        .route("/register", post(app_auth::register_email))
        .route("/login/email", post(app_auth::login_email))
        .route("/login/telegram", post(app_auth::login_telegram))
        .route("/login/code", post(app_auth::login_code))
        .route("/refresh", post(app_auth::refresh))
        .route("/logout", post(app_auth::logout))
        // Публичная валидация кода вовлечения (до register/login). READ-ONLY,
        // не списывает использование; PII не утекает.
        .route("/enroll/{code}", get(app_enroll::validate_enroll_code))
        // Публичный branding (до логина): тир-гейт + brand_* из settings.
        // READ-ONLY; отдаёт только brand_* + флаги, никаких секретов.
        .route("/branding", get(app_branding::get_branding));

    // Защищённые маршруты — за require_app_jwt.
    let protected = axum::Router::new()
        .route("/me", get(app::get_me))
        .route("/subscription", get(app::get_subscription))
        .route("/servers", get(app::list_servers))
        // Аккаунт: устройства / рефералы / семья / подписки / relay-страны.
        .route("/devices", get(app_account::list_devices))
        .route(
            "/devices/{id}",
            patch(app_account::rename_device).delete(app_account::revoke_device),
        )
        .route("/referrals", get(app_account::get_referrals))
        .route(
            "/family",
            get(app_account::get_family),
        )
        .route("/family/invite", post(app_account::create_family_invite))
        .route(
            "/family/{member_id}",
            delete(app_account::remove_family_member),
        )
        .route("/subscriptions", get(app_account::list_subscriptions))
        .route("/relays", get(app_account::list_relays))
        // Биллинг: история трафика (график) + создание чек-аута покупки.
        .route("/traffic", get(app_billing::get_traffic))
        .route("/purchase", post(app_billing::purchase))
        // Поддержка: уведомления (inbox) + тикеты. Хранилище — notifications_svc
        // и tickets_svc; владение тикетом проверяют сами сервисы по AuthUser.
        .route("/notifications", get(app_support::list_notifications))
        .route(
            "/notifications/{id}/read",
            post(app_support::mark_notification_read),
        )
        .route(
            "/notifications/read-all",
            post(app_support::mark_all_notifications_read),
        )
        .route(
            "/tickets",
            get(app_support::list_tickets).post(app_support::create_ticket),
        )
        .route("/tickets/{id}", get(app_support::get_ticket))
        .route("/tickets/{id}/reply", post(app_support::reply_ticket))
        // Партнёрский кабинет: per-source реферальные коды + статистика.
        // Гейтинг роли (is_partner) — внутри хендлеров: не-партнёр получает
        // is_partner:false на чтении и 403 на мутациях.
        .route(
            "/partner/codes",
            get(app_partner::list_codes).post(app_partner::create_code),
        )
        .route("/partner/codes/{code}", delete(app_partner::delete_code))
        .route_layer(axum::middleware::from_fn(app_auth::require_app_jwt));

    public.merge(protected)
}
