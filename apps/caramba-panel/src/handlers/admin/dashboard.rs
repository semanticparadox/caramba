// Dashboard Module
// Main dashboard page and system statusbar

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;

use super::auth::get_auth_user;
use crate::AppState;
use crate::services::logging_service::LoggingService;
use crate::utils::format_bytes_str;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub active_nodes: i64,
    pub total_users: i64,
    pub active_subs: i64,
    pub total_revenue: String,
    pub total_traffic: String,
    pub total_traffic_30d: String,
    pub orders: Vec<OrderWithUser>,
    pub top_users: Vec<UserWithTraffic>,
    pub history_data_json: String,
    pub history_labels_json: String,
    pub node_series_json: String,
    pub node_labels_json: String,
    pub activities: Vec<RecentActivity>,
    pub bot_status: String,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/statusbar.html")]
pub struct StatusbarPartial {
    pub bot_status: String,
    pub db_status: String,
    pub redis_status: String,
    pub admin_path: String,
    pub pg_version: String,
    pub redis_version: String,
    pub bot_username: String,
    pub cpu_usage: String,
    pub ram_usage: String,
}

pub struct RecentActivity {
    pub action: String,
    pub details: Option<String>,
    pub created_at: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct OrderWithUser {
    pub id: i64,
    pub username: String,
    pub total_amount: String,
    pub status: String,
    pub created_at: String,
}

pub struct UserWithTraffic {
    pub username: String,
    pub total_traffic_fmt: String,
}

// ============================================================================
// Route Handlers
// ============================================================================

/// GET /admin/dashboard - Main dashboard page
pub async fn get_dashboard(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let stats = state.analytics_service.get_system_stats().await.unwrap_or(
        crate::services::analytics_service::SystemStats {
            active_nodes: 0,
            total_users: 0,
            active_subs: 0,
            total_revenue: 0.0,
            total_traffic_bytes: 0,
            total_traffic_30d_bytes: 0,
        },
    );

    let active_nodes = stats.active_nodes;
    let total_users = stats.total_users;
    let active_subs = stats.active_subs;
    let total_revenue = format!("{:.2}", stats.total_revenue);

    let total_traffic = format_bytes_str(stats.total_traffic_bytes as u64);
    let total_traffic_30d = total_traffic.clone();

    let admin_path = state.admin_path.clone();

    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or("Admin".to_string());

    let is_running = state.bot_manager.is_running().await;
    let bot_status = if is_running { "running" } else { "stopped" }.to_string();

    let logs = LoggingService::get_logs(&state.pool, 10, 0, None)
        .await
        .unwrap_or_default();
    let activities: Vec<RecentActivity> = logs
        .into_iter()
        .map(|log| RecentActivity {
            action: log.action,
            details: log.details,
            created_at: log.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        })
        .collect();

    let orders = state
        .billing_service
        .get_recent_orders(10)
        .await
        .unwrap_or_default();

    let top_users_raw = state
        .analytics_service
        .get_top_users()
        .await
        .unwrap_or_default();
    let top_users: Vec<UserWithTraffic> = top_users_raw
        .into_iter()
        .map(|u| UserWithTraffic {
            username: u.username.unwrap_or_else(|| "Unknown".to_string()),
            total_traffic_fmt: format_bytes_str(u.total_traffic as u64),
        })
        .collect();

    let template = DashboardTemplate {
        active_nodes,
        total_users,
        active_subs,
        total_revenue,
        total_traffic,
        total_traffic_30d,
        orders,
        top_users,
        history_data_json: "[0,0,0,0,0]".to_string(),
        history_labels_json: r#"["Mon", "Tue", "Wed", "Thu", "Fri"]"#.to_string(),
        node_series_json: "[100]".to_string(),
        node_labels_json: r#"["All Nodes"]"#.to_string(),
        activities,
        bot_status,
        is_auth: true,
        username,
        admin_path,
        active_page: "dashboard".to_string(),
    };
    Html(template.render().unwrap())
}

/// GET /admin/statusbar - System status partial
pub async fn get_statusbar(State(state): State<AppState>) -> impl IntoResponse {
    let is_running = state.bot_manager.is_running().await;
    let bot_status = if is_running { "running" } else { "stopped" }.to_string();
    let bot_username = state.settings.get_or_default("bot_username", "").await;

    let (redis_status, redis_version) = match state.redis.get_connection().await {
        Ok(mut con) => {
            let info: String = redis::cmd("INFO")
                .arg("server")
                .query_async::<String>(&mut con)
                .await
                .unwrap_or_default();
            let version = info
                .lines()
                .find(|l| l.starts_with("redis_version:"))
                .map(|l| l.replace("redis_version:", "").trim().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            ("Online".to_string(), version)
        }
        Err(_) => ("Offline".to_string(), "-".to_string()),
    };

    let (db_status, pg_version) = match sqlx::query_scalar::<_, String>("SELECT version()")
        .fetch_one(&state.pool)
        .await
    {
        Ok(v) => (
            "Online".to_string(),
            v.split(' ').next().unwrap_or("Postgres").to_string(),
        ),
        Err(_) => ("Offline".to_string(), "-".to_string()),
    };

    let (cpu_usage, ram_usage) = {
        let mut sys = state.system_stats.lock().await;
        sys.refresh_all();

        let cpu = sys.global_cpu_usage();
        let total_ram = sys.total_memory();
        let used_ram = sys.used_memory();

        let total_gb = total_ram as f64 / 1024.0 / 1024.0 / 1024.0;
        let used_gb = used_ram as f64 / 1024.0 / 1024.0 / 1024.0;

        (
            format!("{:.1}%", cpu),
            format!("{:.1}/{:.1} GB", used_gb, total_gb),
        )
    };

    let admin_path = state.admin_path.clone();

    let template = StatusbarPartial {
        bot_status,
        db_status,
        redis_status,
        admin_path,
        pg_version,
        redis_version,
        bot_username: if bot_username.is_empty() {
            "Not configured".to_string()
        } else {
            bot_username
        },
        cpu_usage,
        ram_usage,
    };
    Html(template.render().unwrap_or_default())
}
