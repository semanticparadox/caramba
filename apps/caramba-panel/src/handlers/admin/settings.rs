// Settings Module
// Application settings, bot management, database exports

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{error, info};
use uuid::Uuid;

use crate::AppState;

use super::auth::{get_auth_user, is_authenticated};

// Helper to mask API keys
fn mask_key(key: &str) -> String {
    let len = key.len();
    if len <= 4 {
        "*".repeat(len)
    } else {
        format!("{}...{}", &key[..2], &key[len - 2..])
    }
}

fn is_checkbox_enabled(value: Option<&str>) -> bool {
    value
        .map(|v| {
            let normalized = v.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "true" | "1" | "on" | "yes")
        })
        .unwrap_or(false)
}

fn normalize_base_url(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return value;
    }
    if !value.starts_with("http://") && !value.starts_with("https://") {
        value = format!("https://{}", value);
    }
    while value.ends_with('/') {
        value.pop();
    }
    value
}

fn normalize_url_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }
    let mut normalized = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    };
    while normalized.ends_with('/') && normalized.len() > 1 {
        normalized.pop();
    }
    normalized
}

fn join_base_and_path(base_url: &str, path: &str) -> String {
    let normalized_path = normalize_url_path(path);
    if base_url.trim().is_empty() {
        return format!("https://YOUR_PANEL_DOMAIN{}", normalized_path);
    }
    format!("{}{}", normalize_base_url(base_url), normalized_path)
}

fn release_asset_url(version: &str, asset_name: &str) -> String {
    format!(
        "https://github.com/semanticparadox/caramba/releases/download/{}/{}",
        version, asset_name
    )
}

async fn ensure_installer_enrollment_key(pool: &sqlx::PgPool) -> Result<String, sqlx::Error> {
    if let Some(existing) = sqlx::query_scalar::<_, String>(
        r#"
        SELECT key
        FROM api_keys
        WHERE is_active = TRUE
          AND type = 'enrollment'
          AND (max_uses IS NULL OR current_uses < max_uses)
          AND (expires_at IS NULL OR expires_at > NOW())
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?
    {
        return Ok(existing);
    }

    let new_key = format!("EXA-ENROLL-{}", Uuid::new_v4().to_string().to_uppercase());
    sqlx::query(
        r#"
        INSERT INTO api_keys (key, name, type, max_uses, current_uses, is_active, created_at)
        VALUES ($1, 'Auto Installer Enrollment Key', 'enrollment', NULL, 0, TRUE, CURRENT_TIMESTAMP)
        "#,
    )
    .bind(&new_key)
    .execute(pool)
    .await?;

    Ok(new_key)
}

async fn resolve_internal_api_token(state: &AppState) -> (String, bool, String) {
    let env_token = std::env::var("INTERNAL_API_TOKEN")
        .unwrap_or_default()
        .trim()
        .to_string();
    if !env_token.is_empty() {
        return (env_token, true, "env".to_string());
    }

    let saved_token = state
        .settings
        .get_or_default("internal_api_token", "")
        .await
        .trim()
        .to_string();
    if !saved_token.is_empty() {
        return (saved_token, true, "settings".to_string());
    }

    let generated = format!(
        "EXA-INTERNAL-{}",
        Uuid::new_v4().simple().to_string().to_uppercase()
    );
    match state.settings.set("internal_api_token", &generated).await {
        Ok(_) => (generated, true, "generated".to_string()),
        Err(e) => {
            error!(
                "Failed to generate/persist internal API token in settings: {}",
                e
            );
            ("".to_string(), false, "missing".to_string())
        }
    }
}

fn service_file_candidates(service_name: &str) -> [String; 3] {
    [
        format!("/etc/systemd/system/{}", service_name),
        format!("/usr/lib/systemd/system/{}", service_name),
        format!("/lib/systemd/system/{}", service_name),
    ]
}

fn service_exists(service_name: &str) -> bool {
    service_file_candidates(service_name)
        .iter()
        .any(|path| Path::new(path).exists())
}

fn is_service_active(service_name: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", service_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_service_enabled(service_name: &str) -> bool {
    Command::new("systemctl")
        .args(["is-enabled", "--quiet", service_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn normalize_worker_role(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "sub" => Some("sub"),
        "bot" => Some("bot"),
        _ => None,
    }
}

fn worker_asset_name(role: &str) -> &'static str {
    match role {
        "sub" => "caramba-sub",
        "bot" => "caramba-bot",
        _ => "caramba-sub",
    }
}

async fn fetch_worker_update_reports(pool: &sqlx::PgPool) -> Vec<WorkerUpdateReportView> {
    let rows: Vec<WorkerUpdateReportRow> = sqlx::query_as(
        r#"
        SELECT role, worker_id, current_version, target_version, status, message, created_at
        FROM worker_update_reports
        ORDER BY id DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| WorkerUpdateReportView {
            role: row.role.to_ascii_uppercase(),
            worker_id: row.worker_id,
            current_version: row.current_version.unwrap_or_else(|| "-".to_string()),
            target_version: row.target_version.unwrap_or_else(|| "-".to_string()),
            status: row.status.to_ascii_uppercase(),
            message: row.message.unwrap_or_default(),
            created_at: row.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        })
        .collect()
}

pub(crate) fn format_relative_time(dt: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(dt);
    if diff < Duration::seconds(60) {
        format!("{}s ago", diff.num_seconds().max(0))
    } else if diff < Duration::minutes(60) {
        format!("{}m ago", diff.num_minutes())
    } else if diff < Duration::hours(24) {
        format!("{}h ago", diff.num_hours())
    } else {
        format!("{}d ago", diff.num_days())
    }
}

pub(crate) async fn fetch_worker_inventory(pool: &sqlx::PgPool) -> Vec<WorkerInventoryView> {
    let rows: Vec<WorkerRuntimeStatusRow> = sqlx::query_as(
        r#"
        SELECT role, worker_id, current_version, target_version, last_state, last_message, last_seen
        FROM worker_runtime_status
        ORDER BY role ASC, last_seen DESC
        LIMIT 200
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| {
            let is_online = Utc::now().signed_duration_since(row.last_seen) <= Duration::minutes(3);
            let current_v = row.current_version.unwrap_or_default();
            let target_v = row.target_version.unwrap_or_default();

            // Корректное семвер-сравнение: обновление нужно только если target СТРОГО НОВЕЕ current.
            // Без этой проверки панель показывала "нужно обновление: v0.9.1" при current="0.9.7".
            let update_available =
                crate::handlers::api::internal::should_offer_worker_update(&target_v, &current_v);

            WorkerInventoryView {
                role: row.role.to_ascii_uppercase(),
                worker_id: row.worker_id,
                current_version: if current_v.is_empty() {
                    "-".to_string()
                } else {
                    current_v
                },
                target_version: if target_v.is_empty() {
                    "-".to_string()
                } else {
                    target_v
                },
                last_state: row.last_state.to_ascii_uppercase(),
                last_message: row.last_message.unwrap_or_default(),
                is_online,
                online_label: if is_online {
                    "online".to_string()
                } else {
                    "offline".to_string()
                },
                last_seen: row.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                last_seen_ago: format_relative_time(row.last_seen),
                update_available,
            }
        })
        .collect()
}

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "settings.html")]
pub struct SettingsTemplate {
    pub current_version: String,
    pub active_nodes_count: usize,
    pub telegram_stars_enabled: bool,
    pub manual_enabled: bool,
    pub nowpayments_api_key: String,
    pub masked_nowpayments_api_key: String,
    pub masked_nowpayments_ipn_secret: String,
    pub masked_payment_api_key: String,
    pub masked_cryptomus_merchant_id: String,
    pub masked_cryptomus_payment_api_key: String,
    pub masked_aaio_merchant_id: String,
    pub masked_aaio_secret_1: String,
    pub masked_aaio_secret_2: String,
    pub username: String,
    pub masked_bot_token: String,
    pub masked_lava_project_id: String,
    pub masked_lava_secret_key: String,
    pub masked_stripe_secret_key: String,
    pub masked_stripe_webhook_secret: String,
    pub payment_ipn_url: String,
    pub currency_rate: String,
    pub stars_per_usd: String,
    pub support_url: String,
    pub panel_url: String,
    pub panel_url_display: String,
    pub admin_ui_url_display: String,
    pub internal_api_token: String,
    pub internal_api_token_present: bool,
    pub internal_api_token_source: String,
    pub bot_username: String,
    pub brand_name: String,
    pub referral_bonus_percent: String,
    pub referral_referrer_signup_bonus_cents: String,
    pub referral_referred_signup_bonus_cents: String,
    // Бонусный трафик (services/bonus_traffic.rs). 0 = выключено.
    pub signup_bonus_traffic_mb: String,
    pub referral_bonus_traffic_mb_referrer: String,
    pub referral_bonus_traffic_mb_referee: String,
    pub admin_notification_tg_ids: String,
    pub terms_of_service: String,
    pub bot_buttons_mode: String,
    /// Язык для пользователей без выбранного `users.language_code`: "ru" | "en".
    pub default_language: String,
    pub bot_support_button_always_on: bool,
    pub decoy_enabled: bool,
    pub decoy_urls: String,
    pub decoy_min_interval: String,
    pub decoy_max_interval: String,
    pub kill_switch_enabled: bool,
    pub kill_switch_timeout: String,
    pub last_export: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub deployment_mode: String,
    pub local_panel_detected: bool,
    pub local_sub_detected: bool,
    pub local_bot_detected: bool,
    pub local_node_detected: bool,
    pub local_panel_active: bool,
    pub local_sub_active: bool,
    pub local_bot_active: bool,
    pub local_node_active: bool,
    pub local_panel_enabled: bool,
    pub local_sub_enabled: bool,
    pub local_bot_enabled: bool,
    pub local_node_enabled: bool,
    pub auto_update_agents: bool,
    pub agent_latest_version: String,
    pub agent_update_url: String,
    pub agent_update_hash: String,
    pub sub_worker_target_version: String,
    pub sub_worker_update_url: String,
    pub bot_worker_target_version: String,
    pub bot_worker_update_url: String,
    pub worker_inventory: Vec<WorkerInventoryView>,
    pub worker_total_count: usize,
    pub worker_online_count: usize,
    pub worker_update_reports: Vec<WorkerUpdateReportView>,
    pub relay_legacy_usage_last_seen_at: String,
    pub relay_legacy_usage_last_seen_bytes: String,
    pub installer_enrollment_key: String,
    pub installer_sub_token: String,
    pub installer_node_command: String,
    pub installer_sub_command: String,
    pub installer_bot_command: String,
    pub installer_sub_token_ready: bool,
    pub subscription_domain: String,
    pub relay_auth_mode: String,
    // WATA
    pub wata_enabled: bool,
    pub masked_wata_jwt_token: String,
    pub masked_wata_webhook_secret: String,
    // Paypalych (pal24.pro / pally.info — SBP + USDT TRC20)
    pub paypalych_enabled: bool,
    pub masked_paypalych_api_token: String,
    pub masked_paypalych_shop_id: String,
    pub masked_paypalych_webhook_secret: String,
    // CrystalPay
    pub crystalpay_enabled: bool,
    pub masked_crystalpay_login: String,
    pub masked_crystalpay_secret: String,
    pub masked_crystalpay_salt: String,
    // Tribute
    pub tribute_enabled: bool,
    pub masked_tribute_api_key: String,
    pub masked_tribute_webhook_secret: String,
    // BTCPay Server
    pub btcpay_enabled: bool,
    pub btcpay_url: String,
    pub masked_btcpay_api_key: String,
    pub masked_btcpay_store_id: String,
    pub masked_btcpay_webhook_secret: String,
    // OxaPay
    pub oxapay_enabled: bool,
    pub masked_oxapay_merchant_key: String,
    // Coinbase Commerce
    pub coinbase_enabled: bool,
    pub masked_coinbase_api_key: String,
    pub masked_coinbase_webhook_secret: String,
    // Plisio
    pub plisio_enabled: bool,
    pub masked_plisio_api_key: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct WorkerUpdateReportRow {
    role: String,
    worker_id: String,
    current_version: Option<String>,
    target_version: Option<String>,
    status: String,
    message: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct WorkerUpdateReportView {
    pub role: String,
    pub worker_id: String,
    pub current_version: String,
    pub target_version: String,
    pub status: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct WorkerRuntimeStatusRow {
    role: String,
    worker_id: String,
    current_version: Option<String>,
    target_version: Option<String>,
    last_state: String,
    last_message: Option<String>,
    last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct WorkerInventoryView {
    pub role: String,
    pub worker_id: String,
    pub current_version: String,
    pub target_version: String,
    pub last_state: String,
    pub last_message: String,
    pub is_online: bool,
    pub online_label: String,
    pub last_seen: String,
    pub last_seen_ago: String,
    /// true только если target_version СТРОГО НОВЕЕ current_version (семвер-сравнение).
    /// Устраняет ложный "нужно обновление" когда target="0.9.1" и current="0.9.7".
    pub update_available: bool,
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
    pub miniapp_domain: String,
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
    pub stripe_secret_key: Option<String>,
    pub stripe_webhook_secret: Option<String>,
    pub manual_enabled: Option<String>,
    pub telegram_stars_enabled: Option<String>,
    pub payment_ipn_url: Option<String>,
    pub currency_rate: Option<String>,
    pub stars_per_usd: Option<String>,
    pub support_url: Option<String>,
    pub panel_url: Option<String>,
    pub bot_username: Option<String>,
    pub brand_name: Option<String>,
    pub referral_bonus_percent: Option<String>,
    pub referral_referrer_signup_bonus_cents: Option<String>,
    pub referral_referred_signup_bonus_cents: Option<String>,
    pub signup_bonus_traffic_mb: Option<String>,
    pub referral_bonus_traffic_mb_referrer: Option<String>,
    pub referral_bonus_traffic_mb_referee: Option<String>,
    pub admin_notification_tg_ids: Option<String>,
    pub terms_of_service: Option<String>,
    pub bot_buttons_mode: Option<String>,
    pub default_language: Option<String>,
    pub bot_support_button_always_on: Option<String>,
    pub decoy_enabled: Option<String>,
    pub decoy_urls: Option<String>,
    pub decoy_min_interval: Option<String>,
    pub decoy_max_interval: Option<String>,
    pub kill_switch_enabled: Option<String>,
    pub kill_switch_timeout: Option<String>,
    pub deployment_mode: Option<String>,
    pub local_sub_enabled: Option<String>,
    pub local_bot_enabled: Option<String>,
    pub auto_update_agents: Option<String>,
    pub agent_latest_version: Option<String>,
    pub agent_update_url: Option<String>,
    pub agent_update_hash: Option<String>,
    pub expiry_reminders_enabled: Option<String>,
    pub expiry_hours_threshold: Option<String>,
    pub nowpayments_ipn_secret: Option<String>,
    // WATA
    pub wata_enabled: Option<String>,
    pub wata_jwt_token: Option<String>,
    pub wata_webhook_secret: Option<String>,
    // Paypalych
    pub paypalych_enabled: Option<String>,
    pub paypalych_api_token: Option<String>,
    pub paypalych_shop_id: Option<String>,
    pub paypalych_webhook_secret: Option<String>,
    // CrystalPay
    pub crystalpay_enabled: Option<String>,
    pub crystalpay_login: Option<String>,
    pub crystalpay_secret: Option<String>,
    pub crystalpay_salt: Option<String>,
    // Tribute
    pub tribute_enabled: Option<String>,
    pub tribute_api_key: Option<String>,
    pub tribute_webhook_secret: Option<String>,
    // BTCPay Server
    pub btcpay_enabled: Option<String>,
    pub btcpay_url: Option<String>,
    pub btcpay_api_key: Option<String>,
    pub btcpay_store_id: Option<String>,
    pub btcpay_webhook_secret: Option<String>,
    // OxaPay
    pub oxapay_enabled: Option<String>,
    pub oxapay_merchant_key: Option<String>,
    // Coinbase Commerce
    pub coinbase_enabled: Option<String>,
    pub coinbase_api_key: Option<String>,
    pub coinbase_webhook_secret: Option<String>,
    // Plisio
    pub plisio_enabled: Option<String>,
    pub plisio_api_key: Option<String>,
}

#[derive(Deserialize)]
pub struct QueueWorkerUpdateForm {
    pub role: String,
    pub version: Option<String>,
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_settings(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        let admin_path = state.admin_path.clone();
        return axum::response::Redirect::to(&admin_path).into_response();
    }

    let payment_api_key = state.settings.get_or_default("payment_api_key", "").await;
    let nowpayments_api_key = state
        .settings
        .get_or_default("nowpayments_api_key", "")
        .await;
    let lava_project_id = state.settings.get_or_default("lava_project_id", "").await;
    let lava_secret_key = state.settings.get_or_default("lava_secret_key", "").await;
    let stripe_secret_key = state.settings.get_or_default("stripe_secret_key", "").await;
    let stripe_webhook_secret = state
        .settings
        .get_or_default("stripe_webhook_secret", "")
        .await;
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    let telegram_stars_enabled = state
        .settings
        .get_or_default("telegram_stars_enabled", "false")
        .await
        == "true";
    let manual_enabled = state
        .settings
        .get_or_default("manual_enabled", "false")
        .await
        == "true";
    let current_nowpayments_ipn = state
        .settings
        .get_or_default("nowpayments_ipn_secret", "")
        .await;
    let masked_nowpayments_ipn = if !current_nowpayments_ipn.is_empty() {
        mask_key(&current_nowpayments_ipn)
    } else {
        "".to_string()
    };

    let payment_ipn_url = state.settings.get_or_default("payment_ipn_url", "").await;
    let currency_rate = state.settings.get_or_default("currency_rate", "1.0").await;
    // Rendered through the same parser the payment path uses, so the form always
    // shows the rate that is actually in force (a stored out-of-range or garbage
    // value displays as the fallback rather than lying about what buyers pay).
    let stars_per_usd = crate::services::payment::stars::stars_per_usd(&state.settings)
        .await
        .to_string();
    let support_url = state.settings.get_or_default("support_url", "").await;
    let panel_url_setting = state.settings.get_or_default("panel_url", "").await;
    let panel_url_env = std::env::var("PANEL_URL").unwrap_or_default();
    let panel_url = if !panel_url_setting.trim().is_empty() {
        normalize_base_url(&panel_url_setting)
    } else if !panel_url_env.trim().is_empty() {
        normalize_base_url(&panel_url_env)
    } else {
        "".to_string()
    };
    let panel_url_display = if panel_url.is_empty() {
        "https://YOUR_PANEL_DOMAIN".to_string()
    } else {
        panel_url.clone()
    };
    let admin_ui_url_display = join_base_and_path(&panel_url_display, &state.admin_path);
    let (internal_api_token, internal_api_token_present, internal_api_token_source) =
        resolve_internal_api_token(&state).await;
    let bot_username = state.settings.get_or_default("bot_username", "").await;
    let brand_name = state.settings.get_or_default("brand_name", "CARAMBA").await;
    let referral_bonus_percent = state
        .settings
        .get_or_default("referral_bonus_percent", "10")
        .await;
    let referral_referrer_signup_bonus_cents = state
        .settings
        .get_or_default("referral_referrer_signup_bonus_cents", "0")
        .await;
    let referral_referred_signup_bonus_cents = state
        .settings
        .get_or_default("referral_referred_signup_bonus_cents", "0")
        .await;
    // Бонусный трафик. Отдаём через тот же парсер, что и грант-путь, поэтому
    // форма всегда показывает действующее значение (мусор => 0 = выключено).
    let bonus_setting = async |key: &str| {
        crate::services::bonus_traffic::parse_bonus_mb(state.settings.get(key).await.as_deref())
            .to_string()
    };
    let signup_bonus_traffic_mb =
        bonus_setting(crate::services::bonus_traffic::SETTING_SIGNUP_BONUS_MB).await;
    let referral_bonus_traffic_mb_referrer =
        bonus_setting(crate::services::bonus_traffic::SETTING_REFERRAL_BONUS_MB_REFERRER).await;
    let referral_bonus_traffic_mb_referee =
        bonus_setting(crate::services::bonus_traffic::SETTING_REFERRAL_BONUS_MB_REFEREE).await;

    let admin_notification_tg_ids = state
        .settings
        .get_or_default("admin_notification_tg_ids", "")
        .await;
    let terms_of_service = state
        .settings
        .get_or_default("terms_of_service", "Welcome to CARAMBA.")
        .await;
    let bot_buttons_mode = state
        .settings
        .get_or_default("bot_buttons_mode", "full")
        .await;
    let default_language = state
        .settings
        .get_or_default(
            crate::bot::translations::DEFAULT_LANGUAGE_SETTING,
            crate::bot::translations::Lang::DEFAULT.as_str(),
        )
        .await;
    let bot_support_button_always_on = state
        .settings
        .get_bool_or_default("bot_support_button_always_on", true)
        .await;

    let decoy_enabled = state
        .settings
        .get_or_default("decoy_enabled", "false")
        .await
        == "true";
    let decoy_urls = state.settings.get_or_default("decoy_urls", "[]").await;
    let decoy_min_interval = state
        .settings
        .get_or_default("decoy_min_interval", "60")
        .await;
    let decoy_max_interval = state
        .settings
        .get_or_default("decoy_max_interval", "600")
        .await;

    let kill_switch_enabled = state
        .settings
        .get_or_default("kill_switch_enabled", "false")
        .await
        == "true";
    let kill_switch_timeout = state
        .settings
        .get_or_default("kill_switch_timeout", "300")
        .await;

    let admin_path = normalize_url_path(&state.admin_path);

    let last_export = state.settings.get_or_default("last_export", "Never").await;

    let deployment_mode = "single-host".to_string();
    let miniapp_enabled = state
        .settings
        .get_or_default("miniapp_enabled", "false")
        .await
        == "true";
    let _ = miniapp_enabled; // unused now but keeping for fetch consistency
    let subscription_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;

    let auto_update_agents = state
        .settings
        .get_or_default("auto_update_agents", "true")
        .await
        == "true";
    let mut agent_latest_version = state
        .settings
        .get_or_default("agent_latest_version", "0.0.0")
        .await;
    let mut agent_update_url = state.settings.get_or_default("agent_update_url", "").await;
    let agent_update_hash = state.settings.get_or_default("agent_update_hash", "").await;
    let mut sub_worker_target_version = state
        .settings
        .get_or_default("worker_sub_target_version", "")
        .await;
    let mut sub_worker_update_url = state
        .settings
        .get_or_default("worker_sub_update_url", "")
        .await;
    let mut bot_worker_target_version = state
        .settings
        .get_or_default("worker_bot_target_version", "")
        .await;
    let mut bot_worker_update_url = state
        .settings
        .get_or_default("worker_bot_update_url", "")
        .await;

    let mut auto_fill_updates = HashMap::new();
    let current_version = crate::utils::current_panel_version();
    let default_version =
        if agent_latest_version.trim().is_empty() || agent_latest_version.trim() == "0.0.0" {
            current_version.clone()
        } else {
            agent_latest_version.clone()
        };
    if default_version != agent_latest_version {
        agent_latest_version = default_version.clone();
        auto_fill_updates.insert(
            "agent_latest_version".to_string(),
            agent_latest_version.clone(),
        );
    }
    let normalized_agent_version = if agent_latest_version.starts_with('v') {
        agent_latest_version.clone()
    } else {
        format!("v{}", agent_latest_version)
    };
    if agent_update_url.trim().is_empty() {
        agent_update_url = release_asset_url(&normalized_agent_version, "caramba-node");
        auto_fill_updates.insert("agent_update_url".to_string(), agent_update_url.clone());
    }
    if sub_worker_target_version.trim().is_empty() {
        sub_worker_target_version = normalized_agent_version.clone();
        auto_fill_updates.insert(
            "worker_sub_target_version".to_string(),
            sub_worker_target_version.clone(),
        );
    }
    if bot_worker_target_version.trim().is_empty() {
        bot_worker_target_version = normalized_agent_version.clone();
        auto_fill_updates.insert(
            "worker_bot_target_version".to_string(),
            bot_worker_target_version.clone(),
        );
    }
    if sub_worker_update_url.trim().is_empty() && !sub_worker_target_version.trim().is_empty() {
        sub_worker_update_url = release_asset_url(&sub_worker_target_version, "caramba-sub");
        auto_fill_updates.insert(
            "worker_sub_update_url".to_string(),
            sub_worker_update_url.clone(),
        );
    }
    if bot_worker_update_url.trim().is_empty() && !bot_worker_target_version.trim().is_empty() {
        bot_worker_update_url = release_asset_url(&bot_worker_target_version, "caramba-bot");
        auto_fill_updates.insert(
            "worker_bot_update_url".to_string(),
            bot_worker_update_url.clone(),
        );
    }
    if !auto_fill_updates.is_empty()
        && let Err(e) = state.settings.set_multiple(auto_fill_updates).await
    {
        error!("Failed to persist auto-filled rollout settings: {}", e);
    }
    if let Err(e) = crate::handlers::api::internal::ensure_worker_update_tables(&state.pool).await {
        error!("Failed to ensure worker update tables: {}", e);
    }
    let worker_inventory = fetch_worker_inventory(&state.pool).await;
    let worker_total_count = worker_inventory.len();
    let worker_online_count = worker_inventory.iter().filter(|w| w.is_online).count();
    let worker_update_reports = fetch_worker_update_reports(&state.pool).await;
    let relay_auth_mode = state
        .settings
        .get_or_default("relay_auth_mode", "dual")
        .await;
    let relay_legacy_usage_last_seen_at_raw = state
        .settings
        .get_or_default("relay_legacy_usage_last_seen_at", "")
        .await;
    let relay_legacy_usage_last_seen_bytes = state
        .settings
        .get_or_default("relay_legacy_usage_last_seen_bytes", "0")
        .await;
    let relay_legacy_usage_last_seen_at = if relay_legacy_usage_last_seen_at_raw.trim().is_empty() {
        "never".to_string()
    } else {
        relay_legacy_usage_last_seen_at_raw
    };
    let installer_enrollment_key = match ensure_installer_enrollment_key(&state.pool).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to ensure installer enrollment key: {}", e);
            "<ENROLLMENT_KEY>".to_string()
        }
    };
    let installer_sub_token_ready = internal_api_token_present;
    let installer_sub_token = if internal_api_token_present {
        internal_api_token.clone()
    } else {
        "<INTERNAL_API_TOKEN>".to_string()
    };
    let installer_node_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role node --panel {} --token {} --node-type exit",
        panel_url_display, panel_url_display, installer_enrollment_key
    );
    let installer_sub_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role sub --panel {} --token {} --region global",
        panel_url_display, panel_url_display, installer_sub_token
    );
    let installer_bot_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role bot --panel {} --panel-token {}",
        panel_url_display, panel_url_display, installer_sub_token
    );
    let local_panel_detected =
        Path::new("/opt/caramba/caramba-panel").exists() || service_exists("caramba-panel.service");
    let local_sub_detected =
        Path::new("/opt/caramba/caramba-sub").exists() || service_exists("caramba-sub.service");
    let local_bot_detected =
        Path::new("/opt/caramba/caramba-bot").exists() || service_exists("caramba-bot.service");
    let local_node_detected =
        Path::new("/opt/caramba/caramba-node").exists() || service_exists("caramba-node.service");
    let local_panel_active = is_service_active("caramba-panel.service");
    let local_sub_active = is_service_active("caramba-sub.service");
    let local_bot_active = is_service_active("caramba-bot.service");
    let local_node_active = is_service_active("caramba-node.service");
    let local_panel_enabled = is_service_enabled("caramba-panel.service");
    let local_sub_enabled = is_service_enabled("caramba-sub.service");
    let local_bot_enabled = is_service_enabled("caramba-bot.service");
    let local_node_enabled = is_service_enabled("caramba-node.service");

    let masked_payment_api_key = if !payment_api_key.is_empty() {
        mask_key(&payment_api_key)
    } else {
        "".to_string()
    };
    let masked_nowpayments_api_key = if !nowpayments_api_key.is_empty() {
        mask_key(&nowpayments_api_key)
    } else {
        "".to_string()
    };
    let masked_bot_token = if !bot_token.is_empty() {
        mask_key(&bot_token)
    } else {
        "".to_string()
    };

    let cryptomus_merchant_id = state
        .settings
        .get_or_default("cryptomus_merchant_id", "")
        .await;
    let cryptomus_payment_api_key = state
        .settings
        .get_or_default("cryptomus_payment_api_key", "")
        .await;
    let aaio_merchant_id = state.settings.get_or_default("aaio_merchant_id", "").await;
    let aaio_secret_1 = state.settings.get_or_default("aaio_secret_1", "").await;
    let aaio_secret_2 = state.settings.get_or_default("aaio_secret_2", "").await;

    let masked_lava_project_id = if !lava_project_id.is_empty() {
        mask_key(&lava_project_id)
    } else {
        "".to_string()
    };
    let masked_lava_secret_key = if !lava_secret_key.is_empty() {
        mask_key(&lava_secret_key)
    } else {
        "".to_string()
    };
    let masked_stripe_secret_key = if !stripe_secret_key.is_empty() {
        mask_key(&stripe_secret_key)
    } else {
        "".to_string()
    };
    let masked_stripe_webhook_secret = if !stripe_webhook_secret.is_empty() {
        mask_key(&stripe_webhook_secret)
    } else {
        "".to_string()
    };

    let masked_cryptomus_merchant_id = if !cryptomus_merchant_id.is_empty() {
        mask_key(&cryptomus_merchant_id)
    } else {
        "".to_string()
    };
    let masked_cryptomus_payment_api_key = if !cryptomus_payment_api_key.is_empty() {
        mask_key(&cryptomus_payment_api_key)
    } else {
        "".to_string()
    };
    let masked_aaio_merchant_id = if !aaio_merchant_id.is_empty() {
        mask_key(&aaio_merchant_id)
    } else {
        "".to_string()
    };
    let masked_aaio_secret_1 = if !aaio_secret_1.is_empty() {
        mask_key(&aaio_secret_1)
    } else {
        "".to_string()
    };
    let masked_aaio_secret_2 = if !aaio_secret_2.is_empty() {
        mask_key(&aaio_secret_2)
    } else {
        "".to_string()
    };

    // ── Новые платёжные провайдеры ──────────────────────────────────────────
    let wata_enabled = state.settings.get_or_default("wata_enabled", "false").await == "true";
    let wata_jwt_token_raw = state.settings.get_or_default("wata_jwt_token", "").await;
    let wata_webhook_secret_raw = state
        .settings
        .get_or_default("wata_webhook_secret", "")
        .await;
    let masked_wata_jwt_token = if wata_jwt_token_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&wata_jwt_token_raw)
    };
    let masked_wata_webhook_secret = if wata_webhook_secret_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&wata_webhook_secret_raw)
    };

    // ── Paypalych (pal24.pro) ──────────────────────────────────────────────────
    let paypalych_enabled = state
        .settings
        .get_or_default("paypalych_enabled", "false")
        .await
        == "true";
    let paypalych_api_token_raw = state
        .settings
        .get_or_default("paypalych_api_token", "")
        .await;
    let paypalych_shop_id_raw = state.settings.get_or_default("paypalych_shop_id", "").await;
    let paypalych_webhook_secret_raw = state
        .settings
        .get_or_default("paypalych_webhook_secret", "")
        .await;
    let masked_paypalych_api_token = if paypalych_api_token_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&paypalych_api_token_raw)
    };
    // shop_id is not a secret, so we don't mask it — show it verbatim.
    let masked_paypalych_shop_id = paypalych_shop_id_raw;
    let masked_paypalych_webhook_secret = if paypalych_webhook_secret_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&paypalych_webhook_secret_raw)
    };

    let crystalpay_enabled = state
        .settings
        .get_or_default("crystalpay_enabled", "false")
        .await
        == "true";
    let crystalpay_login_raw = state.settings.get_or_default("crystalpay_login", "").await;
    let crystalpay_secret_raw = state.settings.get_or_default("crystalpay_secret", "").await;
    let crystalpay_salt_raw = state.settings.get_or_default("crystalpay_salt", "").await;
    let masked_crystalpay_login = if crystalpay_login_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&crystalpay_login_raw)
    };
    let masked_crystalpay_secret = if crystalpay_secret_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&crystalpay_secret_raw)
    };
    let masked_crystalpay_salt = if crystalpay_salt_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&crystalpay_salt_raw)
    };

    let tribute_enabled = state
        .settings
        .get_or_default("tribute_enabled", "false")
        .await
        == "true";
    let tribute_api_key_raw = state.settings.get_or_default("tribute_api_key", "").await;
    let tribute_webhook_secret_raw = state
        .settings
        .get_or_default("tribute_webhook_secret", "")
        .await;
    let masked_tribute_api_key = if tribute_api_key_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&tribute_api_key_raw)
    };
    let masked_tribute_webhook_secret = if tribute_webhook_secret_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&tribute_webhook_secret_raw)
    };

    let btcpay_enabled = state
        .settings
        .get_or_default("btcpay_enabled", "false")
        .await
        == "true";
    let btcpay_url_raw = state.settings.get_or_default("btcpay_url", "").await;
    let btcpay_api_key_raw = state.settings.get_or_default("btcpay_api_key", "").await;
    let btcpay_store_id_raw = state.settings.get_or_default("btcpay_store_id", "").await;
    let btcpay_webhook_secret_raw = state
        .settings
        .get_or_default("btcpay_webhook_secret", "")
        .await;
    let masked_btcpay_api_key = if btcpay_api_key_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&btcpay_api_key_raw)
    };
    let masked_btcpay_store_id = if btcpay_store_id_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&btcpay_store_id_raw)
    };
    let masked_btcpay_webhook_secret = if btcpay_webhook_secret_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&btcpay_webhook_secret_raw)
    };

    let oxapay_enabled = state
        .settings
        .get_or_default("oxapay_enabled", "false")
        .await
        == "true";
    let oxapay_merchant_key_raw = state
        .settings
        .get_or_default("oxapay_merchant_key", "")
        .await;
    let masked_oxapay_merchant_key = if oxapay_merchant_key_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&oxapay_merchant_key_raw)
    };

    let coinbase_enabled = state
        .settings
        .get_or_default("coinbase_enabled", "false")
        .await
        == "true";
    let coinbase_api_key_raw = state.settings.get_or_default("coinbase_api_key", "").await;
    let coinbase_webhook_secret_raw = state
        .settings
        .get_or_default("coinbase_webhook_secret", "")
        .await;
    let masked_coinbase_api_key = if coinbase_api_key_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&coinbase_api_key_raw)
    };
    let masked_coinbase_webhook_secret = if coinbase_webhook_secret_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&coinbase_webhook_secret_raw)
    };

    let plisio_enabled = state
        .settings
        .get_or_default("plisio_enabled", "false")
        .await
        == "true";
    let plisio_api_key_raw = state.settings.get_or_default("plisio_api_key", "").await;
    let masked_plisio_api_key = if plisio_api_key_raw.is_empty() {
        "".to_string()
    } else {
        mask_key(&plisio_api_key_raw)
    };
    // ────────────────────────────────────────────────────────────────────────

    let template = SettingsTemplate {
        current_version: current_version.clone(),
        active_nodes_count: state
            .orchestration_service
            .node_repo
            .get_all_nodes()
            .await
            .unwrap_or_default()
            .len(),
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        masked_bot_token,
        telegram_stars_enabled,
        manual_enabled,
        nowpayments_api_key,
        masked_nowpayments_api_key,
        masked_nowpayments_ipn_secret: masked_nowpayments_ipn,
        masked_payment_api_key,
        masked_cryptomus_merchant_id,
        masked_cryptomus_payment_api_key,
        masked_aaio_merchant_id,
        masked_aaio_secret_1,
        masked_aaio_secret_2,
        masked_lava_project_id,
        masked_lava_secret_key,
        masked_stripe_secret_key,
        masked_stripe_webhook_secret,
        payment_ipn_url,
        currency_rate,
        stars_per_usd,
        support_url,
        panel_url,
        panel_url_display,
        admin_ui_url_display,
        internal_api_token,
        internal_api_token_present,
        internal_api_token_source,
        bot_username,
        brand_name,
        referral_bonus_percent,
        signup_bonus_traffic_mb,
        referral_bonus_traffic_mb_referrer,
        referral_bonus_traffic_mb_referee,
        referral_referrer_signup_bonus_cents,
        referral_referred_signup_bonus_cents,
        admin_notification_tg_ids,
        terms_of_service,
        bot_buttons_mode,
        default_language,
        bot_support_button_always_on,
        decoy_enabled,
        decoy_urls,
        decoy_min_interval,
        decoy_max_interval,
        kill_switch_enabled,
        kill_switch_timeout,
        last_export,
        is_auth: true,
        admin_path,
        active_page: "settings".to_string(),
        deployment_mode,
        local_panel_detected,
        local_sub_detected,
        local_bot_detected,
        local_node_detected,
        local_panel_active,
        local_sub_active,
        local_bot_active,
        local_node_active,
        local_panel_enabled,
        local_sub_enabled,
        local_bot_enabled,
        local_node_enabled,
        auto_update_agents,
        agent_latest_version,
        agent_update_url,
        agent_update_hash,
        sub_worker_target_version,
        sub_worker_update_url,
        bot_worker_target_version,
        bot_worker_update_url,
        worker_inventory,
        worker_total_count,
        worker_online_count,
        worker_update_reports,
        relay_legacy_usage_last_seen_at,
        relay_legacy_usage_last_seen_bytes,
        installer_enrollment_key,
        installer_sub_token,
        installer_node_command,
        installer_sub_command,
        installer_bot_command,
        installer_sub_token_ready,
        subscription_domain,
        relay_auth_mode,
        // WATA
        wata_enabled,
        masked_wata_jwt_token,
        masked_wata_webhook_secret,
        // Paypalych
        paypalych_enabled,
        masked_paypalych_api_token,
        masked_paypalych_shop_id,
        masked_paypalych_webhook_secret,
        // CrystalPay
        crystalpay_enabled,
        masked_crystalpay_login,
        masked_crystalpay_secret,
        masked_crystalpay_salt,
        // Tribute
        tribute_enabled,
        masked_tribute_api_key,
        masked_tribute_webhook_secret,
        // BTCPay Server
        btcpay_enabled,
        btcpay_url: btcpay_url_raw,
        masked_btcpay_api_key,
        masked_btcpay_store_id,
        masked_btcpay_webhook_secret,
        // OxaPay
        oxapay_enabled,
        masked_oxapay_merchant_key,
        // Coinbase Commerce
        coinbase_enabled,
        masked_coinbase_api_key,
        masked_coinbase_webhook_secret,
        // Plisio
        plisio_enabled,
        masked_plisio_api_key,
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
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
    let masked_bot_token = if !current_bot_token.is_empty() {
        mask_key(&current_bot_token)
    } else {
        "".to_string()
    };

    if let Some(v) = form.bot_token {
        let v = v.trim().to_string();
        if !v.is_empty() && v != masked_bot_token {
            if is_running {
                return (
                    StatusCode::BAD_REQUEST,
                    "Cannot update Bot Token while bot is running. Please stop the bot first.",
                )
                    .into_response();
            }
            settings.insert("bot_token".to_string(), v);
        }
    }

    let current_payment_key = state.settings.get_or_default("payment_api_key", "").await;
    let masked_payment_key = if !current_payment_key.is_empty() {
        mask_key(&current_payment_key)
    } else {
        "".to_string()
    };

    if let Some(v) = form.payment_api_key
        && !v.is_empty()
        && v != masked_payment_key
    {
        settings.insert("payment_api_key".to_string(), v);
    }

    let current_nowpayments_key = state
        .settings
        .get_or_default("nowpayments_api_key", "")
        .await;
    let masked_nowpayments_key = if !current_nowpayments_key.is_empty() {
        mask_key(&current_nowpayments_key)
    } else {
        "".to_string()
    };
    if let Some(v) = form.nowpayments_api_key
        && !v.is_empty()
        && v != masked_nowpayments_key
    {
        settings.insert("nowpayments_api_key".to_string(), v);
    }

    let current_nowpayments_ipn = state
        .settings
        .get_or_default("nowpayments_ipn_secret", "")
        .await;
    let masked_nowpayments_ipn = if !current_nowpayments_ipn.is_empty() {
        mask_key(&current_nowpayments_ipn)
    } else {
        "".to_string()
    };
    if let Some(v) = form.nowpayments_ipn_secret
        && !v.is_empty()
        && v != masked_nowpayments_ipn
    {
        settings.insert("nowpayments_ipn_secret".to_string(), v);
    }

    let current_lava_project = state.settings.get_or_default("lava_project_id", "").await;
    let masked_lava_project = if !current_lava_project.is_empty() {
        mask_key(&current_lava_project)
    } else {
        "".to_string()
    };
    if let Some(v) = form.lava_project_id
        && !v.is_empty()
        && v != masked_lava_project
    {
        settings.insert("lava_project_id".to_string(), v);
    }

    let current_lava_secret = state.settings.get_or_default("lava_secret_key", "").await;
    let masked_lava_secret = if !current_lava_secret.is_empty() {
        mask_key(&current_lava_secret)
    } else {
        "".to_string()
    };
    if let Some(v) = form.lava_secret_key
        && !v.is_empty()
        && v != masked_lava_secret
    {
        settings.insert("lava_secret_key".to_string(), v);
    }

    let current_stripe_secret = state.settings.get_or_default("stripe_secret_key", "").await;
    let masked_stripe_secret = if !current_stripe_secret.is_empty() {
        mask_key(&current_stripe_secret)
    } else {
        "".to_string()
    };
    if let Some(v) = form.stripe_secret_key
        && !v.is_empty()
        && v != masked_stripe_secret
    {
        settings.insert("stripe_secret_key".to_string(), v);
    }

    let current_stripe_webhook = state
        .settings
        .get_or_default("stripe_webhook_secret", "")
        .await;
    let masked_stripe_webhook = if !current_stripe_webhook.is_empty() {
        mask_key(&current_stripe_webhook)
    } else {
        "".to_string()
    };
    if let Some(v) = form.stripe_webhook_secret
        && !v.is_empty()
        && v != masked_stripe_webhook
    {
        settings.insert("stripe_webhook_secret".to_string(), v);
    }

    settings.insert(
        "telegram_stars_enabled".to_string(),
        if is_checkbox_enabled(form.telegram_stars_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    settings.insert(
        "manual_enabled".to_string(),
        if is_checkbox_enabled(form.manual_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    let current_cryptomus_id = state
        .settings
        .get_or_default("cryptomus_merchant_id", "")
        .await;
    let masked_cryptomus_id = if !current_cryptomus_id.is_empty() {
        mask_key(&current_cryptomus_id)
    } else {
        "".to_string()
    };
    if let Some(v) = form.cryptomus_merchant_id
        && !v.is_empty()
        && v != masked_cryptomus_id
    {
        settings.insert("cryptomus_merchant_id".to_string(), v);
    }

    let current_cryptomus_key = state
        .settings
        .get_or_default("cryptomus_payment_api_key", "")
        .await;
    let masked_cryptomus_key = if !current_cryptomus_key.is_empty() {
        mask_key(&current_cryptomus_key)
    } else {
        "".to_string()
    };
    if let Some(v) = form.cryptomus_payment_api_key
        && !v.is_empty()
        && v != masked_cryptomus_key
    {
        settings.insert("cryptomus_payment_api_key".to_string(), v);
    }

    let current_aaio_id = state.settings.get_or_default("aaio_merchant_id", "").await;
    let masked_aaio_id = if !current_aaio_id.is_empty() {
        mask_key(&current_aaio_id)
    } else {
        "".to_string()
    };
    if let Some(v) = form.aaio_merchant_id
        && !v.is_empty()
        && v != masked_aaio_id
    {
        settings.insert("aaio_merchant_id".to_string(), v);
    }

    let current_aaio_s1 = state.settings.get_or_default("aaio_secret_1", "").await;
    let masked_aaio_s1 = if !current_aaio_s1.is_empty() {
        mask_key(&current_aaio_s1)
    } else {
        "".to_string()
    };
    if let Some(v) = form.aaio_secret_1
        && !v.is_empty()
        && v != masked_aaio_s1
    {
        settings.insert("aaio_secret_1".to_string(), v);
    }

    let current_aaio_s2 = state.settings.get_or_default("aaio_secret_2", "").await;
    let masked_aaio_s2 = if !current_aaio_s2.is_empty() {
        mask_key(&current_aaio_s2)
    } else {
        "".to_string()
    };
    if let Some(v) = form.aaio_secret_2
        && !v.is_empty()
        && v != masked_aaio_s2
    {
        settings.insert("aaio_secret_2".to_string(), v);
    }

    if let Some(v) = form.payment_ipn_url {
        settings.insert("payment_ipn_url".to_string(), v);
    }
    if let Some(v) = form.currency_rate {
        settings.insert("currency_rate".to_string(), v);
    }
    // Telegram Stars rate. Bounds are enforced here (and again on read) so a
    // typo can never price a plan at 0 XTR or at an absurd multiple: anything
    // outside [MIN_STARS_PER_USD, MAX_STARS_PER_USD] is normalized back to the
    // built-in fallback instead of being stored.
    if let Some(v) = form.stars_per_usd {
        let normalized = crate::services::payment::stars::parse_stars_per_usd(Some(&v));
        settings.insert(
            crate::services::payment::stars::STARS_PER_USD_SETTING.to_string(),
            normalized.to_string(),
        );
    }
    if let Some(v) = form.support_url {
        settings.insert("support_url".to_string(), v);
    }
    if let Some(v) = form.panel_url {
        let normalized = normalize_base_url(&v);
        settings.insert("panel_url".to_string(), normalized);
    }
    if let Some(v) = form.bot_username {
        settings.insert(
            "bot_username".to_string(),
            v.trim().trim_start_matches('@').to_string(),
        );
    }
    if let Some(v) = form.brand_name {
        settings.insert("brand_name".to_string(), v);
    }
    if let Some(v) = form.referral_bonus_percent {
        settings.insert("referral_bonus_percent".to_string(), v);
    }
    if let Some(v) = form.referral_referrer_signup_bonus_cents {
        settings.insert("referral_referrer_signup_bonus_cents".to_string(), v);
    }
    if let Some(v) = form.referral_referred_signup_bonus_cents {
        settings.insert("referral_referred_signup_bonus_cents".to_string(), v);
    }
    // Бонусный трафик: нормализуем на записи тем же парсером, что и на чтении,
    // чтобы в БД никогда не оказалось значения, которое грант-путь молча
    // прочитает как 0 (админ бы решил, что фича включена).
    for (key, raw) in [
        (
            crate::services::bonus_traffic::SETTING_SIGNUP_BONUS_MB,
            form.signup_bonus_traffic_mb,
        ),
        (
            crate::services::bonus_traffic::SETTING_REFERRAL_BONUS_MB_REFERRER,
            form.referral_bonus_traffic_mb_referrer,
        ),
        (
            crate::services::bonus_traffic::SETTING_REFERRAL_BONUS_MB_REFEREE,
            form.referral_bonus_traffic_mb_referee,
        ),
    ] {
        if let Some(v) = raw {
            let normalized = crate::services::bonus_traffic::parse_bonus_mb(Some(&v));
            settings.insert(key.to_string(), normalized.to_string());
        }
    }
    if let Some(v) = form.admin_notification_tg_ids {
        settings.insert("admin_notification_tg_ids".to_string(), v);
    }
    if let Some(v) = form.terms_of_service {
        settings.insert("terms_of_service".to_string(), v);
    }
    if let Some(v) = form.bot_buttons_mode {
        settings.insert("bot_buttons_mode".to_string(), v);
    }
    // Пишем только распознанный код — резолвер должен уметь его прочитать,
    // мусор в этой настройке молча вернул бы всех к русскому.
    if let Some(lang) = form
        .default_language
        .as_deref()
        .and_then(crate::bot::translations::Lang::parse)
    {
        settings.insert(
            crate::bot::translations::DEFAULT_LANGUAGE_SETTING.to_string(),
            lang.as_str().to_string(),
        );
    }
    settings.insert(
        "bot_support_button_always_on".to_string(),
        if is_checkbox_enabled(form.bot_support_button_always_on.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    settings.insert(
        "decoy_enabled".to_string(),
        if is_checkbox_enabled(form.decoy_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    if let Some(v) = form.decoy_urls {
        settings.insert("decoy_urls".to_string(), v);
    }
    if let Some(v) = form.decoy_min_interval {
        settings.insert("decoy_min_interval".to_string(), v);
    }
    if let Some(v) = form.decoy_max_interval {
        settings.insert("decoy_max_interval".to_string(), v);
    }

    settings.insert(
        "kill_switch_enabled".to_string(),
        if is_checkbox_enabled(form.kill_switch_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    if let Some(v) = form.kill_switch_timeout {
        settings.insert("kill_switch_timeout".to_string(), v);
    }

    settings.insert("deployment_mode".to_string(), "single-host".to_string());

    settings.insert(
        "auto_update_agents".to_string(),
        if is_checkbox_enabled(form.auto_update_agents.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    settings.insert(
        "expiry_reminders_enabled".to_string(),
        if is_checkbox_enabled(form.expiry_reminders_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    if let Some(v) = form.expiry_hours_threshold {
        let val: i64 = v.trim().parse().unwrap_or(72);
        settings.insert("expiry_hours_threshold".to_string(), val.to_string());
    }
    if let Some(v) = form.agent_latest_version {
        let normalized = v.trim().to_string();
        if !normalized.is_empty() {
            settings.insert("agent_latest_version".to_string(), normalized);
        }
    }
    if let Some(v) = form.agent_update_url {
        let normalized = v.trim().to_string();
        if !normalized.is_empty()
            && !normalized.starts_with("https://")
            && !normalized.starts_with("http://")
            && !normalized.starts_with('/')
        {
            return (
                StatusCode::BAD_REQUEST,
                "agent_update_url must start with https://, http://, or /",
            )
                .into_response();
        }
        settings.insert("agent_update_url".to_string(), normalized);
    }
    if let Some(v) = form.agent_update_hash {
        let normalized = v.trim().to_ascii_lowercase();
        if !normalized.is_empty() && !is_valid_sha256_hex(&normalized) {
            return (
                StatusCode::BAD_REQUEST,
                "agent_update_hash must be a 64-character SHA256 hex string",
            )
                .into_response();
        }
        settings.insert("agent_update_hash".to_string(), normalized);
    }

    // ── Новые платёжные провайдеры — сохранение ─────────────────────────────
    // Паттерн: чувствительные поля сохраняются только если не совпадают с маской.
    settings.insert(
        "wata_enabled".to_string(),
        if is_checkbox_enabled(form.wata_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state.settings.get_or_default("wata_jwt_token", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.wata_jwt_token
            && !v.is_empty()
            && v != masked
        {
            settings.insert("wata_jwt_token".to_string(), v);
        }
        let cur = state
            .settings
            .get_or_default("wata_webhook_secret", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.wata_webhook_secret
            && !v.is_empty()
            && v != masked
        {
            settings.insert("wata_webhook_secret".to_string(), v);
        }
    }

    // ── Paypalych — сохранение ────────────────────────────────────────────────
    settings.insert(
        "paypalych_enabled".to_string(),
        if is_checkbox_enabled(form.paypalych_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state
            .settings
            .get_or_default("paypalych_api_token", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.paypalych_api_token
            && !v.is_empty()
            && v != masked
        {
            settings.insert("paypalych_api_token".to_string(), v);
        }
        // shop_id is a non-secret identifier — always save as-is when supplied.
        if let Some(v) = form.paypalych_shop_id {
            let trimmed = v.trim().to_string();
            if !trimmed.is_empty() {
                settings.insert("paypalych_shop_id".to_string(), trimmed);
            }
        }
        let cur = state
            .settings
            .get_or_default("paypalych_webhook_secret", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.paypalych_webhook_secret
            && !v.is_empty()
            && v != masked
        {
            settings.insert("paypalych_webhook_secret".to_string(), v);
        }
    }

    settings.insert(
        "crystalpay_enabled".to_string(),
        if is_checkbox_enabled(form.crystalpay_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state.settings.get_or_default("crystalpay_login", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.crystalpay_login
            && !v.is_empty()
            && v != masked
        {
            settings.insert("crystalpay_login".to_string(), v);
        }
        let cur = state.settings.get_or_default("crystalpay_secret", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.crystalpay_secret
            && !v.is_empty()
            && v != masked
        {
            settings.insert("crystalpay_secret".to_string(), v);
        }
        let cur = state.settings.get_or_default("crystalpay_salt", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.crystalpay_salt
            && !v.is_empty()
            && v != masked
        {
            settings.insert("crystalpay_salt".to_string(), v);
        }
    }

    settings.insert(
        "tribute_enabled".to_string(),
        if is_checkbox_enabled(form.tribute_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state.settings.get_or_default("tribute_api_key", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.tribute_api_key
            && !v.is_empty()
            && v != masked
        {
            settings.insert("tribute_api_key".to_string(), v);
        }
        let cur = state
            .settings
            .get_or_default("tribute_webhook_secret", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.tribute_webhook_secret
            && !v.is_empty()
            && v != masked
        {
            settings.insert("tribute_webhook_secret".to_string(), v);
        }
    }

    settings.insert(
        "btcpay_enabled".to_string(),
        if is_checkbox_enabled(form.btcpay_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        if let Some(v) = form.btcpay_url {
            let normalized = normalize_base_url(&v);
            settings.insert("btcpay_url".to_string(), normalized);
        }
        let cur = state.settings.get_or_default("btcpay_api_key", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.btcpay_api_key
            && !v.is_empty()
            && v != masked
        {
            settings.insert("btcpay_api_key".to_string(), v);
        }
        let cur = state.settings.get_or_default("btcpay_store_id", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.btcpay_store_id
            && !v.is_empty()
            && v != masked
        {
            settings.insert("btcpay_store_id".to_string(), v);
        }
        let cur = state
            .settings
            .get_or_default("btcpay_webhook_secret", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.btcpay_webhook_secret
            && !v.is_empty()
            && v != masked
        {
            settings.insert("btcpay_webhook_secret".to_string(), v);
        }
    }

    settings.insert(
        "oxapay_enabled".to_string(),
        if is_checkbox_enabled(form.oxapay_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state
            .settings
            .get_or_default("oxapay_merchant_key", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.oxapay_merchant_key
            && !v.is_empty()
            && v != masked
        {
            settings.insert("oxapay_merchant_key".to_string(), v);
        }
    }

    settings.insert(
        "coinbase_enabled".to_string(),
        if is_checkbox_enabled(form.coinbase_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state.settings.get_or_default("coinbase_api_key", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.coinbase_api_key
            && !v.is_empty()
            && v != masked
        {
            settings.insert("coinbase_api_key".to_string(), v);
        }
        let cur = state
            .settings
            .get_or_default("coinbase_webhook_secret", "")
            .await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.coinbase_webhook_secret
            && !v.is_empty()
            && v != masked
        {
            settings.insert("coinbase_webhook_secret".to_string(), v);
        }
    }

    settings.insert(
        "plisio_enabled".to_string(),
        if is_checkbox_enabled(form.plisio_enabled.as_deref()) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );
    {
        let cur = state.settings.get_or_default("plisio_api_key", "").await;
        let masked = if cur.is_empty() {
            "".to_string()
        } else {
            mask_key(&cur)
        };
        if let Some(v) = form.plisio_api_key
            && !v.is_empty()
            && v != masked
        {
            settings.insert("plisio_api_key".to_string(), v);
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    match state.settings.set_multiple(settings).await {
        Ok(_) => {
            let active_nodes = state
                .store_service
                .get_active_node_ids()
                .await
                .unwrap_or_default();

            let pubsub = state.pubsub.clone();
            tokio::spawn(async move {
                for node_id in active_nodes {
                    let _ = pubsub
                        .publish(&format!("node_events:{}", node_id), "settings_update")
                        .await;
                }
            });

            ([("HX-Refresh", "true")], "Settings Saved").into_response()
        }
        Err(e) => {
            error!("Failed to save settings: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save settings").into_response()
        }
    }
}

pub async fn toggle_bot(State(state): State<AppState>) -> impl IntoResponse {
    let is_running = state.bot_manager.is_running().await;

    let new_status = if is_running {
        info!("Stopping bot via toggle");
        state.bot_manager.stop_bot().await;
        // Try to stop external worker if running on Linux with systemd
        let _ = run_systemctl_action(&["stop", "caramba-bot"]);
        "stopped".to_string()
    } else {
        info!("Starting bot via toggle");
        let token = state.settings.get_or_default("bot_token", "").await;
        if token.is_empty() {
            return (StatusCode::BAD_REQUEST, "Bot token is empty").into_response();
        }
        if !state.bot_manager.start_bot(token, state.clone()).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to start bot notifier. Check token and logs.",
            )
                .into_response();
        }
        // Try to start external worker
        let _ = run_systemctl_action(&["start", "caramba-bot"]);
        "running".to_string()
    };

    let _ = state.settings.set("bot_status", &new_status).await;

    let admin_path = state.admin_path.clone();

    let template = BotStatusPartial {
        bot_status: new_status,
        admin_path,
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
    }
}

pub async fn bot_logs_page(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return axum::response::Redirect::to(&format!("{}/login", state.admin_path))
            .into_response();
    }

    let is_running = state.bot_manager.is_running().await;
    let bot_status = if is_running {
        "running".to_string()
    } else {
        "stopped".to_string()
    };
    let bot_username = state.settings.get_or_default("bot_username", "").await;
    let subscription_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let tma_domain = state.settings.get_or_default("tma_domain", "").await;
    let miniapp_domain = if tma_domain.trim().is_empty() {
        subscription_domain.clone()
    } else {
        tma_domain
    };

    let admin_path = state.admin_path.clone();

    Html(
        BotLogsTemplate {
            is_auth: true,
            username: get_auth_user(&state, &jar)
                .await
                .unwrap_or("Admin".to_string()),
            admin_path,
            active_page: "bot_logs".to_string(),
            bot_status,
            bot_username,
            miniapp_domain,
        }
        .render()
        .unwrap(),
    )
    .into_response()
}

pub async fn bot_logs_history(State(_state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&_state, &jar).await {
        return "Unauthorized".to_string();
    }

    // Use dedicated bot.log only to avoid mixing panel/node logs in bot UI.
    if let Some(history) = read_log_history("bot.log", 300, true) {
        return history;
    }

    "No bot logs available yet".to_string()
}

static LAST_BOT_LOG_POS: AtomicU64 = AtomicU64::new(0);

pub async fn bot_logs_tail(State(_state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&_state, &jar).await {
        return String::new();
    }

    if Path::new("bot.log").exists() {
        return read_log_tail("bot.log", &LAST_BOT_LOG_POS, true).unwrap_or_default();
    }

    String::new()
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
        || line.contains("caramba_bot::")
        || line.contains("teloxide")
        || line.contains("Bot connected")
        || line.contains("Received message:")
        || line.contains("Received callback:")
        || line.contains("Unknown command. Use /help.")
        || line.contains("Failed to upsert user on /start")
}

pub async fn export_database(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    info!("Database export requested");

    let export_service = crate::services::export_service::ExportService::new();
    let export_result = export_service.create_export().await;

    match export_result {
        Ok(data) => {
            let filename = format!(
                "panel_backup_{}.sql.gz",
                chrono::Utc::now().format("%Y%m%d_%H%M%S")
            );

            let _ = state
                .settings
                .set(
                    "last_export",
                    &chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
                )
                .await;

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/gzip".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                data,
            )
                .into_response()
        }
        Err(e) => {
            error!("Database export failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Export failed. Check server logs for details.",
            )
                .into_response()
        }
    }
}

#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

fn is_stable_semver_tag(tag: &str) -> bool {
    if !tag.starts_with('v') || tag.contains('-') {
        return false;
    }
    let trimmed = &tag[1..];
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Public re-export so other admin handlers (e.g. updates.rs) can re-check
/// the latest release without duplicating the GitHub API call. Same logic
/// as the internal `resolve_latest_release_version` used during settings render.
pub async fn resolve_latest_release_version_pub() -> anyhow::Result<String> {
    resolve_latest_release_version().await
}

async fn resolve_latest_release_version() -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    let releases_resp = client
        .get("https://api.github.com/repos/semanticparadox/caramba/releases")
        .header("User-Agent", "caramba-panel")
        .send()
        .await?;

    if releases_resp.status().is_success() {
        let releases: Vec<GitHubRelease> = releases_resp.json().await.unwrap_or_default();
        for release in releases {
            if is_stable_semver_tag(&release.tag_name) {
                return Ok(release.tag_name);
            }
        }
    }

    let latest_resp = client
        .get("https://api.github.com/repos/semanticparadox/caramba/releases/latest")
        .header("User-Agent", "caramba-panel")
        .send()
        .await?;
    if !latest_resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "latest release endpoint returned {}",
            latest_resp.status()
        ));
    }
    let latest: GitHubRelease = latest_resp.json().await?;
    if !is_stable_semver_tag(&latest.tag_name) {
        return Err(anyhow::anyhow!(
            "latest tag '{}' is not stable semver",
            latest.tag_name
        ));
    }
    Ok(latest.tag_name)
}

fn parse_semver_tuple(version: &str) -> (u32, u32, u32) {
    let clean = version.trim().trim_start_matches('v');
    let mut parts = clean.split('.');
    let major = parts
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    (major, minor, patch)
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn render_system_action_result(message: &str, success: bool) -> String {
    let (bg, border, text) = if success {
        (
            "bg-emerald-500/10",
            "border-emerald-500/30",
            "text-emerald-300",
        )
    } else {
        ("bg-rose-500/10", "border-rose-500/30", "text-rose-300")
    };
    format!(
        r#"<div class="mt-3 rounded-xl border {border} {bg} px-4 py-2 text-sm {text}">{message}</div>"#
    )
}

fn run_systemctl_action(args: &[&str]) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| format!("systemctl {:?} failed to start: {}", args, e))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!(
            "systemctl {:?} failed with status {}",
            args, output.status
        ))
    } else {
        Err(format!("systemctl {:?}: {}", args, stderr))
    }
}

fn render_topology_apply_result(mode: &str, ok_lines: &[String], err_lines: &[String]) -> String {
    let mode_label = match mode {
        "distributed" => "Distributed",
        "modular" => "Modular (Service-by-Service)",
        _ => "Hub",
    };

    let mut html = String::new();
    html.push_str(
        r#"<div class="mt-3 rounded-xl border border-white/10 bg-slate-950/40 px-4 py-3">"#,
    );
    html.push_str(&format!(
        r#"<p class="text-sm text-white font-medium">Applied local topology mode: <span class="text-cyan-300">{}</span></p>"#,
        mode_label
    ));
    if !ok_lines.is_empty() {
        html.push_str(r#"<ul class="mt-2 space-y-1 text-xs text-emerald-300">"#);
        for line in ok_lines {
            html.push_str(&format!(r#"<li>• {}</li>"#, line));
        }
        html.push_str("</ul>");
    }
    if !err_lines.is_empty() {
        html.push_str(r#"<ul class="mt-2 space-y-1 text-xs text-rose-300">"#);
        for line in err_lines {
            html.push_str(&format!(r#"<li>• {}</li>"#, line));
        }
        html.push_str("</ul>");
    }
    if mode == "distributed" {
        html.push_str(
            r#"<p class="mt-2 text-[11px] text-slate-400">Next: install/upgrade Sub and Bot on separate servers using role install commands above.</p>"#,
        );
    } else {
        html.push_str(
            r#"<p class="mt-2 text-[11px] text-slate-400">Hub mode expects Panel/Sub locally. Bot can be local or external.</p>"#,
        );
    }
    html.push_str("</div>");
    html
}

pub async fn apply_deployment_topology(State(_state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Html(render_topology_apply_result(
            "deprecated",
            &["Legacy topology toggles are disabled. Use Reverse Proxy & Relay for single-host routing and relay entrypoint selection.".to_string()],
            &[],
        )),
    )
        .into_response()
}

pub async fn check_update(State(state): State<AppState>) -> impl IntoResponse {
    let current = crate::utils::current_panel_version();
    let latest = resolve_latest_release_version().await;
    let distributed_mode = false;

    let panel_url_setting = state.settings.get_or_default("panel_url", "").await;
    let panel_url_env = std::env::var("PANEL_URL").unwrap_or_default();
    let panel_url = if !panel_url_setting.trim().is_empty() {
        normalize_base_url(&panel_url_setting)
    } else if !panel_url_env.trim().is_empty() {
        normalize_base_url(&panel_url_env)
    } else {
        "https://YOUR_PANEL_DOMAIN".to_string()
    };
    let admin_ui_url = join_base_and_path(&panel_url, &state.admin_path);
    let (internal_api_token, internal_api_token_present, internal_api_token_source) =
        resolve_internal_api_token(&state).await;
    let installer_enrollment_key = match ensure_installer_enrollment_key(&state.pool).await {
        Ok(v) => v,
        Err(e) => {
            error!(
                "Failed to ensure installer enrollment key for update panel: {}",
                e
            );
            "<ENROLLMENT_KEY>".to_string()
        }
    };
    let update_role = if distributed_mode { "panel" } else { "hub" };
    let update_scope = if distributed_mode {
        "Distributed mode: panel host updates control-plane only. Sub/Bot workers are upgraded on their own hosts."
    } else {
        "Hub mode: this host carries panel + sub (+ optional bot). Node agents are upgraded via Agent Rollout."
    };

    let body = match latest {
        Ok(latest_version) => {
            let update_available =
                parse_semver_tuple(&latest_version) > parse_semver_tuple(&current);
            let status_line = if update_available {
                format!(
                    r#"<p class="text-sm text-emerald-400">Update available: <span class="font-mono">{}</span></p>"#,
                    latest_version
                )
            } else {
                format!(
                    r#"<p class="text-sm text-emerald-400">Up to date (<span class="font-mono">{}</span>)</p>"#,
                    current
                )
            };

            let command = format!(
                "curl -fsSL {}/install.sh | sudo bash -s -- --role {} --version {}",
                panel_url, update_role, latest_version
            );
            let local_upgrade_command =
                format!("sudo caramba upgrade --version {}", latest_version);
            let node_command = format!(
                "curl -fsSL {}/install.sh | sudo bash -s -- --role node --panel {} --token {} --node-type exit --version {}",
                panel_url, panel_url, installer_enrollment_key, latest_version
            );
            let sub_command = format!(
                "curl -fsSL {}/install.sh | sudo bash -s -- --role sub --panel {} --token {} --region global --version {}",
                panel_url,
                panel_url,
                if internal_api_token_present {
                    internal_api_token.clone()
                } else {
                    "<INTERNAL_API_TOKEN>".to_string()
                },
                latest_version
            );
            let bot_command = format!(
                "curl -fsSL {}/install.sh | sudo bash -s -- --role bot --panel {} --panel-token {} --version {}",
                panel_url,
                panel_url,
                if internal_api_token_present {
                    internal_api_token.clone()
                } else {
                    "<INTERNAL_API_TOKEN>".to_string()
                },
                latest_version
            );
            let token_source_badge = match internal_api_token_source.as_str() {
                "env" => "INTERNAL_API_TOKEN source: environment",
                "settings" => "INTERNAL_API_TOKEN source: panel settings",
                "generated" => "INTERNAL_API_TOKEN source: auto-generated (panel settings)",
                _ => "INTERNAL_API_TOKEN source: missing",
            };
            format!(
                r##"
<div class="flex items-center justify-between" id="update-status-container">
    <div class="w-full">
        <p class="text-sm text-slate-400">Current Version: <span class="text-white font-mono">{current}</span></p>
        {status_line}
        <p class="text-xs text-slate-500 mt-1">{update_scope}</p>
        <p class="text-xs text-cyan-300 mt-1">Admin UI URL (hidden path): <span class="font-mono">{admin_ui_url}</span></p>
        <p class="text-xs text-slate-500 mt-1">{token_source_badge}</p>
        <div class="mt-3 grid grid-cols-1 gap-2 text-[11px]">
            <div class="rounded-lg border border-white/10 bg-slate-900/40 p-2">
                <p class="text-slate-400 mb-1">Recommended on already installed host:</p>
                <p class="font-mono text-slate-200 break-all">{local_upgrade_command}</p>
            </div>
            <div class="rounded-lg border border-white/10 bg-slate-900/40 p-2">
                <p class="text-slate-400 mb-1">{control_plane_title} one-shot install/upgrade:</p>
                <p class="font-mono text-slate-200 break-all">{command}</p>
            </div>
            <div class="rounded-lg border border-white/10 bg-slate-900/40 p-2">
                <p class="text-slate-400 mb-1">Node worker install/upgrade:</p>
                <p class="font-mono text-slate-200 break-all">{node_command}</p>
            </div>
            <div class="rounded-lg border border-white/10 bg-slate-900/40 p-2">
                <p class="text-slate-400 mb-1">Sub worker install/upgrade:</p>
                <p class="font-mono text-slate-200 break-all">{sub_command}</p>
            </div>
            <div class="rounded-lg border border-white/10 bg-slate-900/40 p-2">
                <p class="text-slate-400 mb-1">Bot worker install/upgrade:</p>
                <p class="font-mono text-slate-200 break-all">{bot_command}</p>
            </div>
        </div>
    </div>
    <button hx-post="{admin_path}/settings/update/check" hx-target="#update-status-container" hx-swap="outerHTML" style="height:fit-content"
        class="flex items-center gap-2 bg-slate-800 hover:bg-slate-700 text-white font-medium py-2 px-4 rounded-lg transition-all border border-white/5">
        <i data-lucide="refresh-cw" class="w-4 h-4"></i> Check Again
    </button>
</div>
                "##,
                current = current,
                status_line = status_line,
                command = command,
                local_upgrade_command = local_upgrade_command,
                control_plane_title = if distributed_mode {
                    "Control-plane"
                } else {
                    "Hub"
                },
                node_command = node_command,
                sub_command = sub_command,
                bot_command = bot_command,
                update_scope = update_scope,
                admin_ui_url = admin_ui_url,
                token_source_badge = token_source_badge,
                admin_path = state.admin_path.clone()
            )
        }
        Err(e) => {
            error!("Failed to check updates: {}", e);
            format!(
                r##"
<div class="flex items-center justify-between" id="update-status-container">
    <div>
        <p class="text-sm text-slate-400">Current Version: <span class="text-white font-mono">{current}</span></p>
        <p class="text-sm text-amber-400">Unable to check GitHub release right now.</p>
        <p class="text-xs text-slate-500 mt-1">Fallback command on this host: <span class="font-mono text-slate-300">sudo caramba upgrade</span></p>
    </div>
    <button hx-post="{admin_path}/settings/update/check" hx-target="#update-status-container" hx-swap="outerHTML"
        class="flex items-center gap-2 bg-slate-800 hover:bg-slate-700 text-white font-medium py-2 px-4 rounded-lg transition-all border border-white/5">
        <i data-lucide="refresh-cw" class="w-4 h-4"></i> Retry
    </button>
</div>
                "##,
                current = current,
                admin_path = state.admin_path.clone()
            )
        }
    };

    (
        axum::http::StatusCode::OK,
        [("HX-Trigger", "update-checked")],
        Html(body),
    )
        .into_response()
}

pub async fn prepare_agent_update(State(state): State<AppState>) -> impl IntoResponse {
    let latest_version = match resolve_latest_release_version().await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to resolve latest release for agent prepare: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                render_system_action_result(
                    "Failed to resolve latest release version from GitHub.",
                    false,
                ),
            )
                .into_response();
        }
    };

    let asset_url = format!(
        "https://github.com/semanticparadox/caramba/releases/download/{}/caramba-node",
        latest_version
    );

    let client = reqwest::Client::new();
    let response = match client
        .get(&asset_url)
        .header("User-Agent", "caramba-panel")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to download agent asset '{}': {}", asset_url, e);
            return (
                StatusCode::BAD_GATEWAY,
                render_system_action_result("Failed to download agent release asset.", false),
            )
                .into_response();
        }
    };

    if !response.status().is_success() {
        error!(
            "Agent asset download failed with status {} for '{}'",
            response.status(),
            asset_url
        );
        return (
            StatusCode::BAD_GATEWAY,
            render_system_action_result(
                "GitHub release asset is unavailable for this version.",
                false,
            ),
        )
            .into_response();
    }

    let bytes = match response.bytes().await {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read agent bytes: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                render_system_action_result("Failed to read downloaded agent binary.", false),
            )
                .into_response();
        }
    };

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash = format!("{:x}", hasher.finalize());

    let mut updates = HashMap::new();
    updates.insert("agent_latest_version".to_string(), latest_version.clone());
    updates.insert("agent_update_url".to_string(), asset_url.clone());
    updates.insert("agent_update_hash".to_string(), hash.clone());

    if let Err(e) = state.settings.set_multiple(updates).await {
        error!("Failed to persist prepared agent update metadata: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            render_system_action_result("Failed to save prepared agent metadata.", false),
        )
            .into_response();
    }

    let short_hash = &hash[..12];
    (
        StatusCode::OK,
        [("HX-Refresh", "true")],
        render_system_action_result(
            &format!(
                "Prepared agent update: version {} (sha256 {}…).",
                latest_version, short_hash
            ),
            true,
        ),
    )
        .into_response()
}

pub async fn rollout_agent_update(State(state): State<AppState>) -> impl IntoResponse {
    let target_version = state
        .settings
        .get_or_default("agent_latest_version", "0.0.0")
        .await;
    let update_url = state.settings.get_or_default("agent_update_url", "").await;
    let update_hash = state.settings.get_or_default("agent_update_hash", "").await;

    if target_version.trim().is_empty() || target_version.trim() == "0.0.0" {
        return (
            StatusCode::BAD_REQUEST,
            render_system_action_result(
                "Agent latest version is not configured. Prepare or fill version first.",
                false,
            ),
        )
            .into_response();
    }
    if update_url.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            render_system_action_result("Agent update URL is empty.", false),
        )
            .into_response();
    }
    if update_hash.trim().is_empty() || !is_valid_sha256_hex(update_hash.trim()) {
        return (
            StatusCode::BAD_REQUEST,
            render_system_action_result(
                "Agent update hash is missing or invalid (must be SHA256 hex).",
                false,
            ),
        )
            .into_response();
    }

    let active_nodes = match state.store_service.get_active_node_ids().await {
        Ok(ids) => ids,
        Err(e) => {
            error!("Failed to fetch active nodes for rollout: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                render_system_action_result("Failed to fetch active nodes.", false),
            )
                .into_response();
        }
    };

    if active_nodes.is_empty() {
        return (
            StatusCode::OK,
            render_system_action_result("No active nodes found for rollout.", true),
        )
            .into_response();
    }

    let mut marked = 0usize;
    let mut signaled = 0usize;
    for node_id in &active_nodes {
        match sqlx::query("UPDATE nodes SET target_version = $1 WHERE id = $2")
            .bind(target_version.trim())
            .bind(*node_id)
            .execute(&state.pool)
            .await
        {
            Ok(_) => marked += 1,
            Err(e) => error!(
                "Failed to set target_version '{}' for node {}: {}",
                target_version, node_id, e
            ),
        }

        if state
            .pubsub
            .publish(&format!("node_events:{}", node_id), "update")
            .await
            .is_ok()
        {
            signaled += 1;
        }
    }

    (
        StatusCode::OK,
        render_system_action_result(
            &format!(
                "Rollout queued for version {}: marked {} node(s), signaled {} node(s).",
                target_version, marked, signaled
            ),
            true,
        ),
    )
        .into_response()
}

pub async fn queue_worker_update(
    State(state): State<AppState>,
    Form(form): Form<QueueWorkerUpdateForm>,
) -> impl IntoResponse {
    let role = match normalize_worker_role(&form.role) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                render_system_action_result("Unknown worker role.", false),
            )
                .into_response();
        }
    };

    let version = match form
        .version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => v.to_string(),
        None => match resolve_latest_release_version().await {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to resolve latest release for worker queue: {}", e);
                return (
                    StatusCode::BAD_GATEWAY,
                    render_system_action_result(
                        "Failed to resolve latest release version from GitHub.",
                        false,
                    ),
                )
                    .into_response();
            }
        },
    };

    let asset_url = format!(
        "https://github.com/semanticparadox/caramba/releases/download/{}/{}",
        version,
        worker_asset_name(role)
    );

    let asset_resp = match reqwest::Client::new()
        .get(&asset_url)
        .header("User-Agent", "caramba-panel")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => resp,
        Ok(resp) => {
            return (
                StatusCode::BAD_GATEWAY,
                render_system_action_result(
                    &format!(
                        "Release asset for {} not available (status {}).",
                        role,
                        resp.status()
                    ),
                    false,
                ),
            )
                .into_response();
        }
        Err(e) => {
            error!("Failed to check worker asset '{}': {}", asset_url, e);
            return (
                StatusCode::BAD_GATEWAY,
                render_system_action_result("Failed to check release asset URL.", false),
            )
                .into_response();
        }
    };

    // Download and compute SHA256
    let worker_hash = match asset_resp.bytes().await {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            format!("{:x}", hasher.finalize())
        }
        Err(_) => String::new(),
    };

    let target_version_key = format!("worker_{}_target_version", role);
    let update_url_key = format!("worker_{}_update_url", role);
    let update_hash_key = format!("worker_{}_update_hash", role);

    let mut updates = HashMap::new();
    updates.insert(target_version_key, version.clone());
    updates.insert(update_url_key, asset_url.clone());
    updates.insert(update_hash_key, worker_hash);
    if let Err(e) = state.settings.set_multiple(updates).await {
        error!("Failed to save worker update metadata: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            render_system_action_result("Failed to persist worker update metadata.", false),
        )
            .into_response();
    }

    if let Err(e) = crate::handlers::api::internal::ensure_worker_update_tables(&state.pool).await {
        error!("Failed to ensure worker update tables: {}", e);
    } else if let Err(e) = sqlx::query(
        r#"
        INSERT INTO worker_update_reports (role, worker_id, current_version, target_version, status, message)
        VALUES ($1, '*', NULL, $2, 'queued', $3)
        "#,
    )
    .bind(role)
    .bind(version.as_str())
    .bind("Queued from panel settings. Worker will self-update on next poll.".to_string())
    .execute(&state.pool)
    .await
    {
        error!("Failed to write worker queue report: {}", e);
    }

    (
        StatusCode::OK,
        [("HX-Refresh", "true")],
        render_system_action_result(
            &format!(
                "Queued {} worker update to {}. Workers will apply on next poll.",
                role.to_ascii_uppercase(),
                version
            ),
            true,
        ),
    )
        .into_response()
}
