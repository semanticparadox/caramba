pub mod bot_auth;
pub mod client;
pub mod node;

use crate::handlers;
use crate::AppState;
use axum::routing::{get, post};

/// Защищённый роутер для всех /api/v2/bot/* маршрутов бота.
/// Middleware require_bot_token проверяет X-Bot-Token на каждом запросе.
/// Возвращает Router<AppState> без вызова with_state — состояние передаётся
/// через родительский роутер при монтировании через .nest().
pub fn bot_routes() -> axum::Router<AppState> {
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
        // Применяем проверку токена ко всем маршрутам этого роутера
        .route_layer(axum::middleware::from_fn(bot_auth::require_bot_token))
}
