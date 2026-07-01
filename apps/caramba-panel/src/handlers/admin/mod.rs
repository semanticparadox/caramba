// Admin Module - Modular Structure
pub mod analytics;
pub mod api_keys;
pub mod auth;
pub mod backups;
pub mod dashboard;
pub mod frontends;
pub mod marketplace;
pub mod nodes;
pub mod notifications;
pub mod payment_pricing;
pub mod payments;
pub mod plans;
pub mod promo;
pub mod settings;
pub mod store;
pub mod tickets;
pub mod updates;
pub mod users;

pub use updates::get_updates_page;

// Re-export commonly used functions for convenience
pub use analytics::{get_system_logs_page, get_traffic_analytics, get_transactions};
pub use api_keys::{create_api_key, delete_api_key, list_api_keys};
pub use auth::{get_auth_user, get_login, is_authenticated, login, logout};
pub use dashboard::{get_dashboard, get_statusbar, get_task_health};
pub use frontends::get_frontends;
pub use nodes::{
    activate_node, block_sni, delete_node, get_install_sh, get_node_edit, get_node_install_script,
    get_node_logs, get_node_manage, get_node_raw_install_script, get_node_rescue, get_nodes,
    install_node, pin_sni, sync_node, toggle_node_enable, trigger_scan, unpin_sni, update_node,
};
pub use notifications::{get_notifications_page, run_expiry_reminder_loop};
pub use plans::{add_plan, delete_plan, get_plan_edit, get_plans, update_plan};
pub use promo::{add_promo, delete_promo, get_promos};
pub use settings::{
    apply_deployment_topology, bot_logs_history, bot_logs_page, bot_logs_tail, check_update,
    export_database, get_settings, prepare_agent_update, queue_worker_update, rollout_agent_update,
    save_settings, toggle_bot,
};
pub use store::{
    create_category, create_product, delete_category, delete_product, get_store_categories_page,
    get_store_products_page,
};
pub use users::{
    admin_gift_subscription, admin_kill_subscription_sessions, approve_user_subscription,
    delete_user_subscription, extend_user_subscription, get_subscription_devices,
    get_user_details, get_users,
    notify_all_users, notify_preview, notify_user, refund_user_subscription,
    reset_user_referral_rates, set_subscription_node, update_user, update_user_balance,
    update_user_referral_rates,
};

pub use backups::{create_backup_now, delete_backup_handler, download_backup, get_backups_page};

pub use marketplace::{
    approve_manual_payment, get_marketplace_page, reject_manual_payment, save_marketplace_settings,
};

pub use payments::test_provider_connection;

// Stubs removed
