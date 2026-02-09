// Analytics Module
// Traffic analytics, transactions, logs

use axum::{
    extract::{State, Query},
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;


use crate::AppState;
use super::auth::{get_auth_user, is_authenticated};
use super::dashboard::{get_recent_orders, OrderWithUser};
use crate::services::logging_service::LoggingService;

fn format_bytes_str(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// Templates
// ============================================================================

#[derive(sqlx::FromRow)]
pub struct UserWithTraffic {
    pub username: String,
    pub total_traffic_fmt: String,
}



#[derive(Template, WebTemplate)]
#[template(path = "analytics.html")]
pub struct AnalyticsTemplate {
    pub total_traffic_30d: String,
    pub active_nodes_count: i64,
    pub orders: Vec<OrderWithUser>,
    pub top_users: Vec<UserWithTraffic>,
    pub history_data_json: String,
    pub history_labels_json: String,
    pub node_series_json: String,
    pub node_labels_json: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "transactions.html")]
pub struct TransactionsTemplate {
    pub orders: Vec<OrderWithUser>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "logs.html")]
pub struct SystemLogsTemplate {
    pub logs: Vec<crate::services::logging_service::LogEntry>,
    pub categories: Vec<String>,
    pub current_category: String,
    pub current_page: i64,
    pub has_next: bool,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Deserialize)]
pub struct LogsFilter {
    pub category: Option<String>,
    pub page: Option<i64>,
}

// ============================================================================
// Route Handlers
// ===========================================================================

pub async fn get_traffic_analytics(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let total_traffic_30d_bytes = sqlx::query_scalar::<_, i64>("SELECT SUM(total_ingress + total_egress) FROM nodes").fetch_one(&state.pool).await.unwrap_or(0);
    let total_traffic_30d = format_bytes_str(total_traffic_30d_bytes as u64);

    let active_nodes_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM nodes WHERE status = 'active'").fetch_one(&state.pool).await.unwrap_or(0);

    let orders = get_recent_orders(&state.pool).await;

    let top_users = sqlx::query_as!(
        UserWithTraffic,
        r#"SELECT COALESCE(username, full_name, 'Unknown') as "username!", '0 GB' as "total_traffic_fmt!" FROM users LIMIT 5"#
    ).fetch_all(&state.pool).await.unwrap_or_default();

    let admin_path = state.admin_path.clone();

    let template = AnalyticsTemplate {
        total_traffic_30d,
        active_nodes_count,
        orders,
        top_users,
        history_data_json: "[0,0,0,0,0]".to_string(),
        history_labels_json: r#"["Mon", "Tue", "Wed", "Thu", "Fri"]"#.to_string(),
        node_series_json: "[100]".to_string(),
        node_labels_json: r#"["All Nodes"]"#.to_string(),
        is_auth: true,
        admin_path,
        active_page: "analytics".to_string(),
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap())
}

pub async fn get_transactions(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return axum::response::Redirect::to(&format!("{}/login", state.admin_path)).into_response();
    }

    struct OrderQueryRow {
        id: i64,
        username: String,
        total_amount: i64,
        status: String,
        created_at: Option<chrono::NaiveDateTime>,
    }

    let orders = sqlx::query_as!(
        OrderQueryRow,
        r#"
        SELECT 
            o.id, 
            COALESCE(u.username, u.full_name, 'Unknown') as "username!", 
            o.total_amount as "total_amount!", 
            o.status as "status!", 
            o.created_at
        FROM orders o
        JOIN users u ON o.user_id = u.id
        ORDER BY o.created_at DESC
        "#
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|row| OrderWithUser {
        id: row.id,
        username: row.username,
        total_amount: format!("{:.2}", row.total_amount as f64 / 100.0),
        status: row.status,
        created_at: row.created_at.map(|dt| dt.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default(),
    })
    .collect();

    let admin_path = state.admin_path.clone();

    let template = TransactionsTemplate {
        orders,
        is_auth: true,
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "transactions".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn get_system_logs_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(filter): Query<LogsFilter>,
) -> impl IntoResponse {
    let page = filter.page.unwrap_or(1);
    let limit = 50;
    let offset = (page - 1) * limit;
    
    let category = filter.category.unwrap_or_default();
    
    let logs = LoggingService::get_logs(&state.pool, limit + 1, offset, Some(category.clone()))
        .await
        .unwrap_or_default();
        
    let has_next = logs.len() > limit as usize;
    let logs = if has_next {
        logs.into_iter().take(limit as usize).collect()
    } else {
        logs
    };

    let categories = LoggingService::get_categories(&state.pool)
        .await
        .unwrap_or_default();

    let admin_path = state.admin_path.clone();

    let template = SystemLogsTemplate {
        logs,
        categories,
        current_category: category,
        current_page: page,
        has_next,
        is_auth: true,
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "system_logs".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}
