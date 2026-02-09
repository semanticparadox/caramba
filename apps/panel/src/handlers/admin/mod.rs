// Admin Module - Modular Structure
pub mod auth;
pub mod dashboard;
pub mod nodes;
pub mod users;
pub mod plans;
pub mod settings;
pub mod analytics;
pub mod store;

// Re-export commonly used functions for convenience
pub use auth::{get_login, login, logout, get_auth_user, is_authenticated};
pub use dashboard::{get_dashboard, get_statusbar};
pub use nodes::{get_nodes, install_node, get_node_edit, update_node, sync_node, delete_node, toggle_node_enable, activate_node, get_node_install_script, get_node_raw_install_script};
pub use users::{get_users, admin_gift_subscription, get_user_details, update_user, update_user_balance, delete_user_subscription, refund_user_subscription, extend_user_subscription, get_subscription_devices, admin_kill_subscription_sessions};
pub use plans::{get_plans, add_plan, delete_plan, get_plan_edit, update_plan};
pub use settings::{get_settings, save_settings, toggle_bot, bot_logs_page, bot_logs_history, bot_logs_tail, export_database, update_trial_config, reset_user_trials};
pub use analytics::{get_traffic_analytics, get_transactions, get_system_logs_page};
pub use store::{get_store_categories_page, create_category, delete_category, get_store_products_page, create_product, delete_product};

// Aliases for backward compatibility with main.rs routes
pub use settings::export_database as db_export_download;
pub use settings::bot_logs_page as get_bot_page;

// TODO: These functions were not in the original admin.rs or need to be migrated from elsewhere
// Stub implementations to fix compilation - these should be properly implemented
pub async fn check_update(axum::extract::State(_state): axum::extract::State<crate::AppState>) -> impl axum::response::IntoResponse {
    "Update check not yet implemented".to_string()
}

pub async fn get_node_logs(axum::extract::Path(_id): axum::extract::Path<i64>, axum::extract::State(_state): axum::extract::State<crate::AppState>) -> impl axum::response::IntoResponse {
    "Node logs not yet implemented".to_string()
}

pub async fn get_frontends(axum::extract::State(_state): axum::extract::State<crate::AppState>, _jar: axum_extra::extract::cookie::CookieJar) -> impl axum::response::IntoResponse {
    "Frontends page not yet implemented".to_string()
}

pub async fn api_keys_list(axum::extract::State(_state): axum::extract::State<crate::AppState>, _jar: axum_extra::extract::cookie::CookieJar) -> impl axum::response::IntoResponse {
    "API keys list not yet implemented".to_string()
}

pub async fn api_keys_create(axum::extract::State(_state): axum::extract::State<crate::AppState>) -> impl axum::response::IntoResponse {
    "API key creation not yet implemented".to_string()
}

pub async fn api_keys_delete(axum::extract::Path(_id): axum::extract::Path<i64>, axum::extract::State(_state): axum::extract::State<crate::AppState>) -> impl axum::response::IntoResponse {
    "API key deletion not yet implemented".to_string()
}

pub async fn handle_payment(axum::extract::Path(_source): axum::extract::Path<String>, axum::extract::State(_state): axum::extract::State<crate::AppState>, _body: axum::body::Bytes) -> impl axum::response::IntoResponse {
    axum::http::StatusCode::NOT_IMPLEMENTED
}

