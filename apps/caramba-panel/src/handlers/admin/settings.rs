// Settings Module
// Application settings, bot management, database exports  

use axum::{
    extract::{State, Form},
    response::{IntoResponse, Html},
    http::{header, StatusCode},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, error};
use chrono::{DateTime, Duration, Utc};

use crate::AppState;

use super::auth::{get_auth_user, is_authenticated};

// Helper to mask API keys
fn mask_key(key: &str) -> String {
    let len = key.len();
    if len <= 4 {
        "*".repeat(len)
    } else {
        format!("{}...{}", &key[..2], &key[len-2..])
    }
}

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "settings.html")]
pub struct SettingsTemplate {
    pub username: String,
    pub masked_bot_token: String,
    pub masked_payment_api_key: String,
    pub masked_nowpayments_api_key: String,
    pub masked_cryptomus_merchant_id: String,
    pub masked_cryptomus_payment_api_key: String,
    pub masked_aaio_merchant_id: String,
    pub masked_aaio_secret_1: String,
    pub masked_aaio_secret_2: String,
    pub masked_lava_project_id: String,
    pub masked_lava_secret_key: String,
    pub telegram_stars_enabled: bool,
    pub payment_ipn_url: String,
    pub currency_rate: String,
    pub support_url: String,
    pub bot_username: String,
    pub brand_name: String,
    pub terms_of_service: String,
    pub decoy_enabled: bool,
    pub decoy_urls: String,
    pub decoy_min_interval: String,
    pub decoy_max_interval: String,
    pub kill_switch_enabled: bool,
    pub kill_switch_timeout: String,
    pub free_trial_days: i32,
    pub channel_trial_days: i32,
    pub free_trial_traffic_limit: i32,
    pub free_trial_device_limit: i32,
    pub required_channel_id: String,
    pub last_export: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub frontend_mode: String,
    pub miniapp_enabled: bool,
    pub subscription_domain: String,
    // Phase 67
    pub auto_update_panel: bool,
    pub auto_update_agents: bool,
    pub auto_update_frontend: bool,
    pub relay_auth_mode: String,
    pub relay_legacy_usage_last_seen_at: String,
    pub relay_legacy_usage_last_seen_bytes: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "bot_logs.html")]
pub struct BotLogsTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
    pub bot_status: String,
    pub bot_username: String,
    pub subscription_domain: String,
}

#[derive(Template)]
#[template(path = "partials/bot_status.html")]
pub struct BotStatusPartial {
    pub bot_status: String,
    pub admin_path: String,
}

#[derive(Deserialize)]
pub struct SaveSettingsForm {
    pub bot_token: Option<String>,
    pub payment_api_key: Option<String>,
    pub nowpayments_api_key: Option<String>,
    pub cryptomus_merchant_id: Option<String>,
    pub cryptomus_payment_api_key: Option<String>,
    pub aaio_merchant_id: Option<String>,
    pub aaio_secret_1: Option<String>,
    pub aaio_secret_2: Option<String>,
    pub lava_project_id: Option<String>,
    pub lava_secret_key: Option<String>,
    pub telegram_stars_enabled: Option<String>,
    pub payment_ipn_url: Option<String>,
    pub currency_rate: Option<String>,
    pub support_url: Option<String>,
    pub bot_username: Option<String>,
    pub brand_name: Option<String>,
    pub terms_of_service: Option<String>,
    pub decoy_enabled: Option<String>,
    pub decoy_urls: Option<String>,
    pub decoy_min_interval: Option<String>,
    pub decoy_max_interval: Option<String>,
    pub kill_switch_enabled: Option<String>,
    pub kill_switch_timeout: Option<String>,
    pub frontend_mode: Option<String>,
    pub miniapp_enabled: Option<String>,
    pub subscription_domain: Option<String>,
    // Phase 67
    pub auto_update_panel: Option<String>,
    pub auto_update_agents: Option<String>,
    pub auto_update_frontend: Option<String>,
    pub relay_auth_mode: Option<String>,
}

#[derive(Deserialize)]
pub struct TrialConfigForm {
    pub free_trial_days: i32,
    pub channel_trial_days: i32,
    pub free_trial_traffic_limit: i32,
    pub free_trial_device_limit: i32,
    pub required_channel_id: String,
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_settings(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        let admin_path = state.admin_path.clone();
        return axum::response::Redirect::to(&admin_path).into_response();
    }

    let payment_api_key = state.settings.get_or_default("payment_api_key", "").await;
    let nowpayments_api_key = state.settings.get_or_default("nowpayments_api_key", "").await;
    let lava_project_id = state.settings.get_or_default("lava_project_id", "").await;
    let lava_secret_key = state.settings.get_or_default("lava_secret_key", "").await;
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    let telegram_stars_enabled = state.settings.get_or_default("telegram_stars_enabled", "false").await == "true";

    let payment_ipn_url = state.settings.get_or_default("payment_ipn_url", "").await;
    let currency_rate = state.settings.get_or_default("currency_rate", "1.0").await;
    let support_url = state.settings.get_or_default("support_url", "").await;
    let bot_username = state.settings.get_or_default("bot_username", "").await;
    let brand_name = state.settings.get_or_default("brand_name", "CARAMBA").await;
    let terms_of_service = state.settings.get_or_default("terms_of_service", "Welcome to CARAMBA.").await;
    
    let decoy_enabled = state.settings.get_or_default("decoy_enabled", "false").await == "true";
    let decoy_urls = state.settings.get_or_default("decoy_urls", "[]").await;
    let decoy_min_interval = state.settings.get_or_default("decoy_min_interval", "60").await;
    let decoy_max_interval = state.settings.get_or_default("decoy_max_interval", "600").await;

    let kill_switch_enabled = state.settings.get_or_default("kill_switch_enabled", "false").await == "true";
    let kill_switch_timeout = state.settings.get_or_default("kill_switch_timeout", "300").await;

    let admin_path = state.admin_path.clone();

    let free_trial_days = state.settings.get_or_default("free_trial_days", "3").await.parse().unwrap_or(3);
    let channel_trial_days = state.settings.get_or_default("channel_trial_days", "7").await.parse().unwrap_or(7);
    let free_trial_traffic_limit = state.settings.get_or_default("free_trial_traffic_limit", "10").await.parse().unwrap_or(10);
    let free_trial_device_limit = state.settings.get_or_default("free_trial_device_limit", "1").await.parse().unwrap_or(1);
    let required_channel_id = state.settings.get_or_default("required_channel_id", "").await;

    let last_export = state.settings.get_or_default("last_export", "Never").await;

    let frontend_mode = state.settings.get_or_default("frontend_mode", "local").await;
    let miniapp_enabled = state.settings.get_or_default("miniapp_enabled", "true").await == "true";
    let subscription_domain = state.settings.get_or_default("subscription_domain", "").await;

    // Phase 67
    let auto_update_panel = state.settings.get_or_default("auto_update_panel", "false").await == "true";
    let auto_update_agents = state.settings.get_or_default("auto_update_agents", "true").await == "true";
    let auto_update_frontend = state.settings.get_or_default("auto_update_frontend", "false").await == "true";
    let relay_auth_mode = state.settings.get_or_default("relay_auth_mode", "dual").await;
    let relay_legacy_usage_last_seen_at_raw = state.settings.get_or_default("relay_legacy_usage_last_seen_at", "").await;
    let relay_legacy_usage_last_seen_bytes = state.settings.get_or_default("relay_legacy_usage_last_seen_bytes", "0").await;
    let relay_legacy_usage_last_seen_at = if relay_legacy_usage_last_seen_at_raw.trim().is_empty() {
        "never".to_string()
    } else {
        relay_legacy_usage_last_seen_at_raw
    };


    let masked_payment_api_key = if !payment_api_key.is_empty() { mask_key(&payment_api_key) } else { "".to_string() };
    let masked_nowpayments_api_key = if !nowpayments_api_key.is_empty() { mask_key(&nowpayments_api_key) } else { "".to_string() };
    let masked_bot_token = if !bot_token.is_empty() { mask_key(&bot_token) } else { "".to_string() };

    let cryptomus_merchant_id = state.settings.get_or_default("cryptomus_merchant_id", "").await;
    let cryptomus_payment_api_key = state.settings.get_or_default("cryptomus_payment_api_key", "").await;
    let aaio_merchant_id = state.settings.get_or_default("aaio_merchant_id", "").await;
    let aaio_secret_1 = state.settings.get_or_default("aaio_secret_1", "").await;
    let aaio_secret_2 = state.settings.get_or_default("aaio_secret_2", "").await;
    
    let masked_lava_project_id = if !lava_project_id.is_empty() { mask_key(&lava_project_id) } else { "".to_string() };
    let masked_lava_secret_key = if !lava_secret_key.is_empty() { mask_key(&lava_secret_key) } else { "".to_string() };

    let masked_cryptomus_merchant_id = if !cryptomus_merchant_id.is_empty() { mask_key(&cryptomus_merchant_id) } else { "".to_string() };
    let masked_cryptomus_payment_api_key = if !cryptomus_payment_api_key.is_empty() { mask_key(&cryptomus_payment_api_key) } else { "".to_string() };
    let masked_aaio_merchant_id = if !aaio_merchant_id.is_empty() { mask_key(&aaio_merchant_id) } else { "".to_string() };
    let masked_aaio_secret_1 = if !aaio_secret_1.is_empty() { mask_key(&aaio_secret_1) } else { "".to_string() };
    let masked_aaio_secret_2 = if !aaio_secret_2.is_empty() { mask_key(&aaio_secret_2) } else { "".to_string() };

    let template = SettingsTemplate {
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        masked_bot_token,
        masked_payment_api_key,
        masked_nowpayments_api_key,
        masked_cryptomus_merchant_id,
        masked_cryptomus_payment_api_key,
        masked_aaio_merchant_id,
        masked_aaio_secret_1,
        masked_aaio_secret_2,
        masked_lava_project_id,
        masked_lava_secret_key,
        telegram_stars_enabled,
        payment_ipn_url,
        currency_rate,
        support_url,
        bot_username,
        brand_name,
        terms_of_service,
        decoy_enabled,
        decoy_urls,
        decoy_min_interval,
        decoy_max_interval,
        kill_switch_enabled,
        kill_switch_timeout,
        free_trial_days,
        channel_trial_days,
        free_trial_traffic_limit,
        free_trial_device_limit,
        required_channel_id,
        last_export,
        is_auth: true,
        admin_path,
        active_page: "settings".to_string(),
        frontend_mode,
        miniapp_enabled,
        subscription_domain,
        auto_update_panel,
        auto_update_agents,
        auto_update_frontend,
        relay_auth_mode,
        relay_legacy_usage_last_seen_at,
        relay_legacy_usage_last_seen_bytes,
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn save_settings(
    State(state): State<AppState>,
    Form(form): Form<SaveSettingsForm>,
) -> impl IntoResponse {
    info!("Saving system settings");
    
    let mut settings = HashMap::new();
    let is_running = state.bot_manager.is_running().await;
    
    let current_bot_token = state.settings.get_or_default("bot_token", "").await;
    let masked_bot_token = if !current_bot_token.is_empty() { mask_key(&current_bot_token) } else { "".to_string() };
    
     if let Some(v) = form.bot_token {
        let v = v.trim().to_string();
        if !v.is_empty() && v != masked_bot_token {
            if is_running {
                 return (
                    StatusCode::BAD_REQUEST, 
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

    let current_nowpayments_key = state.settings.get_or_default("nowpayments_api_key", "").await;
    let masked_nowpayments_key = if !current_nowpayments_key.is_empty() { mask_key(&current_nowpayments_key) } else { "".to_string() };
    if let Some(v) = form.nowpayments_api_key {
        if !v.is_empty() && v != masked_nowpayments_key {
            settings.insert("nowpayments_api_key".to_string(), v);
        }
    }

    let current_lava_project = state.settings.get_or_default("lava_project_id", "").await;
    let masked_lava_project = if !current_lava_project.is_empty() { mask_key(&current_lava_project) } else { "".to_string() };
    if let Some(v) = form.lava_project_id {
        if !v.is_empty() && v != masked_lava_project {
            settings.insert("lava_project_id".to_string(), v);
        }
    }

    let current_lava_secret = state.settings.get_or_default("lava_secret_key", "").await;
    let masked_lava_secret = if !current_lava_secret.is_empty() { mask_key(&current_lava_secret) } else { "".to_string() };
    if let Some(v) = form.lava_secret_key {
        if !v.is_empty() && v != masked_lava_secret {
            settings.insert("lava_secret_key".to_string(), v);
        }
    }

    if let Some(v) = form.telegram_stars_enabled {
        settings.insert("telegram_stars_enabled".to_string(), v);
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

    if let Some(v) = form.payment_ipn_url { settings.insert("payment_ipn_url".to_string(), v); }
    if let Some(v) = form.currency_rate { settings.insert("currency_rate".to_string(), v); }
    if let Some(v) = form.support_url { settings.insert("support_url".to_string(), v); }
    if let Some(v) = form.bot_username {
        settings.insert("bot_username".to_string(), v.trim().trim_start_matches('@').to_string());
    }
    if let Some(v) = form.brand_name { settings.insert("brand_name".to_string(), v); }
    if let Some(v) = form.terms_of_service { settings.insert("terms_of_service".to_string(), v); }

    if let Some(v) = form.decoy_enabled { settings.insert("decoy_enabled".to_string(), v); }
    if let Some(v) = form.decoy_urls { settings.insert("decoy_urls".to_string(), v); }
    if let Some(v) = form.decoy_min_interval { settings.insert("decoy_min_interval".to_string(), v); }
    if let Some(v) = form.decoy_max_interval { settings.insert("decoy_max_interval".to_string(), v); }

    if let Some(v) = form.kill_switch_enabled { settings.insert("kill_switch_enabled".to_string(), v); }

    if let Some(v) = form.kill_switch_timeout { settings.insert("kill_switch_timeout".to_string(), v); }

    if let Some(v) = form.frontend_mode { settings.insert("frontend_mode".to_string(), v); }
    if let Some(v) = form.miniapp_enabled { settings.insert("miniapp_enabled".to_string(), v); }
    if let Some(v) = form.subscription_domain { settings.insert("subscription_domain".to_string(), v); }

    // Phase 67
    if let Some(v) = form.auto_update_panel { settings.insert("auto_update_panel".to_string(), v); }
    if let Some(v) = form.auto_update_agents { settings.insert("auto_update_agents".to_string(), v); }
    if let Some(v) = form.auto_update_frontend { settings.insert("auto_update_frontend".to_string(), v); }
    if let Some(v) = form.relay_auth_mode {
        let normalized = v.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "legacy" | "v1" | "dual") {
            if normalized == "v1" {
                let last_seen_raw = state
                    .settings
                    .get_or_default("relay_legacy_usage_last_seen_at", "")
                    .await;
                if !last_seen_raw.trim().is_empty() {
                    if let Ok(last_seen) = DateTime::parse_from_rfc3339(&last_seen_raw) {
                        let last_seen_utc = last_seen.with_timezone(&Utc);
                        let age = Utc::now().signed_duration_since(last_seen_utc);
                        let guard_window = Duration::hours(24);
                        if age < guard_window {
                            let last_bytes = state
                                .settings
                                .get_or_default("relay_legacy_usage_last_seen_bytes", "0")
                                .await;
                            return (
                                StatusCode::BAD_REQUEST,
                                format!(
                                    "Cannot switch relay_auth_mode to v1: legacy relay traffic was observed at {} ({} bytes). Keep dual mode until no legacy traffic is seen for 24 hours.",
                                    last_seen_utc.to_rfc3339(),
                                    last_bytes
                                ),
                            )
                                .into_response();
                        }
                    }
                }
            }
            settings.insert("relay_auth_mode".to_string(), normalized);
        }
    }

    match state.settings.set_multiple(settings).await {
        Ok(_) => {
             let active_nodes = state.store_service.get_active_node_ids().await.unwrap_or_default();
             
             let pubsub = state.pubsub.clone();
             tokio::spawn(async move {
                 for node_id in active_nodes {
                     let _ = pubsub.publish(&format!("node_events:{}", node_id), "settings_update").await;
                 }
             });

             ([(("HX-Refresh", "true"))], "Settings Saved").into_response()
        },
        Err(e) => {
            error!("Failed to save settings: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save settings").into_response()
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
             return (StatusCode::BAD_REQUEST, "Bot token is empty").into_response();
        }
        if !state.bot_manager.start_bot(token, state.clone()).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to start bot. Check token and logs.").into_response();
        }
        new_status = "running".to_string();
    }

    let _ = state.settings.set("bot_status", &new_status).await;

    let admin_path = state.admin_path.clone();

    let template = BotStatusPartial {
        bot_status: new_status,
        admin_path,
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}



pub async fn bot_logs_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return axum::response::Redirect::to(&format!("{}/login", state.admin_path)).into_response();
    }
    
    let is_running = state.bot_manager.is_running().await;
    let bot_status = if is_running { "running".to_string() } else { "stopped".to_string() };
    let bot_username = state.settings.get_or_default("bot_username", "").await;
    let subscription_domain = state.settings.get_or_default("subscription_domain", "").await;
    
    let admin_path = state.admin_path.clone();
    
    Html(BotLogsTemplate { 
        is_auth: true, 
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()), 
        admin_path, 
        active_page: "settings".to_string(),
        bot_status,
        bot_username,
        subscription_domain,
    }.render().unwrap()).into_response()
}

pub async fn bot_logs_history(
    State(_state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&_state, &jar).await {
        return "Unauthorized".to_string();
    }

    // Prefer dedicated bot.log stream. Fallback to filtered server.log for legacy installs.
    if let Some(history) = read_log_history("bot.log", 300, false) {
        return history;
    }

    read_log_history("server.log", 300, true).unwrap_or_else(|| "No bot logs available yet".to_string())
}

static LAST_BOT_LOG_POS: AtomicU64 = AtomicU64::new(0);
static LAST_SERVER_LOG_POS: AtomicU64 = AtomicU64::new(0);

pub async fn bot_logs_tail(
    State(_state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&_state, &jar).await {
        return String::new();
    }
    
    if Path::new("bot.log").exists() {
        return read_log_tail("bot.log", &LAST_BOT_LOG_POS, false).unwrap_or_default();
    }

    read_log_tail("server.log", &LAST_SERVER_LOG_POS, true).unwrap_or_default()
}

fn read_log_history(path: &str, limit: usize, filter_bot_only: bool) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let lines = content
        .lines()
        .filter(|line| !filter_bot_only || is_bot_log_line(line))
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    Some(lines[start..].join("\n"))
}

fn read_log_tail(path: &str, pos: &AtomicU64, filter_bot_only: bool) -> Option<String> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let mut file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let file_len = metadata.len();
    let current_pos = pos.load(Ordering::Relaxed);

    if file_len < current_pos {
        pos.store(0, Ordering::Relaxed);
        file.seek(SeekFrom::Start(0)).ok()?;
    } else {
        file.seek(SeekFrom::Start(current_pos)).ok()?;
    }

    let reader = BufReader::new(file);
    let mut new_lines = Vec::new();
    for line in reader.lines() {
        let line = line.ok()?;
        if !filter_bot_only || is_bot_log_line(&line) {
            new_lines.push(line);
        }
    }

    pos.store(file_len, Ordering::Relaxed);
    Some(new_lines.join("\n"))
}

fn is_bot_log_line(line: &str) -> bool {
    line.contains("caramba_panel::bot")
        || line.contains("caramba_panel::bot_manager")
        || line.contains("teloxide")
        || line.contains("Bot connected as")
        || line.contains("Received message:")
        || line.contains("Received callback:")
}

pub async fn export_database(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    info!("Database export requested");

    let export_service = crate::services::export_service::ExportService::new();
    let export_result = export_service.create_export().await;

    match export_result {
        Ok(data) => {
            let filename = format!("panel_backup_{}.sql.gz", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
            
            let _ = state.settings.set("last_export", &chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string()).await;
            
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
    
    let _ = state.settings.set("free_trial_days", &form.free_trial_days.to_string()).await;
    let _ = state.settings.set("channel_trial_days", &form.channel_trial_days.to_string()).await;
    let _ = state.settings.set("free_trial_traffic_limit", &form.free_trial_traffic_limit.to_string()).await;
    let _ = state.settings.set("free_trial_device_limit", &form.free_trial_device_limit.to_string()).await;
    let _ = state.settings.set("required_channel_id", &form.required_channel_id).await;
    
    if let Err(e) = state.catalog_service.update_trial_plan_limits(form.free_trial_device_limit, form.free_trial_traffic_limit).await {
        error!("Failed to update trial plan limits: {}", e);
    }

    Redirect::to(&format!("{}/settings", state.admin_path)).into_response()
}


pub async fn check_update(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // Stub for update check
    (
        axum::http::StatusCode::OK,
        [("HX-Trigger", "update-checked")],
        "Up to date"
    ).into_response()
}
