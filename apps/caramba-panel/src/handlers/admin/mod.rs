// Admin Module - Modular Structure
pub mod analytics;
pub mod api_keys;
pub mod auth;
pub mod dashboard;
pub mod frontends;
pub mod nodes;
pub mod plans;
pub mod promo;
pub mod settings;
pub mod store;
pub mod updates;
pub mod users;

// Re-export commonly used functions for convenience
pub use analytics::{get_system_logs_page, get_traffic_analytics, get_transactions};
pub use api_keys::{create_api_key, delete_api_key, list_api_keys};
pub use auth::{get_auth_user, get_login, is_authenticated, login, logout};
pub use dashboard::{get_dashboard, get_statusbar};
pub use frontends::get_frontends;
pub use nodes::{
    activate_node, block_sni, delete_node, get_install_sh, get_node_edit, get_node_install_script,
    get_node_logs, get_node_manage, get_node_raw_install_script, get_node_rescue, get_nodes,
    install_node, pin_sni, sync_node, toggle_node_enable, trigger_scan, unpin_sni, update_node,
};
pub use plans::{add_plan, delete_plan, get_plan_edit, get_plans, update_plan};
pub use promo::{add_promo, delete_promo, get_promos};
pub use settings::{
    bot_logs_history, bot_logs_page, bot_logs_tail, check_update, export_database, get_settings,
    save_settings, toggle_bot, update_trial_config,
};
pub use store::{
    create_category, create_product, delete_category, delete_product, get_store_categories_page,
    get_store_products_page,
};
pub use users::{
    admin_gift_subscription, admin_kill_subscription_sessions, delete_user_subscription,
    extend_user_subscription, get_subscription_devices, get_user_details, get_users,
    notify_all_users, notify_user, refund_user_subscription, update_user, update_user_balance,
};

// Stubs removed

pub async fn handle_payment(
    axum::extract::Path(_source): axum::extract::Path<String>,
    axum::extract::State(_state): axum::extract::State<crate::AppState>,
    _body: axum::body::Bytes,
) -> impl axum::response::IntoResponse {
    axum::http::StatusCode::NOT_IMPLEMENTED
}
