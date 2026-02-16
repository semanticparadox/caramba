// Admin Module - Modular Structure
pub mod auth;
pub mod dashboard;
pub mod nodes;
pub mod users;
pub mod plans;
pub mod settings;
pub mod analytics;
pub mod store;
pub mod frontends;
pub mod api_keys;
pub mod promo;
pub mod updates;

// Re-export commonly used functions for convenience
pub use auth::{get_login, login, logout, get_auth_user, is_authenticated};
pub use dashboard::{get_dashboard, get_statusbar};
pub use nodes::{get_nodes, install_node, get_node_edit, update_node, sync_node, delete_node, toggle_node_enable, activate_node, get_node_install_script, get_node_raw_install_script, get_node_logs, get_install_sh, get_node_rescue, trigger_scan, get_node_manage, pin_sni, unpin_sni, block_sni};
pub use users::{get_users, admin_gift_subscription, get_user_details, update_user, update_user_balance, delete_user_subscription, refund_user_subscription, extend_user_subscription, get_subscription_devices, admin_kill_subscription_sessions};
pub use plans::{get_plans, add_plan, delete_plan, get_plan_edit, update_plan};
pub use settings::{get_settings, save_settings, toggle_bot, bot_logs_page, bot_logs_history, bot_logs_tail, update_trial_config, check_update, export_database};
pub use analytics::{get_traffic_analytics, get_transactions, get_system_logs_page};
pub use store::{get_store_categories_page, create_category, delete_category, get_store_products_page, create_product, delete_product};
pub use frontends::get_frontends;
pub use api_keys::{list_api_keys, create_api_key, delete_api_key};
pub use promo::{get_promos, add_promo, delete_promo};


// Stubs removed


pub async fn handle_payment(axum::extract::Path(_source): axum::extract::Path<String>, axum::extract::State(_state): axum::extract::State<crate::AppState>, _body: axum::body::Bytes) -> impl axum::response::IntoResponse {
    axum::http::StatusCode::NOT_IMPLEMENTED
}

