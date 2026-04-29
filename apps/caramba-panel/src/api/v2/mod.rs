pub mod bot_auth;
pub mod bot_rate_limit;
pub mod client;
pub mod node;

use crate::handlers;
use crate::AppState;
use axum::routing::{get, post};

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
        .route("/settings/{key}", get(handlers::api::bot::get_settings))
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
