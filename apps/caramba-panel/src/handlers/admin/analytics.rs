// Analytics Module
// Traffic analytics, transactions, logs

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use super::auth::{get_auth_user, is_authenticated};
use super::dashboard::OrderWithUser;
use crate::AppState;
use crate::services::unified_log_service::{UnifiedLogEntry, UnifiedLogService};

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
#[allow(dead_code)] // Fields used in template but compiler can't detect
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
    pub logs: Vec<UnifiedLogEntry>,
    pub categories: Vec<String>,
    pub sources: Vec<String>,
    pub current_category: String,
    pub current_source: String,
    pub current_node_id: i64,
    pub nodes: Vec<caramba_db::models::node::Node>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Deserialize)]
pub struct LogsFilter {
    pub category: Option<String>,
    pub source: Option<String>,
    pub node_id: Option<i64>,
}

// ============================================================================
// Route Handlers
// ===========================================================================

pub async fn get_traffic_analytics(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    // Fetch System Stats from AnalyticsService
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

    let total_traffic_30d = format_bytes_str(stats.total_traffic_30d_bytes as u64);
    let active_nodes_count = stats.active_nodes;

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

    let admin_path = state.admin_path.clone();

    // Fetch real chart data
    let traffic_history = state
        .analytics_service
        .get_traffic_history()
        .await
        .unwrap_or_default();
    let mut history_labels: Vec<String> = traffic_history.iter().map(|d| d.date.clone()).collect();
    let mut history_data: Vec<i64> = traffic_history
        .iter()
        .map(|d| d.traffic_used / (1024 * 1024 * 1024))
        .collect();
    history_labels.reverse();
    history_data.reverse();
    if history_labels.is_empty() {
        history_labels.push("Today".to_string());
        history_data.push(0);
    }

    let node_traffic = state
        .analytics_service
        .get_node_traffic_stats()
        .await
        .unwrap_or_default();
    let node_labels: Vec<String> = node_traffic.iter().map(|n| n.name.clone()).collect();
    let node_series: Vec<i64> = node_traffic
        .iter()
        .map(|n| n.total_traffic / (1024 * 1024 * 1024))
        .collect();

    let template = AnalyticsTemplate {
        total_traffic_30d,
        active_nodes_count,
        orders,
        top_users,
        history_data_json: serde_json::to_string(&history_data)
            .unwrap_or_else(|_| "[0]".to_string()),
        history_labels_json: serde_json::to_string(&history_labels)
            .unwrap_or_else(|_| r#"["Today"]"#.to_string()),
        node_series_json: serde_json::to_string(&node_series).unwrap_or_else(|_| "[0]".to_string()),
        node_labels_json: serde_json::to_string(&node_labels)
            .unwrap_or_else(|_| r#"["All"]"#.to_string()),
        is_auth: true,
        admin_path,
        active_page: "analytics".to_string(),
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap_or_default())
}

pub async fn get_transactions(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return axum::response::Redirect::to(&format!("{}/login", state.admin_path))
            .into_response();
    }

    let orders = state
        .billing_service
        .get_all_orders()
        .await
        .unwrap_or_default();

    let admin_path = state.admin_path.clone();

    let template = TransactionsTemplate {
        orders,
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "transactions".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
    }
}

pub async fn get_system_logs_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(filter): Query<LogsFilter>,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return axum::response::Redirect::to(&format!("{}/login", state.admin_path))
            .into_response();
    }

    let category = filter.category.unwrap_or_default().trim().to_string();
    let source = filter.source.unwrap_or_else(|| "all".to_string());
    let nodes = state
        .infrastructure_service
        .get_all_nodes()
        .await
        .unwrap_or_default();
    let selected_node = filter.node_id.and_then(|node_id| {
        nodes
            .iter()
            .find(|node| node.id == node_id)
            .map(|node| (node.id, node.name.clone()))
    });

    let payload = UnifiedLogService::get_logs(
        &state.pool,
        &state.redis,
        &state.task_health,
        &source,
        if category.is_empty() {
            None
        } else {
            Some(category.as_str())
        },
        selected_node,
        80,
        0,
    )
    .await
    .unwrap_or_else(
        |_| crate::services::unified_log_service::UnifiedLogPayload {
            logs: Vec::new(),
            categories: Vec::new(),
        },
    );

    let admin_path = state.admin_path.clone();

    let template = SystemLogsTemplate {
        logs: payload.logs,
        categories: payload.categories,
        sources: vec!["all", "system", "bot", "node", "monitoring"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        current_category: category,
        current_source: source,
        current_node_id: filter.node_id.unwrap_or(0),
        nodes,
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "logs".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
    }
}
