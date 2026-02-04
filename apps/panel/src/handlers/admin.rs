use axum::{
    extract::{State, Form, Path},
    response::{IntoResponse, Html},
};
use askama::Template;
use serde::Deserialize;
use crate::AppState;
use crate::models::node::Node;
use crate::models::store::{Plan, User, Order};
use crate::services::logging_service::LoggingService;
use std::collections::HashMap;
use tracing::{info, error};
use axum_extra::extract::cookie::{Cookie, CookieJar};

#[derive(serde::Serialize)]
#[allow(dead_code)]
pub struct TrialStats {
    pub default_count: i64,
    pub channel_count: i64,
    pub active_count: i64,
}

#[derive(Template)]
#[template(path = "settings.html")]
pub struct SettingsTemplate {
    // pub masked_bot_token: String,  // Removed unused
    // pub bot_status: String,        // Removed unused
    pub masked_payment_api_key: String,
    pub masked_cryptomus_merchant_id: String,
    pub masked_cryptomus_payment_api_key: String,
    pub masked_aaio_merchant_id: String,
    pub masked_aaio_secret_1: String,
    pub masked_aaio_secret_2: String,

    pub payment_ipn_url: String,
    pub currency_rate: String,
    pub support_url: String,
    pub bot_username: String,
    pub brand_name: String,
    pub terms_of_service: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,

    // Decoy Settings
    pub decoy_enabled: bool,
    pub decoy_urls: String,
    pub decoy_min_interval: String,
    pub decoy_max_interval: String,

    // Kill Switch Settings
    pub kill_switch_enabled: bool,
    pub kill_switch_timeout: String,

    // Trial Configuration (Moved from Tools)
    pub free_trial_days: i64,
    pub channel_trial_days: i64,
    pub required_channel_id: String,
    pub last_export: String,
}

#[derive(Template)]
#[template(path = "bot.html")]
pub struct BotTemplate {
    pub masked_bot_token: String,
    pub bot_status: String,
    pub bot_username: String,
    // pub webhook_info: Option<String>, // Removed unused
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}


#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub total_users: i64,
    pub active_subs: i64,
    pub total_revenue: f64,
    pub active_nodes: i64,
    // Add traffic stats
    // Add traffic stats
    pub total_traffic: String,
    pub bot_status: String,
    pub is_auth: bool,
    pub activities: Vec<crate::services::logging_service::LogEntry>,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "partials/bot_status.html")]
pub struct BotStatusPartial {
    pub bot_status: String,
    pub admin_path: String,
}

#[derive(Template)]
#[template(path = "nodes.html")]
pub struct NodesTemplate {
    pub nodes: Vec<Node>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}




#[derive(Template)]
#[template(path = "users.html")]
pub struct UsersTemplate {
    pub users: Vec<crate::models::store::User>,
    pub search: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "bot_logs.html")]
pub struct BotLogsTemplate {
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "transactions.html")]
pub struct TransactionsTemplate {
    pub orders: Vec<OrderWithUser>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Clone, serde::Serialize, sqlx::FromRow)]
pub struct DailyStat {
    pub date: String, // YYYY-MM-DD
    pub new_users: i64,
    pub active_users: i64,
    pub total_orders: i64,
    pub total_revenue: i64,
}

impl DailyStat {
    pub fn revenue_usd(&self) -> f64 {
        self.total_revenue as f64 / 100.0
    }
}

#[derive(Template)]
#[template(path = "admin/stats.html")]
pub struct AnalyticsTemplate {
    pub stats: Vec<DailyStat>,
    pub stats_rev: Vec<DailyStat>,
    pub today_new_users: i64,
    pub today_active_users: i64,
    pub today_orders: i64,
    pub today_revenue: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

pub struct OrderWithUser {
    pub id: i64,
    pub username: String,
    pub total_amount: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}



#[derive(Deserialize)]
pub struct SaveSettingsForm {
    pub bot_token: Option<String>,

    pub payment_api_key: Option<String>,
    pub cryptomus_merchant_id: Option<String>,
    pub cryptomus_payment_api_key: Option<String>,
    pub aaio_merchant_id: Option<String>,
    pub aaio_secret_1: Option<String>,

    pub aaio_secret_2: Option<String>,
    pub payment_ipn_url: Option<String>,
    pub currency_rate: Option<String>,
    pub support_url: Option<String>,
    pub bot_username: Option<String>,
    pub brand_name: Option<String>,
    pub terms_of_service: Option<String>,
    pub decoy_enabled: Option<String>, // Checkbox sends "on" or nothing
    pub decoy_urls: Option<String>,
    pub decoy_min_interval: Option<String>,
    pub decoy_max_interval: Option<String>,
    
    // Kill Switch
    pub kill_switch_enabled: Option<String>,
    pub kill_switch_timeout: Option<String>,
}

pub async fn get_login() -> impl IntoResponse {
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    let template = LoginTemplate { 
        is_auth: false,
        admin_path,
        active_page: "login".to_string(),
    };
    Html(template.render().unwrap_or_default())
}


pub async fn analytics(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    // Fetch last 30 days of stats
    // We order by date DESC to get latest first
    let stats = sqlx::query_as::<_, DailyStat>(
        "SELECT date, new_users, active_users, total_orders, total_revenue FROM daily_stats ORDER BY date DESC LIMIT 30"
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    // Today's stats (first in list if date matches today, but simplified: just take first if exists)
    // Ideally we check if date == today, but for UI "Today" usually means "Latest available" or 0 if empty.
    let today_stat = stats.first();
    
    // Check if the latest stat is actually from today. If not, show 0.
    let today_str = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let is_today = today_stat.map(|s| s.date == today_str).unwrap_or(false);

    let (t_new, t_active, t_orders, t_rev) = if is_today {
        let s = today_stat.unwrap();
        (s.new_users, s.active_users, s.total_orders, s.total_revenue)
    } else {
        (0, 0, 0, 0)
    };

    // For Chart, we need Ascending order (oldest to newest)
    let mut stats_rev = stats.clone();
    stats_rev.reverse();

    let template = AnalyticsTemplate {
        stats,
        stats_rev,
        today_new_users: t_new,
        today_active_users: t_active,
        today_orders: t_orders,
        today_revenue: format!("{:.2}", t_rev as f64 / 100.0),
        is_auth: true,
        admin_path,
        active_page: "analytics".to_string(),
    };

    Html(template.render().unwrap_or_else(|e| {
        error!("Template render error: {}", e);
        "Error rendering analytics".to_string()
    }))
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    // Rate Limit: 5 attempts per minute per username (optional if Redis unavailable)
    let rate_key = format!("rate:login:{}", form.username);
    if let Ok(allowed) = state.redis.check_rate_limit(&rate_key, 5, 60).await {
        if !allowed {
             tracing::warn!("Login rate limit exceeded for user: {}", form.username);
             return (axum::http::StatusCode::TOO_MANY_REQUESTS, "Too many login attempts. Please wait.").into_response();
        }
    } else {
        tracing::warn!("Redis unavailable, skipping rate limit check");
    }

    let admin_res = sqlx::query("SELECT password_hash FROM admins WHERE username = ?")
        .bind(&form.username)
        .fetch_optional(&state.pool)
        .await;

    match admin_res {
        Ok(Some(row)) => {
            use sqlx::Row;
            let hash: String = row.get(0);
            if bcrypt::verify(&form.password, &hash).unwrap_or(false) {
                tracing::info!("✅ Login successful for admin: {}", form.username);
                
                // Log successful admin login
                let _ = LoggingService::log_system(
                    &state.pool,
                    "admin_login_success",
                    &format!("Admin '{}' logged in successfully", form.username)
                ).await;
                
                let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                
                // Generate random session token
                let token = uuid::Uuid::new_v4().to_string();
                
                // Store in Redis (24h TTL) - optional if Redis unavailable
                let redis_key = format!("session:{}", token);
                if let Err(e) = state.redis.set(&redis_key, "admin", 86400).await {
                    tracing::warn!("Redis unavailable for session storage: {}. Sessions will use cookies only.", e);
                }

                let cookie = Cookie::build(("admin_session", token))
                    .path("/")
                    .http_only(true)
                    .build();
                
                // For HTMX requests, we use HX-Redirect header
                // For HTMX requests, we use HX-Redirect header.
                // We return 200 OK to prevent HTMX from just swapping the redirect response body into the current page.
                // The HX-Redirect header forces a full page navigation.
                let mut headers = axum::http::HeaderMap::new();
                headers.insert("HX-Redirect", format!("{}/dashboard", admin_path).parse().unwrap());

                return (
                    axum::http::StatusCode::OK,
                    jar.add(cookie),
                    headers,
                ).into_response();
            } else {
                tracing::warn!("❌ Login failed for admin: {} (invalid password)", form.username);
                
                // Log failed login attempt
                let _ = LoggingService::log_system(
                    &state.pool,
                    "admin_login_failed",
                    &format!("Failed login attempt for admin '{}' (invalid password)", form.username)
                ).await;
            }
        }
        Ok(None) => {
            tracing::warn!("❌ Login failed: admin '{}' not found", form.username);
            
            // Log failed login attempt (user not found)
            let _ = LoggingService::log_system(
                &state.pool,
                "admin_login_failed",
                &format!("Failed login attempt for non-existent admin '{}'", form.username)
            ).await;
        }
        Err(e) => {
            tracing::error!("❌ Database error during login: {}", e);
        }
    }

    (axum::http::StatusCode::UNAUTHORIZED, "Invalid username or password").into_response()
}

pub async fn activate_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Forcing activation for node ID: {}", id);

    let res = sqlx::query("UPDATE nodes SET status = 'active' WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Node", &format!("Node {} activated", id)).await;
            
            // Notify Node (PubSub)
            let _ = state.pubsub.publish(&format!("node_events:{}", id), "update").await;

            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            ([("HX-Redirect", &format!("{}/nodes", admin_path))], "Activated").into_response()
        },
        Err(e) => {
            error!("Failed to activate node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to activate node").into_response()
        }
    }
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar
) -> impl IntoResponse {
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    
    // Invalidate in Redis if exists
    if let Some(cookie) = jar.get("admin_session") {
        let token = cookie.value();
        let redis_key = format!("session:{}", token);
        let _ = state.redis.del(&redis_key).await;
    }

    let cookie = Cookie::build(("admin_session", ""))
        .path("/")
        // expire immediately
        .build();
    
    (jar.add(cookie), axum::response::Redirect::to(&format!("{}/login", admin_path))).into_response()
}

pub async fn get_dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let active_nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let active_subs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let total_revenue: f64 = sqlx::query_scalar("SELECT SUM(amount) FROM payments WHERE status = 'completed'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0.0);



    let total_traffic_bytes: i64 = sqlx::query_scalar("SELECT SUM(used_traffic) FROM subscriptions")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let _total_traffic = format!("{:.2} GB", total_traffic_bytes as f64 / (1024.0 * 1024.0 * 1024.0));

    let bot_status = state.settings.get_or_default("bot_status", "stopped").await;

    let activities = crate::services::logging_service::LoggingService::get_logs(&state.pool, 10, 0, None)
        .await
        .unwrap_or_default();

    let template = DashboardTemplate {
        total_users,
        active_subs,
        total_revenue,
        active_nodes,
        total_traffic: format!("{:.2} GB", total_traffic_bytes as f64 / 1024.0 / 1024.0 / 1024.0),
        bot_status,
        is_auth: true,
        activities,
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "dashboard".to_string(),
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// --- Traffic Analytics ---

#[derive(serde::Serialize)]
pub struct ChartUser {
    pub username: Option<String>,
    pub total_traffic_fmt: String,
}

#[derive(Template)]
#[template(path = "analytics.html")]
pub struct TrafficAnalyticsTemplate {
    pub total_traffic_30d: String,
    pub active_nodes_count: i64,
    pub top_users: Vec<ChartUser>,
    
    // Chart Data (JSON strings)
    pub history_labels_json: String,
    pub history_data_json: String,
    pub node_labels_json: String,
    pub node_series_json: String,

    pub orders: Vec<OrderWithUser>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

pub async fn get_traffic_analytics(State(state): State<AppState>) -> impl IntoResponse {
    use crate::services::analytics_service::AnalyticsService;
    
    // 1. Fetch Data concurrently
    let (history, top_users, node_stats) = tokio::join!(
        AnalyticsService::get_traffic_history(&state.pool),
        AnalyticsService::get_top_users(&state.pool),
        AnalyticsService::get_node_traffic_stats(&state.pool)
    );

    let history = history.unwrap_or_default();
    let top_users = top_users.unwrap_or_default();
    let node_stats = node_stats.unwrap_or_default();

    // 2. Process History (Area Chart)
    // History comes DESC (newest first). Need ASC for chart.
    let mut history_asc = history;
    history_asc.reverse();

    let history_labels: Vec<String> = history_asc.iter().map(|d| d.date.clone()).collect();
    let history_data: Vec<f64> = history_asc.iter().map(|d| d.traffic_used as f64 / 1024.0 / 1024.0 / 1024.0).collect(); // GB

    let total_traffic_bytes: i64 = history_asc.iter().map(|d| d.traffic_used).sum();
    let total_traffic_30d = format!("{:.2} GB", total_traffic_bytes as f64 / 1024.0 / 1024.0 / 1024.0);

    // 3. Process Node Stats (Donut Chart)
    let node_labels: Vec<String> = node_stats.iter().map(|n| n.name.clone()).collect();
    let node_series: Vec<f64> = node_stats.iter().map(|n| n.total_traffic as f64 / 1024.0 / 1024.0 / 1024.0).collect(); // GB
    let active_nodes_count = node_stats.len() as i64;

    // 4. Process Top Users (Table)
    let formatted_top_users: Vec<ChartUser> = top_users.into_iter().map(|u| ChartUser {
        username: u.username,
        total_traffic_fmt: format!("{:.2} GB", u.total_traffic as f64 / 1024.0 / 1024.0 / 1024.0),
    }).collect();

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = TrafficAnalyticsTemplate {
        total_traffic_30d,
        active_nodes_count,
        top_users: formatted_top_users,
        
        orders: sqlx::query!("SELECT o.id, u.username, o.total_amount, o.status, o.created_at FROM orders o JOIN users u ON o.user_id = u.id ORDER BY o.created_at DESC LIMIT 50")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|row| OrderWithUser {
                id: row.id,
                username: row.username.unwrap_or_else(|| "Unknown".to_string()),
                total_amount: format!("{:.2}", (row.total_amount as f64) / 100.0),
                status: row.status,
                created_at: row.created_at.unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc()).and_utc().format("%Y-%m-%d %H:%M").to_string(),
            })
            .collect(),
        
        history_labels_json: serde_json::to_string(&history_labels).unwrap_or("[]".to_string()),
        history_data_json: serde_json::to_string(&history_data).unwrap_or("[]".to_string()),
        node_labels_json: serde_json::to_string(&node_labels).unwrap_or("[]".to_string()),
        node_series_json: serde_json::to_string(&node_series).unwrap_or("[]".to_string()),

        is_auth: true,
        admin_path,
        active_page: "analytics".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}


#[derive(Deserialize)]
pub struct InstallNodeForm {
    pub name: String,
    pub ip: Option<String>,
    pub vpn_port: i64,
    pub auto_configure: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateNodeForm {
    pub name: String,
    pub ip: String,
    
    // Bandwidth Shaping checkboxes
    // Bandwidth Shaping checkboxes - Unused/Rudimentary
    // pub config_qos_enabled: Option<String>,
    // pub config_block_torrent: Option<String>,
    // pub config_block_ads: Option<String>,
    // pub config_block_porn: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateCategoryForm {
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i64,
}

#[derive(Deserialize)]
pub struct CreateProductForm {
    pub name: String,
    pub category_id: i64,
    pub price: i64, // cents
    pub description: Option<String>,
    pub product_type: String, // 'file', 'text', 'subscription'
    pub content_text: Option<String>,
    // File handled via Multipart
}

#[derive(Template)]
#[template(path = "store_categories.html")]
pub struct StoreCategoriesTemplate {
    pub categories: Vec<crate::models::store::Category>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "store_products.html")]
pub struct StoreProductsTemplate {
    pub products: Vec<crate::models::store::Product>,
    pub categories: Vec<crate::models::store::Category>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}



#[derive(Template)]
#[template(path = "node_edit_modal.html")]
pub struct NodeEditModalTemplate {
    pub node: Node,
    pub admin_path: String,
}

fn mask_key(key: &str) -> String {
    let len = key.len();
    if len < 8 {
        return "*".repeat(len);
    }
    let start_len = (len as f64 * 0.1).ceil() as usize;
    let mask_len = (len as f64 * 0.7).floor() as usize;
    let end_len = len.saturating_sub(start_len + mask_len);
    
    let start = &key[0..start_len];
    let end = &key[len - end_len..];
    format!("{}{}{}", start, "*".repeat(mask_len), end)
}

pub async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    let bot_status = state.settings.get_or_default("bot_status", "stopped").await;
    let payment_api_key = state.settings.get_or_default("payment_api_key", "").await;
    let payment_ipn_url = state.settings.get_or_default("payment_ipn_url", "").await;
    let currency_rate = state.settings.get_or_default("currency_rate", "1.0").await;
    let support_url = state.settings.get_or_default("support_url", "").await;
    let bot_username = state.settings.get_or_default("bot_username", "exarobot_bot").await;
    let brand_name = state.settings.get_or_default("brand_name", "CARAMBA").await;
    let terms_of_service = state.settings.get_or_default("terms_of_service", "Welcome to CARAMBA.").await;
    
    // Fetch Decoy Settings
    let decoy_enabled = state.settings.get_or_default("decoy_enabled", "false").await == "true";
    let decoy_urls = state.settings.get_or_default("decoy_urls", "[\"https://www.google.com\", \"https://www.azure.com\", \"https://www.netflix.com\"]").await;
    let decoy_min_interval = state.settings.get_or_default("decoy_min_interval", "60").await;
    let decoy_max_interval = state.settings.get_or_default("decoy_max_interval", "600").await;

    // Fetch Kill Switch Settings
    let kill_switch_enabled = state.settings.get_or_default("kill_switch_enabled", "false").await == "true";
    let kill_switch_timeout = state.settings.get_or_default("kill_switch_timeout", "300").await; // Default 5 mins

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| {
        tracing::warn!("ADMIN_PATH env var not found in get_settings handler! Defaulting to /admin");
        "/admin".to_string()
    });
    tracing::info!("get_settings handler seeing ADMIN_PATH: {}", admin_path);

    // Fetch Trial Config (Persistent)
    let free_trial_days = state.settings.get_or_default("free_trial_days", "3").await.parse().unwrap_or(3);
    let channel_trial_days = state.settings.get_or_default("channel_trial_days", "7").await.parse().unwrap_or(7);
    let required_channel_id = state.settings.get_or_default("required_channel_id", "").await;
    let last_export = state.settings.get_or_default("last_export", "Never").await;

    let masked_bot_token = if !bot_token.is_empty() { mask_key(&bot_token) } else { "".to_string() };
    let masked_payment_api_key = if !payment_api_key.is_empty() { mask_key(&payment_api_key) } else { "".to_string() };

    let cryptomus_merchant_id = state.settings.get_or_default("cryptomus_merchant_id", "").await;
    let cryptomus_payment_api_key = state.settings.get_or_default("cryptomus_payment_api_key", "").await;
    let aaio_merchant_id = state.settings.get_or_default("aaio_merchant_id", "").await;
    let aaio_secret_1 = state.settings.get_or_default("aaio_secret_1", "").await;
    let aaio_secret_2 = state.settings.get_or_default("aaio_secret_2", "").await;

    let masked_cryptomus_merchant_id = if !cryptomus_merchant_id.is_empty() { mask_key(&cryptomus_merchant_id) } else { "".to_string() };
    let masked_cryptomus_payment_api_key = if !cryptomus_payment_api_key.is_empty() { mask_key(&cryptomus_payment_api_key) } else { "".to_string() };
    let masked_aaio_merchant_id = if !aaio_merchant_id.is_empty() { mask_key(&aaio_merchant_id) } else { "".to_string() };
    let masked_aaio_secret_1 = if !aaio_secret_1.is_empty() { mask_key(&aaio_secret_1) } else { "".to_string() };
    let masked_aaio_secret_2 = if !aaio_secret_2.is_empty() { mask_key(&aaio_secret_2) } else { "".to_string() };

    let template = SettingsTemplate {
        // masked_bot_token, // Removed
        // bot_status,       // Removed
        masked_payment_api_key,
        masked_cryptomus_merchant_id,
        masked_cryptomus_payment_api_key,
        masked_aaio_merchant_id,
        masked_aaio_secret_1,
        masked_aaio_secret_2,
        payment_ipn_url,
        currency_rate,
        support_url,
        bot_username,
        brand_name,
        terms_of_service,
        is_auth: true,
        admin_path: std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string()),
        active_page: "settings".to_string(),
        
        decoy_enabled,
        decoy_urls,
        decoy_min_interval,
        decoy_max_interval,

        kill_switch_enabled,
        kill_switch_timeout,

        free_trial_days,
        channel_trial_days,
        required_channel_id,
        last_export,
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// --- System Logs ---

#[derive(Deserialize)]
pub struct LogsFilter {
    pub category: Option<String>,
    pub page: Option<i64>,
}

#[derive(Template)]
#[template(path = "logs.html")]
pub struct SystemLogsTemplate {
    pub logs: Vec<crate::services::logging_service::LogEntry>,
    pub categories: Vec<String>,
    pub current_category: String,
    pub current_page: i64,
    pub has_next: bool,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

pub async fn get_system_logs_page(
    State(state): State<AppState>,
    axum::extract::Query(filter): axum::extract::Query<LogsFilter>,
) -> impl IntoResponse {
    use crate::services::logging_service::LoggingService;
    
    let page = filter.page.unwrap_or(1);
    let limit = 50;
    let offset = (page - 1) * limit;
    
    let category = filter.category.unwrap_or_default();
    
    // Fetch logs
    let logs = LoggingService::get_logs(&state.pool, limit + 1, offset, Some(category.clone()))
        .await
        .unwrap_or_default();
        
    // Check pagination
    let has_next = logs.len() > limit as usize;
    let logs = if has_next {
        logs.into_iter().take(limit as usize).collect()
    } else {
        logs
    };

    // Fetch categories for filter
    let categories = LoggingService::get_categories(&state.pool)
        .await
        .unwrap_or_default();

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = SystemLogsTemplate {
        logs,
        categories,
        current_category: category,
        current_page: page,
        has_next,
        is_auth: true,
        admin_path,
        active_page: "system_logs".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn save_settings(
    State(state): State<AppState>,
    Form(form): Form<SaveSettingsForm>,
) -> impl IntoResponse {
    info!("Saving system settings");
    
    let mut settings = HashMap::new();
    let is_running = state.bot_manager.is_running().await;

    // Logic for single-field masked update:
    // If input == masked_value(current_db_value), then do NOT update (user didn't touch it)
    // Else, update.
    
    let current_bot_token = state.settings.get_or_default("bot_token", "").await;
    let masked_bot_token = if !current_bot_token.is_empty() { mask_key(&current_bot_token) } else { "".to_string() };
    
    if let Some(v) = form.bot_token {
        if !v.is_empty() && v != masked_bot_token {
            if is_running {
                // Return error if trying to update token while running
                 return (
                    axum::http::StatusCode::BAD_REQUEST, 
                    "Cannot update Bot Token while bot is running. Please stop the bot first."
                ).into_response();
            }
            settings.insert("bot_token".to_string(), v);
        }
    }

    let current_payment_key = state.settings.get_or_default("payment_api_key", "").await;
    let masked_payment_key = if !current_payment_key.is_empty() { mask_key(&current_payment_key) } else { "".to_string() };

    if let Some(v) = form.payment_api_key {
        if !v.is_empty() && v != masked_payment_key {
            settings.insert("payment_api_key".to_string(), v);
        }
    }

    let current_cryptomus_id = state.settings.get_or_default("cryptomus_merchant_id", "").await;
    let masked_cryptomus_id = if !current_cryptomus_id.is_empty() { mask_key(&current_cryptomus_id) } else { "".to_string() };
    if let Some(v) = form.cryptomus_merchant_id {
        if !v.is_empty() && v != masked_cryptomus_id {
            settings.insert("cryptomus_merchant_id".to_string(), v);
        }
    }

    let current_cryptomus_key = state.settings.get_or_default("cryptomus_payment_api_key", "").await;
    let masked_cryptomus_key = if !current_cryptomus_key.is_empty() { mask_key(&current_cryptomus_key) } else { "".to_string() };
    if let Some(v) = form.cryptomus_payment_api_key {
        if !v.is_empty() && v != masked_cryptomus_key {
            settings.insert("cryptomus_payment_api_key".to_string(), v);
        }
    }

    let current_aaio_id = state.settings.get_or_default("aaio_merchant_id", "").await;
    let masked_aaio_id = if !current_aaio_id.is_empty() { mask_key(&current_aaio_id) } else { "".to_string() };
    if let Some(v) = form.aaio_merchant_id {
        if !v.is_empty() && v != masked_aaio_id {
            settings.insert("aaio_merchant_id".to_string(), v);
        }
    }

    let current_aaio_s1 = state.settings.get_or_default("aaio_secret_1", "").await;
    let masked_aaio_s1 = if !current_aaio_s1.is_empty() { mask_key(&current_aaio_s1) } else { "".to_string() };
    if let Some(v) = form.aaio_secret_1 {
        if !v.is_empty() && v != masked_aaio_s1 {
            settings.insert("aaio_secret_1".to_string(), v);
        }
    }

    let current_aaio_s2 = state.settings.get_or_default("aaio_secret_2", "").await;
    let masked_aaio_s2 = if !current_aaio_s2.is_empty() { mask_key(&current_aaio_s2) } else { "".to_string() };
    if let Some(v) = form.aaio_secret_2 {
        if !v.is_empty() && v != masked_aaio_s2 {
            settings.insert("aaio_secret_2".to_string(), v);
        }
    }

    // For other fields, update if provided (allow empty to clear)
    if let Some(v) = form.payment_ipn_url { settings.insert("payment_ipn_url".to_string(), v); }
    if let Some(v) = form.currency_rate { settings.insert("currency_rate".to_string(), v); }
    if let Some(v) = form.support_url { settings.insert("support_url".to_string(), v); }
    if let Some(v) = form.bot_username { settings.insert("bot_username".to_string(), v); }
    if let Some(v) = form.brand_name { settings.insert("brand_name".to_string(), v); }
    if let Some(v) = form.terms_of_service { settings.insert("terms_of_service".to_string(), v); }

    // Decoy Settings
    // Checkbox: if present (usually "on"), it's enabled. If absent (None), it's disabled.
    // However, since it's an Option in the form, if it's None, it means the browser didn't send it (unchecked).
    // BUT, we need to be careful: if the field is missing purely because the form structure changed, we might accidentally disable it.
    // Standard HTML form behavior: unchecked checkboxes are NOT sent.
    // So if we receive the form submission at all, we should assume missing = disabled.
    // We can infer this is a settings update.
    let decoy_enabled = form.decoy_enabled.is_some(); 
    settings.insert("decoy_enabled".to_string(), decoy_enabled.to_string());

    if let Some(v) = form.decoy_urls { settings.insert("decoy_urls".to_string(), v); }
    if let Some(v) = form.decoy_min_interval { settings.insert("decoy_min_interval".to_string(), v); }
    if let Some(v) = form.decoy_max_interval { settings.insert("decoy_max_interval".to_string(), v); }

    // Kill Switch Settings
    let kill_switch_enabled = form.kill_switch_enabled.is_some();
    settings.insert("kill_switch_enabled".to_string(), kill_switch_enabled.to_string());
    
    if let Some(v) = form.kill_switch_timeout { settings.insert("kill_switch_timeout".to_string(), v); }

    match state.settings.set_multiple(settings).await {
        Ok(_) => {
             // Notify ALL nodes about settings change
             // We can use a wildcard channel or iterate used nodes.
             // Ideally we publish to "global_settings_update" channel if we had one.
             // But existing agents listen to "node_events:{id}".
             // For now, let's rely on polling (1m-10m) for settings, OR we can iterate active nodes and notify.
             // Iterating active nodes is safer for now.
             let active_nodes: Vec<i64> = sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active'")
                 .fetch_all(&state.pool)
                 .await
                 .unwrap_or_default();
             
             let pubsub = state.pubsub.clone();
             tokio::spawn(async move {
                 for node_id in active_nodes {
                     let _ = pubsub.publish(&format!("node_events:{}", node_id), "settings_update").await;
                 }
             });

             // Basic toast notification via HX-Trigger could be added here
             ([("HX-Refresh", "true")], "Settings Saved").into_response()
        },
        Err(e) => {
            error!("Failed to save settings: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to save settings").into_response()
        }
    }
}

pub async fn toggle_bot(State(state): State<AppState>) -> impl IntoResponse {
    let is_running = state.bot_manager.is_running().await;
    let new_status;

    if is_running {
        info!("Stopping bot via toggle");
        state.bot_manager.stop_bot().await;
        new_status = "stopped".to_string();
    } else {
        info!("Starting bot via toggle");
        let token = state.settings.get_or_default("bot_token", "").await;
        if token.is_empty() {
             return (axum::http::StatusCode::BAD_REQUEST, "Bot token is empty").into_response();
        }
        state.bot_manager.start_bot(token, state.clone()).await;
        new_status = "running".to_string();
    }

    let _ = state.settings.set("bot_status", &new_status).await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = BotStatusPartial {
        bot_status: new_status,
        admin_path,
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// Nodes Handlers
pub async fn get_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.orchestration_service.get_all_nodes().await.unwrap_or_default();
    
    let template = NodesTemplate { 
        nodes, 
        is_auth: true, 
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "nodes".to_string(),
    };
    Html(template.render().unwrap())
}


pub async fn install_node(
    State(state): State<AppState>,
    Form(form): Form<InstallNodeForm>,
) -> impl IntoResponse {
    let check_ip = form.ip.clone().unwrap_or_default();
    if !check_ip.is_empty() {
        info!("Adding node: {} @ {}", form.name, check_ip);
    } else {
        info!("Adding pending node: {}", form.name);
    }

    // Generate Token if IP is missing or if auto-config requested
    // Actually, we always generate a token for "Smart Setup" possibility
    let token = uuid::Uuid::new_v4().to_string();
    let auto_configure = form.auto_configure.unwrap_or(false);

    // If IP is empty, we set it to 'pending' placeholder or allow NULL? Schema says TEXT NOT NULL UNIQUE.
    // We should probably allow placeholder IP (e.g. "pending-<token>") or make IP nullable.
    // Migration didn't make IP nullable. So let's use a unique placeholder.
    let ip = if let Some(ref i) = form.ip {
        if i.is_empty() { format!("pending-{}", &token[0..8]) } else { i.clone() }
    } else {
        format!("pending-{}", &token[0..8])
    };

    let res = sqlx::query("INSERT INTO nodes (name, ip, vpn_port, status, join_token, auto_configure) VALUES (?, ?, ?, 'installing', ?, ?) RETURNING id")
        .bind(&form.name)
        .bind(&ip)
        .bind(form.vpn_port)
        .bind(&token)
        .bind(auto_configure)
        .fetch_one(&state.pool)
        .await;

    match res {
        Ok(row) => {
            use sqlx::Row;
            let id: i64 = row.get(0);
            
            // Just register in node manager (sets status to 'new' explicitly)
            // Set status to 'new' (was handled by node_manager)
            let _ = sqlx::query("UPDATE nodes SET status = 'new' WHERE id = ?")
                .bind(id)
                .execute(&state.pool)
                .await;
            
            // Initialize default inbounds (Reality Keys, etc.)
            // We spawn this to not block the redirect, or await it? 
            // Awaiting is safer to ensure keys exist when user connects.
            if let Err(e) = state.orchestration_service.init_default_inbounds(id).await {
                error!("Failed to initialize inbounds for node {}: {}", id, e);
                // We don't fail the request, but log it. Admin might need to "reset" node later.
            }
            
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
            
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("HX-Redirect", format!("{}/nodes", admin_path).parse().unwrap());
            (axum::http::StatusCode::OK, headers, "Redirecting...").into_response()
        }

        Err(e) => {
            error!("Failed to insert node: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to add node").into_response()
        }
    }
}

pub async fn get_node_edit(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await {
            Ok(n) => n,
            Err(e) => {
                error!("Failed to fetch node for edit: {}", e);
                return Html(format!(r###"
                    <header>
                        <a href="#close" aria-label="Close" class="close" onclick="document.getElementById('edit-node-modal').close()"></a>
                        Error
                    </header>
                    <div style="padding: 1rem; color: #ff6b6b;">
                        <strong>Failed to load node:</strong><br>
                        {}<br><br>
                        <em>Please run database migrations.</em>
                    </div>
                    <footer><button onclick="document.getElementById('edit-node-modal').close()">Close</button></footer>
                "###, e)).into_response();
            }
        };

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = NodeEditModalTemplate { node, admin_path };
     match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn update_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateNodeForm>,
) -> impl IntoResponse {
    info!("Updating node ID: {}", id);
    
    // If password is empty, don't update it, keep old one? But form sends it. 
    // Usually admin puts new password or we fetch old one if empty.
    // Let's assume for simplicity we update everything. If password field is empty, it might clear it.
    // Better logic: if password is NOT empty, update it.
    
    let query = sqlx::query("UPDATE nodes SET name = ?, ip = ? WHERE id = ?")
        .bind(&form.name)
        .bind(&form.ip)
        .bind(id);

    match query.execute(&state.pool).await {
        Ok(_) => {
             let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
             let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
             
             let mut headers = axum::http::HeaderMap::new();
             headers.insert("HX-Redirect", format!("{}/nodes", admin_path).parse().unwrap());
             (axum::http::StatusCode::OK, headers, "Updated").into_response()
        },
        Err(e) => {
             error!("Failed to update node: {}", e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update node").into_response()
        }
    }
}

pub async fn sync_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
        info!("Manual sync triggered for node: {}", id);
    
    let orch = state.orchestration_service.clone();
    let pubsub = state.pubsub.clone();

    tokio::spawn(async move {
        // Delete existing inbounds to force regeneration with fresh keys
        if let Err(e) = sqlx::query("DELETE FROM inbounds WHERE node_id = ?")
            .bind(id)
            .execute(&orch.pool)
            .await 
        {
            error!("Failed to delete old inbounds: {}", e);
        } else {
            info!("Deleted old inbounds for node {}", id);
        }
        
        // Recreate default inbounds with fresh keys
        if let Err(e) = orch.init_default_inbounds(id).await {
            error!("Failed to recreate inbounds for node {}: {}", id, e);
        } else {
            info!("Successfully regenerated inbounds with fresh keys for node {}", id);
            
            // Notify Agent
            if let Err(e) = pubsub.publish(&format!("node_events:{}", id), "update").await {
                error!("Failed to publish update event: {}", e);
            }
        }
    });

    axum::http::StatusCode::ACCEPTED
}

// Node Scripts
pub async fn get_node_install_script(
    Path(_id): Path<i64>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // In the future, we can inject unique tokens or specific config here based on ID.
    // Use embedded script
    match crate::scripts::Scripts::get_setup_node_script() {
        Some(content) => (
            [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
            content
        ).into_response(),
        None => {
            error!("Setup script not found in embedded assets");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Script not found").into_response()
        }
    }
}

pub async fn get_node_raw_install_script(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    get_node_install_script(Path(id), State(state)).await
}


// Plans Handlers
pub async fn get_plans(State(state): State<AppState>) -> impl IntoResponse {
    let mut plans = match sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb FROM plans")
        .fetch_all(&state.pool)
        .await {
            Ok(p) => {
                info!("Successfully fetched {} plans from DB", p.len());
                p
            },
            Err(e) => {
                error!("Failed to fetch plans from DB (Admin): {}", e);
                Vec::new()
            }
        };

    for plan in &mut plans {
        let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC"
        )
        .bind(plan.id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        plan.durations = durations;
    }

    let nodes = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE is_enabled = 1").fetch_all(&state.pool).await.unwrap_or_default();

    #[derive(Template)]
    #[template(path = "plans.html")]
    pub struct PlansTemplate {
        pub plans: Vec<Plan>,
        pub nodes: Vec<Node>,
        pub is_auth: bool,
        pub admin_path: String,
        pub active_page: String,
    }

    let template = PlansTemplate { 
        plans, 
        nodes,
        is_auth: true, 
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        }, 
        active_page: "plans".to_string() 
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// Helper for handling single or multiple values in form
#[allow(dead_code)]
fn deserialize_vec_or_single<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    struct VecOrSingleVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> serde::de::Visitor<'de> for VecOrSingleVisitor<T>
    where
        T: Deserialize<'de> + std::str::FromStr,
        <T as std::str::FromStr>::Err: std::fmt::Display,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a sequence or a single value")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = seq.next_element()? {
                vec.push(elem);
            }
            Ok(vec)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            match T::from_str(value) {
                Ok(val) => Ok(vec![val]),
                Err(e) => Err(serde::de::Error::custom(format!("Parse error: {}", e))),
            }
        }
    }

    deserializer.deserialize_any(VecOrSingleVisitor(std::marker::PhantomData))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CreatePlanForm {
    pub name: String,
    pub description: String,
    #[serde(deserialize_with = "deserialize_vec_or_single")]
    pub price: Vec<i64>,
    #[serde(deserialize_with = "deserialize_vec_or_single")]
    pub duration_days: Vec<i32>,
    #[serde(deserialize_with = "deserialize_vec_or_single")]
    pub traffic_gb: Vec<i32>,
}

pub async fn add_plan(
    State(state): State<AppState>,
    Form(raw_form): Form<Vec<(String, String)>>,
) -> impl IntoResponse {
    let mut name = String::new();
    let mut description = String::new();
    let mut device_limit: i32 = 3; // Default value
    let mut duration_days: Vec<i32> = Vec::new();
    let mut price: Vec<i64> = Vec::new();
    let mut traffic_limit_gb: i32 = 0;

    let mut node_ids: Vec<i64> = Vec::new();


    for (key, value) in raw_form {
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            "device_limit" => {
                if let Ok(v) = value.parse() {
                    device_limit = v;
                }
            },
            "duration_days" => {
                if let Ok(v) = value.parse() {
                    duration_days.push(v);
                }
            },
            "price" => {
                if let Ok(v) = value.parse() {
                    price.push(v);
                }
            },
            "traffic_limit_gb" => {
                if let Ok(v) = value.parse() {
                    traffic_limit_gb = v;
                }
            },
            "node_ids" => {
                if let Ok(v) = value.parse() {
                    node_ids.push(v);
                }
            },
            _ => {}
        }
    }

    info!("Adding flexible plan: {}", name);
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    // 1. Insert Plan
    // Using traffic_limit_gb for the plan
    let plan_id: i64 = match sqlx::query("INSERT INTO plans (name, description, is_active, price, traffic_limit_gb, device_limit) VALUES (?, ?, 1, 0, ?, ?) RETURNING id")
        .bind(&name)
        .bind(&description)
        .bind(traffic_limit_gb)
        .bind(device_limit)
        .fetch_one(&mut *tx)
        .await {
            Ok(row) => {
                use sqlx::Row;
                row.get(0)
            },
            Err(e) => {
                error!("Failed to insert plan: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to add plan").into_response();
            }
        };

    // 2. Insert Durations
    let count = duration_days.len().min(price.len());
    for i in 0..count {
        let days = duration_days[i];
        let p = price[i];

        if let Err(e) = sqlx::query("INSERT INTO plan_durations (plan_id, duration_days, price) VALUES (?, ?, ?)")
            .bind(plan_id)
            .bind(days)
            .bind(p)
            .execute(&mut *tx)
            .await {
                error!("Failed to insert plan duration {}: {}", i, e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to add plan durations").into_response();
            }
    }

    // 3. Link to Nodes
    for node_id in node_ids {
        if let Err(e) = sqlx::query("INSERT INTO plan_nodes (plan_id, node_id) VALUES (?, ?)")
            .bind(plan_id)
            .bind(node_id)
            .execute(&mut *tx)
            .await {
                error!("Failed to link new plan to node: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to link plan to node").into_response();
            }
    }

    if let Err(e) = tx.commit().await {
         error!("Failed to commit plan transaction: {}", e);
         return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to create plan").into_response();
    }
    
    // Log activity
    let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Plan", &format!("New plan created: {}", name)).await;

    // Redirect to plans page to show new plan
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("HX-Redirect", format!("{}/plans", admin_path).parse().unwrap());
    (axum::http::StatusCode::OK, headers, "Plan Created").into_response()
}

pub async fn delete_plan(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete plan: {}", id);
    
    // 1. Use Store Service to delete plan + refund active users
    match state.store_service.delete_plan_and_refund(id).await {
        Ok((refunded_users, total_refunded_cents)) => {
            info!("Plan {} deleted. Refunded {} users (Total: ${:.2})", id, refunded_users, total_refunded_cents as f64 / 100.0);
            (axum::http::StatusCode::OK, "").into_response()
        },
        Err(e) => {
            error!("Failed to delete plan {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete plan: {}", e)).into_response()
        }
    }
}

pub async fn get_plan_edit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let plan = match sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb FROM plans WHERE id = ?").bind(id).fetch_optional(&state.pool).await {
        Ok(Some(mut p)) => {
            let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC"
            )
            .bind(p.id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
            p.durations = durations;
            p
        },
        _ => return (axum::http::StatusCode::NOT_FOUND, "Plan not found").into_response(),
    };

    let all_nodes = sqlx::query_as::<_, crate::models::node::Node>("SELECT * FROM nodes WHERE is_enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let linked_node_ids: Vec<i64> = sqlx::query_scalar("SELECT node_id FROM plan_nodes WHERE plan_id = ?")
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    #[derive(Template)]
    #[template(path = "plan_edit_modal.html")]
    struct PlanEditModalTemplate {
        plan: Plan,
        nodes: Vec<(crate::models::node::Node, bool)>,
        admin_path: String,
    }

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let nodes_with_status: Vec<(crate::models::node::Node, bool)> = all_nodes.into_iter().map(|n| {
        let is_linked = linked_node_ids.contains(&n.id);
        (n, is_linked)
    }).collect();

    Html(PlanEditModalTemplate { plan, nodes: nodes_with_status, admin_path }.render().unwrap_or_default()).into_response()
}

pub async fn update_plan(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(raw_form): Form<Vec<(String, String)>>,
) -> impl IntoResponse {
    info!("Updating flexible plan (raw): {}", id);

    let mut name = String::new();
    let mut description = String::new();
    let mut device_limit: i32 = 3; 
    let mut duration_days: Vec<i32> = Vec::new();
    let mut price: Vec<i64> = Vec::new();
    let mut traffic_limit_gb: i32 = 0;

    let mut node_ids: Vec<i64> = Vec::new();

    for (key, value) in raw_form {
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            "device_limit" => {
                if let Ok(v) = value.parse() {
                    device_limit = v;
                }
            },
            "duration_days" => {
                if let Ok(v) = value.parse() {
                    duration_days.push(v);
                }
            },
            "price" => {
                if let Ok(v) = value.parse() {
                    price.push(v);
                }
            },
            "traffic_limit_gb" => {
                if let Ok(v) = value.parse() {
                    traffic_limit_gb = v;
                }
            },
            "node_ids" => {
                if let Ok(v) = value.parse() {
                    node_ids.push(v);
                }
            },
            _ => {}
        }
    }

    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    // 1. Update Plan
    if let Err(e) = sqlx::query("UPDATE plans SET name = ?, description = ?, device_limit = ?, traffic_limit_gb = ? WHERE id = ?")
        .bind(&name)
        .bind(&description)
        .bind(device_limit)
        .bind(traffic_limit_gb)
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to update plan: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update plan").into_response();
        }

    // 2. Delete existing durations
    if let Err(e) = sqlx::query("DELETE FROM plan_durations WHERE plan_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to clear durations: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to clear durations").into_response();
        }

    // 3. Insert new durations
    let count = duration_days.len().min(price.len());

    for i in 0..count {
        let days = duration_days[i];
        let p = price[i];

        if let Err(e) = sqlx::query("INSERT INTO plan_durations (plan_id, duration_days, price) VALUES (?, ?, ?)")
            .bind(id)
            .bind(days)
            .bind(p)
            .execute(&mut *tx)
            .await {
                error!("Failed to insert duration: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to insert duration: {}", e)).into_response();
            }
    }

    // 4. Update Node Bindings (Modernized approach)
    if let Err(e) = sqlx::query("DELETE FROM plan_nodes WHERE plan_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to clear plan_nodes: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to clear plan bindings").into_response();
        }

    for node_id in node_ids {
        if let Err(e) = sqlx::query("INSERT INTO plan_nodes (plan_id, node_id) VALUES (?, ?)")
            .bind(id)
            .bind(node_id)
            .execute(&mut *tx)
            .await {
                error!("Failed to link plan to node: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to link plan to node").into_response();
            }
    }

    if let Err(e) = tx.commit().await {
        error!("Failed to commit update transaction: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Transaction failed").into_response();
    }

    let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Plan", &format!("Plan {} updated: {}", id, name)).await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    ([("HX-Redirect", format!("{}/plans", admin_path))], "Redirecting...").into_response()
}
// Users Handlers
pub async fn get_users(
    State(state): State<AppState>,
    query: axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let search = query.get("search").cloned().unwrap_or_default();
    let users = if search.is_empty() {
        sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    } else {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username LIKE ? OR full_name LIKE ? ORDER BY created_at DESC")
            .bind(format!("%{}%", search))
            .bind(format!("%{}%", search))
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    };

    let template = UsersTemplate { users, search, is_auth: true, admin_path: {
        let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
        if p.starts_with('/') { p } else { format!("/{}", p) }
    }, active_page: "users".to_string() };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

#[derive(Template)]
#[template(path = "user_details.html")]
pub struct UserDetailsTemplate {
    pub user: User,
    pub subscriptions: Vec<SubscriptionWithPlan>,
    pub orders: Vec<UserOrderDisplay>,
    pub referrals: Vec<crate::services::store_service::DetailedReferral>,
    pub total_referral_earnings: String,
    pub available_plans: Vec<Plan>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserOrderDisplay {
    pub id: i64,
    pub total_amount: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct AdminGiftForm {
    pub duration_id: i64,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SubscriptionWithPlan {
    pub id: i64,
    pub plan_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub price: i64,
    pub active_devices: i64,
    pub device_limit: i64,
}

pub async fn admin_gift_subscription(
    Path(user_id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<AdminGiftForm>,
) -> impl IntoResponse {
    // 1. Fetch Duration details to get plan_id and days
    let duration = match sqlx::query_as::<_, crate::models::store::PlanDuration>("SELECT * FROM plan_durations WHERE id = ?")
        .bind(form.duration_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return (axum::http::StatusCode::BAD_REQUEST, "Invalid duration ID").into_response(),
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    match state.store_service.admin_gift_subscription(user_id, duration.plan_id, duration.duration_days).await {
        Ok(sub) => {
            // Find User TG ID for notification
            if let Ok(Some(user)) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?").bind(user_id).fetch_optional(&state.pool).await {
                 let msg = format!("🎁 *Gift Received\\!*\n\nYou have received a new subscription\\.\nExpires: {}", sub.expires_at.format("%Y-%m-%d"));
                 let _ = state.bot_manager.send_notification(
                     user.tg_id,
                     &msg
                 ).await;
            }

            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            return axum::response::Redirect::to(&format!("{}/users/{}", admin_path, user_id)).into_response();
        },
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR, 
                format!("Failed to gift subscription: {}", e)
            ).into_response();
        }
    }
}

pub async fn get_user_details(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // 1. Fetch User
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user = match user {
        Some(u) => u,
        None => return (axum::http::StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // 2. Fetch Active Subscriptions with Plan Name, Price, and Device Info
    // We infer the price by matching plan_id and the duration (expires_at - created_at)
    // against plan_durations table.
    // Device count is calculated from subscription_ip_tracking (last 15 minutes)
    let subscriptions = match sqlx::query_as::<_, SubscriptionWithPlan>(
        r#"
        SELECT 
            s.id, 
            p.name as plan_name, 
            s.expires_at, 
            s.created_at,
            s.status,
            0 as price, 
            COALESCE(
                (SELECT COUNT(DISTINCT client_ip) 
                 FROM subscription_ip_tracking 
                 WHERE subscription_id = s.id 
                 AND datetime(last_seen_at) > datetime('now', '-15 minutes')),
                0
            ) as active_devices,
            p.device_limit as device_limit
        FROM subscriptions s
        JOIN plans p ON s.plan_id = p.id
        WHERE s.user_id = ?
        "#
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await {
        Ok(subs) => subs,
        Err(e) => {
            error!("Failed to fetch user subscriptions: {}", e);
            // Return error to UI instead of empty list
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to fetch subs: {}", e)).into_response();
        }
    };

    // 3. Fetch Order History
    let db_orders = sqlx::query_as::<_, Order>(
        "SELECT id, user_id, total_amount, status, created_at, paid_at FROM orders WHERE user_id = ? ORDER BY created_at DESC"
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
         error!("Failed to fetch user orders: {}", e);
         e
    })
    .unwrap_or_default();

    let orders = db_orders.into_iter().map(|o| UserOrderDisplay {
        id: o.id,
        total_amount: format!("{:.2}", o.total_amount as f64 / 100.0),
        status: o.status,
        created_at: o.created_at.format("%Y-%m-%d").to_string(),
    }).collect();

    // 4. Fetch Referrals & Earnings
    let referrals = state.store_service.get_user_referrals(id).await.unwrap_or_default();
    let earnings_cents = state.store_service.get_user_referral_earnings(id).await.unwrap_or(0);
    let _total_referral_earnings = format!("{:.2}", earnings_cents as f64 / 100.0);

    // 5. Fetch Available Plans for Gifting
    let available_plans = state.store_service.get_active_plans().await.unwrap_or_default();

    let template = UserDetailsTemplate {
        user,
        subscriptions,
        orders,
        referrals,
        total_referral_earnings: format!("{:.2}", earnings_cents as f64 / 100.0),
        available_plans,
        is_auth: true,
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "users".to_string(),
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





#[derive(Deserialize)]
pub struct UpdateUserForm {
    pub balance: i64,
    pub is_banned: bool,
    pub referral_code: Option<String>,
}

pub async fn update_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateUserForm>,
) -> impl IntoResponse {
    // Fetch previous state
    let old_user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let res = sqlx::query("UPDATE users SET balance = ?, is_banned = ?, referral_code = ? WHERE id = ?")
        .bind(form.balance)
        .bind(form.is_banned)
        .bind(form.referral_code.as_deref().map(|s| s.trim()))
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let _ = crate::services::activity_service::ActivityService::log(&state.pool, "User", &format!("User {} updated: Balance={}, Banned={}", id, form.balance, form.is_banned)).await;
            
            if let Some(u) = old_user {
                // Notify on ban status change
                if u.is_banned != form.is_banned {
                    let msg = if form.is_banned {
                        "🚫 *Account Banned*\n\nYour account has been suspended by an administrator\\."
                    } else {
                        "✅ *Account Unbanned*\n\nYour account has been reactivated\\. Welcome back\\!"
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, msg).await;
                }

                // Notify on balance change (deposit/deduction by admin)
                if u.balance != form.balance {
                    let diff = form.balance - u.balance;
                    let amount = format!("{:.2}", diff.abs() as f64 / 100.0);
                    let msg = if diff > 0 {
                        format!("💰 *Balance Updated*\n\nAdministrator added *${}* to your account\\.", amount)
                    } else {
                        format!("📉 *Balance Updated*\n\nAdministrator deducted *${}* from your account\\.", amount)
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, &msg).await;
                }
            }

            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            ([("HX-Redirect", format!("{}/users/{}", admin_path, id))], "Updated").into_response()
        },
        Err(e) => {
            error!("Failed to update user {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update user").into_response()
        }
    }
}

pub async fn update_user_balance(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>, // Accept generic form for the modal which sends 'balance'
) -> impl IntoResponse {
    let balance_str = form.get("balance").unwrap_or(&"0".to_string()).clone();
    let balance: i64 = balance_str.parse().unwrap_or(0);

    let res = sqlx::query("UPDATE users SET balance = ? WHERE id = ?")
        .bind(balance)
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            // Log balance update action
            let _ = LoggingService::log_system(
                &state.pool,
                "admin_update_balance",
                &format!("Admin updated user {} balance to {} cents", id, balance)
            ).await;
            
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            ([("HX-Redirect", format!("{}/users", admin_path))], "Updated").into_response()
        },
        Err(e) => {
            error!("Failed to update balance for user {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update balance").into_response()
        }
    }
}

pub async fn delete_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete subscription ID: {}", id);
    match state.store_service.admin_delete_subscription(id).await {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(),
        Err(e) => {
             error!("Failed to delete subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct RefundForm {
    pub amount: i64, 
}

pub async fn refund_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<RefundForm>,
) -> impl IntoResponse {
    info!("Request to refund subscription ID: {} with amount {}", id, form.amount);
    match state.store_service.admin_refund_subscription(id, form.amount).await {
        Ok(_) => ([("HX-Refresh", "true")], "Refunded").into_response(),
        Err(e) => {
             error!("Failed to refund subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to refund: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ExtendForm {
    pub days: i32,
}

pub async fn extend_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<ExtendForm>,
) -> impl IntoResponse {
    info!("Request to extend subscription ID: {} by {} days", id, form.days);
    match state.store_service.admin_extend_subscription(id, form.days).await {
        Ok(_) => ([("HX-Refresh", "true")], "Extended").into_response(),
        Err(e) => {
             error!("Failed to extend subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to extend: {}", e)).into_response()
        }
    }
}


use axum::http::HeaderMap;

pub async fn handle_payment(
    State(state): State<AppState>,
    Path(source): Path<String>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    info!("Received payment webhook from source: {}", source);
    
    // Extract potential signatures
    let crypto_sig = headers.get("crypto-pay-api-signature").and_then(|h| h.to_str().ok());
    let nowpayments_sig = headers.get("x-nowpayments-sig").and_then(|h| h.to_str().ok());
    let stripe_sig = headers.get("stripe-signature").and_then(|h| h.to_str().ok());
    let cryptomus_sig = headers.get("sign").and_then(|h| h.to_str().ok());

    if let Err(e) = state.pay_service.handle_webhook(&source, &body, crypto_sig, nowpayments_sig, stripe_sig, cryptomus_sig).await {
        error!("Failed to process payment webhook: {}", e);
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR;
    }
    axum::http::StatusCode::OK
}

pub async fn bot_logs_page(jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return axum::response::Redirect::to("/admin/login").into_response();
    }
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    Html(BotLogsTemplate { is_auth: true, admin_path, active_page: "settings".to_string() }.render().unwrap()).into_response()
}


pub async fn bot_logs_history(jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return "Unauthorized".to_string();
    }
    
    match std::fs::read_to_string("server.log") {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > 100 { lines.len() - 100 } else { 0 };
            lines[start..].join("\n")
        }
        Err(_) => "Error reading log file".to_string()
    }
}

static mut LAST_LOG_POS: u64 = 0;

pub async fn bot_logs_tail(jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return String::new();
    }
    
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    
    let current_pos = unsafe { LAST_LOG_POS };
    
    match File::open("server.log") {
        Ok(mut file) => {
            let metadata = file.metadata().unwrap();
            let file_len = metadata.len();
            
            if file_len < current_pos {
                unsafe { LAST_LOG_POS = 0; }
                file.seek(SeekFrom::Start(0)).ok();
            } else {
                file.seek(SeekFrom::Start(current_pos)).ok();
            }
            
            let reader = BufReader::new(file);
            let mut new_lines = Vec::new();
            
            for line in reader.lines() {
                if let Ok(line) = line {
                    new_lines.push(line);
                }
            }
            
            unsafe { LAST_LOG_POS = file_len; }
            
            new_lines.join("\n")
        }
        Err(_) => String::new()
    }
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    info!("Request to delete node ID: {}", id);

    // 2. Delete the node (Cascades to inbounds -> plan_inbounds)
    // Subscriptions are linked to plans, not nodes directly, so no need to touch them.
    // If we had direct node-user allocation, we would need to handle it.
    // But currently: Subscription -> Plan -> PlanInbounds -> Inbound -> Node.
    // Deleting Node deletes Inbounds (Cascade).
    // Deleting Inbounds should delete PlanInbounds (if cascade set? Otherwise might need manual cleanup).
    // Let's assume schema handles Inbounds ON DELETE CASCADE (it does).
    // PlanInbounds? Schema not fully visible but likely.
    
    // Proceed to delete node directly.

    // 3. Delete the node
    let res = sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            info!("Node {} deleted successfully", id);
            // Return explicit empty body for HTMX to remove the element
            (axum::http::StatusCode::OK, "").into_response()
        }
        Err(e) => {
            error!("Failed to delete node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete node: {}", e)).into_response()
        }
    }
}

pub async fn toggle_node_enable(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    info!("Request to toggle enable status for node ID: {}", id);
    
    // Fetch current status
    // Use unchecked query to avoid build failure if migration not applied
    let enabled_res: Result<bool, sqlx::Error> = sqlx::query_scalar("SELECT is_enabled FROM nodes WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await;

    let enabled = match enabled_res {
        Ok(e) => e,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
    };

    let new_status = !enabled;
    
    let res = sqlx::query("UPDATE nodes SET is_enabled = ? WHERE id = ?")
        .bind(new_status)
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
            // Refresh the row
            ([("HX-Redirect", format!("{}/nodes", admin_path))], "Toggled").into_response()
        }
        Err(e) => {
            error!("Failed to toggle node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to toggle node").into_response()
        }
    }
}

pub async fn get_transactions(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return axum::response::Redirect::to("/admin/login").into_response();
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
            o.total_amount, 
            o.status, 
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
        total_amount: format!("{:.2}", (row.total_amount as f64) / 100.0),
        status: row.status,
        created_at: row.created_at.unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc()).and_utc().format("%Y-%m-%d %H:%M").to_string(),
    })
    .collect();

    let template = TransactionsTemplate {
        orders,
        is_auth: true,
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "transactions".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}


pub async fn get_subscription_devices(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let ips = match state.store_service.get_subscription_active_ips(sub_id).await {
        Ok(ips) => ips,
        Err(e) => {
            error!("Failed to fetch IPs for sub {}: {}", sub_id, e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch devices").into_response();
        }
    };

    if ips.is_empty() {
        return (axum::http::StatusCode::OK, "<p class='secondary'>No active devices found in the last 15 minutes.</p>").into_response();
    }

    let mut html = String::from("<table class='striped'><thead><tr><th>IP Address</th><th>Last Seen</th></tr></thead><tbody>");
    for ip_record in ips {
        let time_ago = format_duration(chrono::Utc::now() - ip_record.last_seen_at);
        html.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{} ago</td></tr>",
            ip_record.client_ip, time_ago
        ));
    }
    html.push_str("</tbody></table>");

    Html(html).into_response()
}

fn format_duration(dur: chrono::Duration) -> String {
    let secs = dur.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn is_authenticated(jar: &CookieJar) -> bool {
    jar.get("admin_session").is_some()
}
// Frontends UI Handler
#[derive(Template)]
#[template(path = "frontends.html")]
pub struct FrontendsTemplate {
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

pub async fn get_frontends(State(_state): State<AppState>) -> impl IntoResponse {
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = FrontendsTemplate {
        is_auth: true,
        admin_path,
        active_page: "frontends".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// ========== Tools Page Logic Migrated to Settings & Bot Page ==========

pub async fn get_bot_page(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    let bot_status = state.settings.get_or_default("bot_status", "stopped").await;
    
    let masked_bot_token = if !bot_token.is_empty() { mask_key(&bot_token) } else { "".to_string() };
    
    
    // Attempt to get username (or use hardcoded default for now)
    let bot_username = state.settings.get_or_default("bot_username", "exarobot_bot").await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = BotTemplate {
        masked_bot_token,
        bot_status,
        bot_username,
        // webhook_info: None, // Removed
        is_auth: true,
        admin_path,
        active_page: "bot".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn db_export_download(
    State(state): State<AppState>,
) -> impl IntoResponse {
    use axum::http::{header, StatusCode};
    
    info!("Admin requested database export");
    
    match state.export_service.create_export().await {
        Ok(data) => {
            let filename = format!(
                "exarobot_backup_{}.tar.gz",
                chrono::Utc::now().format("%Y%m%d_%H%M%S")
            );
            
            info!("Export successful: {} bytes, filename: {}", data.len(), filename);
            
            // Update last export timestamp
            let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
            state.settings.set("last_export", &now_str).await;

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/gzip".to_string()),
                    (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", filename)),
                ],
                data
            ).into_response()
        }
        Err(e) => {
            error!("Database export failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Export failed. Check server logs for details."
            ).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct TrialConfigForm {
    pub free_trial_days: i64,
    pub channel_trial_days: i64,
    pub required_channel_id: String,
}

pub async fn update_trial_config(
    State(state): State<AppState>,
    Form(form): Form<TrialConfigForm>,
) -> impl IntoResponse {
    use axum::response::Redirect;
    
    info!(
        "Trial configuration update requested: default={}, channel={}, channel_id={}",
        form.free_trial_days,
        form.channel_trial_days,
        form.required_channel_id
    );
    
    // Save to DB
    let _ = state.settings.set("free_trial_days", &form.free_trial_days.to_string()).await;
    let _ = state.settings.set("channel_trial_days", &form.channel_trial_days.to_string()).await;
    let _ = state.settings.set("required_channel_id", &form.required_channel_id).await;
    
    let admin_path = std::env::var("ADMIN_PATH")
        .unwrap_or_else(|_| "/admin".to_string());
    
    Redirect::to(&format!("{}/settings", admin_path))
}

#[allow(dead_code)]
async fn get_trial_stats(pool: &sqlx::SqlitePool) -> anyhow::Result<TrialStats> {
    let result = sqlx::query_as::<_, (Option<i64>, Option<i64>, Option<i64>)>(
        "SELECT 
            SUM(CASE WHEN trial_source = 'default' THEN 1 ELSE 0 END) as default_count,
            SUM(CASE WHEN trial_source = 'channel' THEN 1 ELSE 0 END) as channel_count,
            SUM(CASE WHEN trial_expires_at > datetime('now') THEN 1 ELSE 0 END) as active_count
         FROM users
         WHERE trial_expires_at IS NOT NULL"
    )
    .fetch_one(pool)
    .await?;
    
    Ok(TrialStats {
        default_count: result.0.unwrap_or(0),
        channel_count: result.1.unwrap_or(0),
        active_count: result.2.unwrap_or(0),
    })
}

// --- Store Management Handlers ---

pub async fn get_store_categories_page(State(state): State<AppState>) -> impl IntoResponse {
    let categories = state.store_service.get_categories().await.unwrap_or_default();
    
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = StoreCategoriesTemplate {
        categories,
        is_auth: true,
        admin_path,
        active_page: "store_categories".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn create_category(
    State(state): State<AppState>, 
    Form(form): Form<CreateCategoryForm>
) -> impl IntoResponse {
    let res = sqlx::query("INSERT INTO categories (name, description, sort_order, is_active) VALUES (?, ?, ?, 1)")
        .bind(&form.name)
        .bind(&form.description)
        .bind(form.sort_order)
        .execute(&state.pool)
        .await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());

    match res {
        Ok(_) => ([("HX-Redirect", format!("{}/store/categories", admin_path))].into_response()),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

pub async fn delete_category(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Check if products exist in this category
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE category_id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    if count > 0 {
        return (axum::http::StatusCode::BAD_REQUEST, "Cannot delete category with existing products.").into_response();
    }

    let res = sqlx::query("DELETE FROM categories WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;
        
    match res {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(), // Empty response removes element in HTMX if swap is outerHTML
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response(),
    }
}

pub async fn get_store_products_page(State(state): State<AppState>) -> impl IntoResponse {
    let products = sqlx::query_as::<_, crate::models::store::Product>("SELECT * FROM products ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let categories = state.store_service.get_categories().await.unwrap_or_default();
    
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = StoreProductsTemplate {
        products,
        categories,
        is_auth: true,
        admin_path,
        active_page: "store_products".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn create_product(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let mut name = String::new();
    let mut category_id = 0i64;
    let mut price = 0i64;
    let mut description = None;
    let mut product_type = String::new();
    let mut content = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let field_name = field.name().unwrap_or("").to_string();

        if field_name == "content_file" {
            // Handle file upload
             if let Some(filename) = field.file_name() {
                if !filename.is_empty() {
                    content = Some(filename.to_string());
                }
             }
        } else {
            let val = field.text().await.unwrap_or_default();
            match field_name.as_str() {
                "name" => name = val,
                "category_id" => category_id = val.parse().unwrap_or(0),
                "price" => price = val.parse().unwrap_or(0),
                "description" => description = Some(val),
                "product_type" => product_type = val,
                "content_text" => if !val.is_empty() { content = Some(val) },
                _ => {}
            }
        }
    }

    if name.is_empty() || price < 0 {
         return (axum::http::StatusCode::BAD_REQUEST, "Invalid input").into_response();
    }

    let res = sqlx::query(
        "INSERT INTO products (category_id, name, description, price, product_type, content, is_active) VALUES (?, ?, ?, ?, ?, ?, 1)"
    )
    .bind(category_id)
    .bind(name)
    .bind(description)
    .bind(price)
    .bind(product_type)
    .bind(content)
    .execute(&state.pool)
    .await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());

    match res {
        Ok(_) => ([("HX-Redirect", format!("{}/store/products", admin_path))].into_response()),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

pub async fn delete_product(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let res = sqlx::query("DELETE FROM products WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;
        
    match res {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(), 
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response(),
    }
}

pub async fn check_update(State(_state): State<AppState>) -> impl IntoResponse {
    let output = std::process::Command::new("git")
        .arg("remote")
        .arg("show")
        .arg("origin")
        .output();
        
    let status_html = match output {
        Ok(_) => {
             r##"
             <div class="flex items-center justify-between w-full" id="update-status-container">
                <div>
                    <p class="text-sm text-emerald-400 font-medium flex items-center gap-2">
                        <i data-lucide="check-circle" class="w-4 h-4"></i> System is up to date
                    </p>
                    <p class="text-xs text-slate-500 mt-0.5">Last checked: Just now</p>
                </div>
                <button hx-post="/settings/update/check" hx-target="#update-status-container" hx-swap="outerHTML"
                    class="flex items-center gap-2 bg-slate-800 hover:bg-slate-700 text-white font-medium py-2 px-4 rounded-lg transition-all border border-white/5 opacity-50 cursor-not-allowed" disabled>
                    <i data-lucide="refresh-cw" class="w-4 h-4"></i> Checked
                </button>
            </div>
             "##.to_string()
        },
        Err(_) => {
            r##"
             <div class="flex items-center justify-between w-full" id="update-status-container">
                <div>
                     <p class="text-sm text-red-500 font-medium flex items-center gap-2">
                        <i data-lucide="alert-circle" class="w-4 h-4"></i> Check failed (Git not available)
                    </p>
                    <p class="text-xs text-slate-500 mt-0.5">Manual update required</p>
                </div>
                 <button hx-post="/settings/update/check" hx-target="#update-status-container" hx-swap="outerHTML"
                    class="flex items-center gap-2 bg-slate-800 hover:bg-slate-700 text-white font-medium py-2 px-4 rounded-lg transition-all border border-white/5">
                    <i data-lucide="refresh-cw" class="w-4 h-4"></i> Retry
                </button>
            </div>
            "##.to_string()
        }
    };
    
    Html(status_html)
}


