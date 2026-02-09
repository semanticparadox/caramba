// Dashboard Module
// Main dashboard page and system statusbar

use axum::{
    extract::State,
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;

use crate::AppState;
use crate::utils::format_bytes_str;
use crate::services::logging_service::LoggingService;
use super::auth::get_auth_user;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub active_nodes: i64,
    #[allow(dead_code)]
    pub total_users: i64,
    #[allow(dead_code)]
    pub active_subs: i64,
    pub total_revenue: String,
    #[allow(dead_code)]
    pub total_traffic: String,
    pub total_traffic_30d: String,
    pub orders: Vec<OrderWithUser>,
    pub top_users: Vec<UserWithTraffic>,
    pub history_data_json: String,
    #[allow(dead_code)]
    pub history_labels_json: String,
    pub node_series_json: String,
    #[allow(dead_code)]
    pub node_labels_json: String,
    pub activities: Vec<RecentActivity>,
    #[allow(dead_code)]
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
    pub sqlite_version: String,
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
// Helper Functions
// ============================================================================

pub async fn get_recent_orders(pool: &sqlx::SqlitePool) -> Vec<OrderWithUser> {
    sqlx::query_as::<_, OrderWithUser>(
        r#"
        SELECT o.id, COALESCE(u.username, u.full_name, 'Unknown') as username, 
               printf('%.2f', CAST(o.total_amount AS REAL) / 100.0) as total_amount,
               o.status, o.created_at
        FROM orders o
        LEFT JOIN users u ON o.user_id = u.id
        ORDER BY o.created_at DESC
        LIMIT 10
        "#
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

// ============================================================================
// Route Handlers
// ============================================================================

/// GET /admin/dashboard - Main dashboard page
pub async fn get_dashboard(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let active_nodes = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM nodes WHERE status = 'active'").fetch_one(&state.pool).await.unwrap_or(0);
    let total_users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users").fetch_one(&state.pool).await.unwrap_or(0);
    let active_subs = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM subscriptions WHERE status = 'active'").fetch_one(&state.pool).await.unwrap_or(0);
    let revenue_cents = sqlx::query_scalar::<_, i64>("SELECT SUM(total_amount) FROM orders WHERE status = 'completed'").fetch_one(&state.pool).await.unwrap_or(0);
    let total_revenue = format!("{:.2}", revenue_cents as f64 / 100.0);
    
    // Total traffic across all nodes
    let total_traffic_bytes = sqlx::query_scalar::<_, i64>("SELECT SUM(total_ingress + total_egress) FROM nodes").fetch_one(&state.pool).await.unwrap_or(0);
    let total_traffic = format_bytes_str(total_traffic_bytes as u64);
    let total_traffic_30d = total_traffic.clone(); // Placeholder for 30d specific query if needed

    let admin_path = state.admin_path.clone();

    let username = get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string());

    let is_running = state.bot_manager.is_running().await;
    let bot_status = if is_running { "running" } else { "stopped" }.to_string();

    // Fetch recent activity logs
    let logs = LoggingService::get_logs(&state.pool, 10, 0, None).await.unwrap_or_default();
    let activities: Vec<RecentActivity> = logs.into_iter().map(|log| RecentActivity {
        action: log.action,
        details: log.details,
        created_at: log.created_at,
    }).collect();

    // Analytics Data
    let orders = get_recent_orders(&state.pool).await;
    let top_users = sqlx::query_as!(
        UserWithTraffic,
        r#"SELECT COALESCE(username, full_name, 'Unknown') as "username!", '0 GB' as "total_traffic_fmt!" FROM users LIMIT 5"#
    ).fetch_all(&state.pool).await.unwrap_or_default();

    let template = DashboardTemplate {
        active_nodes,
        total_users,
        active_subs,
        total_revenue,
        total_traffic,
        total_traffic_30d,
        orders,
        top_users,
        history_data_json: "[0,0,0,0,0]".to_string(), // Real data to be implemented if needed
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
    let bot_username = state.settings.get_or_default("bot_username", "Unknown").await;
    
    // Check Redis & Version
    let (redis_status, redis_version) = match state.redis.get_connection().await {
        Ok(mut con) => {
            let info: String = redis::cmd("INFO").arg("server").query_async::<String>(&mut con).await.unwrap_or_default();
            // Parse redis_version: X.Y.Z
            let version = info.lines()
                .find(|l| l.starts_with("redis_version:"))
                .map(|l| l.replace("redis_version:", "").trim().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            ("Online".to_string(), version)
        },
        Err(_) => ("Offline".to_string(), "-".to_string()),
    };

    // Check DB & Version
    let (db_status, sqlite_version) = match sqlx::query_scalar::<_, String>("SELECT sqlite_version()").fetch_one(&state.pool).await {
        Ok(v) => ("Online".to_string(), v),
        Err(_) => ("Offline".to_string(), "-".to_string()),
    };

    // System Stats
    let (cpu_usage, ram_usage) = {
        let mut sys = state.system_stats.lock().await;
        sys.refresh_all();
        
        let cpu = sys.global_cpu_usage();
        let total_ram = sys.total_memory();
        let used_ram = sys.used_memory();
        
        // Format RAM (e.g., "4.5/16 GB")
        let total_gb = total_ram as f64 / 1024.0 / 1024.0 / 1024.0;
        let used_gb = used_ram as f64 / 1024.0 / 1024.0 / 1024.0;
        
        (
            format!("{:.1}%", cpu),
            format!("{:.1}/{:.1} GB", used_gb, total_gb)
        )
    };

    let admin_path = state.admin_path.clone();

    let template = StatusbarPartial {
        bot_status,
        db_status,
        redis_status,
        admin_path,
        sqlite_version,
        redis_version,
        bot_username,
        cpu_usage,
        ram_usage,
    };
    Html(template.render().unwrap_or_default())
}
