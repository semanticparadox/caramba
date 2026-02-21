// Users Module
// User management, subscriptions, balance, devices

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, Query, State},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};
use std::env;
use std::net::IpAddr;
use tracing::{error, info};

use super::auth::{get_auth_user, is_authenticated};
use crate::AppState;
use crate::bot_manager::{NotificationMediaType, NotificationParseMode, NotificationPayload};
use crate::services::logging_service::LoggingService;
use caramba_db::models::store::{Plan, User};

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "users.html")]
pub struct UsersTemplate {
    pub users: Vec<User>,
    pub search: String,
    pub campaigns: Vec<NotificationCampaignHistory>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Debug, Clone)]
pub struct NotificationCampaignHistory {
    pub id: i64,
    pub created_at: String,
    pub title: String,
    pub created_by: String,
    pub segment: String,
    pub parse_mode: String,
    pub media_type: String,
    pub buttons_count: usize,
    pub sent: i32,
    pub failed: i32,
    pub planned: i32,
    pub status: String,
    pub message_preview: String,
}

#[derive(Debug, Clone, FromRow)]
struct NotificationCampaignRow {
    id: i64,
    created_at: DateTime<Utc>,
    title: Option<String>,
    created_by_username: Option<String>,
    target_segment: String,
    parse_mode: String,
    media_type: String,
    buttons_json: Option<serde_json::Value>,
    planned_count: i32,
    sent_count: i32,
    failed_count: i32,
    status: String,
    message_text: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "user_details.html")]
pub struct UserDetailsTemplate {
    pub user: User,
    pub subscriptions: Vec<AdminSubscriptionView>,
    pub orders: Vec<UserOrderDisplay>,
    pub referrals: Vec<caramba_db::models::store::DetailedReferral>,
    pub total_referral_earnings: String,
    pub available_plans: Vec<Plan>,
    pub is_auth: bool,
    pub username: String,
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

#[derive(Debug, Clone)]
pub struct AdminSubscriptionView {
    pub id: i64,
    pub plan_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub price: i64,
    pub active_devices: i64,
    pub device_limit: i64,
    pub subscription_url: String,
    pub primary_vless_link: Option<String>,
    pub vless_links_count: usize,
    pub last_node_label: Option<String>,
    pub last_sub_access_label: Option<String>,
}

#[derive(Deserialize)]
pub struct AdminGiftForm {
    pub duration_id: i64,
}

#[derive(Deserialize)]
pub struct UpdateUserForm {
    pub balance: i64,
    pub is_banned: bool,
    pub referral_code: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct RefundForm {
    pub amount: i64,
}

#[derive(Deserialize)]
pub struct ExtendForm {
    pub days: i32,
}

#[derive(Deserialize, Clone)]
pub struct NotifyForm {
    pub campaign_title: Option<String>,
    pub message: String,
    pub segment: Option<String>,
    pub parse_mode: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub image_url: Option<String>,
    pub button_text: Option<String>,
    pub button_url: Option<String>,
    pub button2_text: Option<String>,
    pub button2_url: Option<String>,
    pub button3_text: Option<String>,
    pub button3_url: Option<String>,
    pub disable_link_preview: Option<String>,
}

fn normalize_optional(input: Option<String>) -> Option<String> {
    input.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_notification_mode(input: Option<&str>) -> NotificationParseMode {
    match input
        .unwrap_or("plain")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => NotificationParseMode::Html,
        "markdown" | "markdownv2" | "md" => NotificationParseMode::MarkdownV2,
        _ => NotificationParseMode::Plain,
    }
}

fn parse_notification_mode_label(mode: &str) -> &'static str {
    match mode {
        "html" => "HTML",
        "markdown" | "markdownv2" => "MarkdownV2",
        _ => "Plain",
    }
}

fn notification_parse_mode_db_value(mode: NotificationParseMode) -> &'static str {
    match mode {
        NotificationParseMode::Plain => "plain",
        NotificationParseMode::MarkdownV2 => "markdown",
        NotificationParseMode::Html => "html",
    }
}

fn parse_notification_media_type(input: Option<&str>) -> NotificationMediaType {
    match input.unwrap_or("none").trim().to_ascii_lowercase().as_str() {
        "photo" | "image" => NotificationMediaType::Photo,
        "video" => NotificationMediaType::Video,
        "document" | "file" => NotificationMediaType::Document,
        _ => NotificationMediaType::None,
    }
}

fn notification_media_type_db_value(media_type: NotificationMediaType) -> &'static str {
    match media_type {
        NotificationMediaType::None => "none",
        NotificationMediaType::Photo => "photo",
        NotificationMediaType::Video => "video",
        NotificationMediaType::Document => "document",
    }
}

fn parse_notification_media_label(media_type: &str) -> &'static str {
    match media_type {
        "photo" => "Photo",
        "video" => "Video",
        "document" => "Document",
        _ => "None",
    }
}

fn notification_parse_mode_label_for_payload(mode: NotificationParseMode) -> &'static str {
    match mode {
        NotificationParseMode::Plain => "Plain",
        NotificationParseMode::MarkdownV2 => "MarkdownV2",
        NotificationParseMode::Html => "HTML",
    }
}

fn notification_media_type_label_for_payload(media_type: NotificationMediaType) -> &'static str {
    match media_type {
        NotificationMediaType::None => "None",
        NotificationMediaType::Photo => "Photo",
        NotificationMediaType::Video => "Video",
        NotificationMediaType::Document => "Document",
    }
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn canonicalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

fn parse_ip_maybe(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(ip) = raw.parse::<IpAddr>() {
        return Some(canonicalize_ip(ip));
    }
    if let Ok(sock) = raw.parse::<std::net::SocketAddr>() {
        return Some(canonicalize_ip(sock.ip()));
    }
    if let Some((host, _port)) = raw.rsplit_once(':') {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Some(canonicalize_ip(ip));
        }
    }
    None
}

fn should_hide_device_ip(raw_ip: &str, infra_ips: &HashSet<IpAddr>) -> bool {
    let Some(ip) = parse_ip_maybe(raw_ip) else {
        return true;
    };
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return true;
    }
    infra_ips.contains(&ip)
}

#[derive(Debug, Clone, Copy)]
enum BroadcastSegment {
    All,
    Active,
    Expiring72h,
    Trial,
    NoActive,
    Banned,
}

impl BroadcastSegment {
    fn parse(input: Option<&str>) -> Self {
        match input.unwrap_or("all").trim().to_ascii_lowercase().as_str() {
            "active" => Self::Active,
            "expiring_72h" => Self::Expiring72h,
            "trial" => Self::Trial,
            "no_active" => Self::NoActive,
            "banned" => Self::Banned,
            _ => Self::All,
        }
    }

    fn as_db(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Expiring72h => "expiring_72h",
            Self::Trial => "trial",
            Self::NoActive => "no_active",
            Self::Banned => "banned",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All users",
            Self::Active => "Active subscribers",
            Self::Expiring72h => "Expiring in 72h",
            Self::Trial => "Trial users",
            Self::NoActive => "Without active subscription",
            Self::Banned => "Banned users",
        }
    }
}

async fn ensure_notification_tables(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notification_campaigns (
            id BIGSERIAL PRIMARY KEY,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            title TEXT,
            created_by_username TEXT,
            target_segment TEXT NOT NULL,
            parse_mode TEXT NOT NULL DEFAULT 'plain',
            media_type TEXT NOT NULL DEFAULT 'none',
            media_url TEXT,
            button_text TEXT,
            button_url TEXT,
            buttons_json JSONB,
            message_text TEXT NOT NULL,
            planned_count INTEGER NOT NULL DEFAULT 0,
            sent_count INTEGER NOT NULL DEFAULT 0,
            failed_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'created'
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("ALTER TABLE notification_campaigns ADD COLUMN IF NOT EXISTS title TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE notification_campaigns ADD COLUMN IF NOT EXISTS buttons_json JSONB")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notification_deliveries (
            id BIGSERIAL PRIMARY KEY,
            campaign_id BIGINT NOT NULL REFERENCES notification_campaigns(id) ON DELETE CASCADE,
            user_id BIGINT,
            tg_id BIGINT NOT NULL,
            status TEXT NOT NULL,
            error_text TEXT,
            sent_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_notification_campaigns_created_at ON notification_campaigns(created_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_notification_deliveries_campaign ON notification_deliveries(campaign_id, sent_at DESC)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn fetch_campaign_history(
    pool: &sqlx::PgPool,
) -> Result<Vec<NotificationCampaignHistory>, sqlx::Error> {
    let rows: Vec<NotificationCampaignRow> = sqlx::query_as(
        r#"
        SELECT
            id,
            created_at,
            title,
            created_by_username,
            target_segment,
            parse_mode,
            media_type,
            buttons_json,
            planned_count,
            sent_count,
            failed_count,
            status,
            message_text
        FROM notification_campaigns
        ORDER BY id DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await?;

    let result = rows
        .into_iter()
        .map(|row| {
            let mut preview = row.message_text.replace('\n', " ");
            if preview.chars().count() > 110 {
                preview = format!("{}...", preview.chars().take(110).collect::<String>());
            }
            let buttons_count = row
                .buttons_json
                .as_ref()
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            NotificationCampaignHistory {
                id: row.id,
                created_at: row.created_at.format("%Y-%m-%d %H:%M UTC").to_string(),
                title: row.title.unwrap_or_else(|| "Untitled".to_string()),
                created_by: row
                    .created_by_username
                    .unwrap_or_else(|| "system".to_string()),
                segment: BroadcastSegment::parse(Some(&row.target_segment))
                    .label()
                    .to_string(),
                parse_mode: parse_notification_mode_label(&row.parse_mode).to_string(),
                media_type: parse_notification_media_label(&row.media_type).to_string(),
                buttons_count,
                sent: row.sent_count,
                failed: row.failed_count,
                planned: row.planned_count,
                status: row.status,
                message_preview: preview,
            }
        })
        .collect();

    Ok(result)
}

async fn fetch_segment_user_ids(
    pool: &sqlx::PgPool,
    sql: &str,
) -> Result<HashSet<i64>, sqlx::Error> {
    let ids: Vec<i64> = sqlx::query_scalar(sql).fetch_all(pool).await?;
    Ok(ids.into_iter().collect())
}

async fn filter_users_by_segment(
    pool: &sqlx::PgPool,
    users: Vec<User>,
    segment: BroadcastSegment,
) -> Result<Vec<User>, sqlx::Error> {
    let users: Vec<User> = users.into_iter().filter(|u| u.tg_id > 0).collect();
    match segment {
        BroadcastSegment::All => Ok(users),
        BroadcastSegment::Banned => Ok(users.into_iter().filter(|u| u.is_banned).collect()),
        BroadcastSegment::Active => {
            let active = fetch_segment_user_ids(
                pool,
                "SELECT DISTINCT user_id FROM subscriptions WHERE status = 'active' AND expires_at > NOW()",
            )
            .await?;
            Ok(users
                .into_iter()
                .filter(|u| active.contains(&u.id))
                .collect())
        }
        BroadcastSegment::Expiring72h => {
            let expiring = fetch_segment_user_ids(
                pool,
                "SELECT DISTINCT user_id FROM subscriptions WHERE status = 'active' AND expires_at > NOW() AND expires_at <= NOW() + INTERVAL '72 hours'",
            )
            .await?;
            Ok(users
                .into_iter()
                .filter(|u| expiring.contains(&u.id))
                .collect())
        }
        BroadcastSegment::Trial => {
            let trial = fetch_segment_user_ids(
                pool,
                "SELECT DISTINCT user_id FROM subscriptions WHERE status = 'active' AND expires_at > NOW() AND COALESCE(is_trial, false) = TRUE",
            )
            .await?;
            Ok(users
                .into_iter()
                .filter(|u| trial.contains(&u.id))
                .collect())
        }
        BroadcastSegment::NoActive => {
            let active = fetch_segment_user_ids(
                pool,
                "SELECT DISTINCT user_id FROM subscriptions WHERE status = 'active' AND expires_at > NOW()",
            )
            .await?;
            Ok(users
                .into_iter()
                .filter(|u| !active.contains(&u.id))
                .collect())
        }
    }
}

fn parse_notification_button(
    text: Option<String>,
    url: Option<String>,
    index: usize,
) -> Result<Option<(String, String)>, String> {
    let text = normalize_optional(text);
    let url = normalize_optional(url);
    if text.is_some() ^ url.is_some() {
        return Err(format!("Button #{} requires both text and URL", index + 1));
    }

    match (text, url) {
        (Some(text), Some(url)) => {
            url::Url::parse(url.trim()).map_err(|_| {
                format!(
                    "Button #{} URL must be a valid absolute URL (https://...)",
                    index + 1
                )
            })?;
            Ok(Some((text, url)))
        }
        _ => Ok(None),
    }
}

fn build_notification_payload(form: NotifyForm) -> Result<NotificationPayload, String> {
    let title = normalize_optional(form.campaign_title.clone());
    let message = form.message.trim().to_string();
    if message.is_empty() {
        return Err("Message cannot be empty".to_string());
    }

    let legacy_image_url = normalize_optional(form.image_url);
    let media_url = normalize_optional(form.media_url).or(legacy_image_url.clone());
    let mut media_type = parse_notification_media_type(form.media_type.as_deref());
    if media_url.is_some() && matches!(media_type, NotificationMediaType::None) {
        media_type = NotificationMediaType::Photo;
    }

    if let Some(url) = media_url.as_ref() {
        let parsed = url::Url::parse(url.trim())
            .map_err(|_| "Media URL must be a valid absolute URL (https://...)".to_string())?;
        match parsed.scheme() {
            "https" | "http" => {}
            _ => {
                return Err("Media URL must use http:// or https://".to_string());
            }
        }
    }

    if media_url.is_some() && message.chars().count() > 1024 {
        return Err("When media is set, message must be <= 1024 chars".to_string());
    }

    let mut buttons: Vec<(String, String)> = Vec::new();
    if let Some(button) = parse_notification_button(form.button_text, form.button_url, 0)? {
        buttons.push(button);
    }
    if let Some(button) = parse_notification_button(form.button2_text, form.button2_url, 1)? {
        buttons.push(button);
    }
    if let Some(button) = parse_notification_button(form.button3_text, form.button3_url, 2)? {
        buttons.push(button);
    }

    let text = match title {
        Some(title) => format!("{}\n\n{}", title, message),
        None => message,
    };

    Ok(NotificationPayload {
        text,
        parse_mode: parse_notification_mode(form.parse_mode.as_deref()),
        media_type,
        media_url,
        buttons,
        disable_link_preview: form.disable_link_preview.is_some(),
    })
}

fn render_notification_preview_html(payload: &NotificationPayload) -> String {
    let mode_label = notification_parse_mode_label_for_payload(payload.parse_mode);
    let media_label = notification_media_type_label_for_payload(payload.media_type);
    let message_body = escape_html(&payload.text).replace('\n', "<br>");

    let mut media_html = String::new();
    if let Some(media_url) = payload.media_url.as_ref() {
        let media_url = escape_html(media_url);
        media_html = match payload.media_type {
            NotificationMediaType::Photo => format!(
                r#"<div class="rounded-xl border border-white/10 bg-slate-950/40 p-2">
                        <img src="{url}" alt="Notification media preview" class="max-h-56 w-auto rounded-lg border border-white/10">
                    </div>"#,
                url = media_url
            ),
            NotificationMediaType::Video => format!(
                r#"<div class="rounded-xl border border-white/10 bg-slate-950/40 p-2">
                        <video src="{url}" controls class="max-h-56 w-full rounded-lg border border-white/10"></video>
                    </div>"#,
                url = media_url
            ),
            NotificationMediaType::Document => format!(
                r#"<a href="{url}" target="_blank" rel="noopener noreferrer"
                        class="inline-flex items-center gap-2 px-3 py-2 rounded-lg border border-indigo-500/30 bg-indigo-500/10 text-indigo-300 text-xs hover:bg-indigo-500/20 transition-colors">
                        Open document URL
                    </a>"#,
                url = media_url
            ),
            NotificationMediaType::None => String::new(),
        };
    }

    let buttons_html = if payload.buttons.is_empty() {
        "<span class=\"text-xs text-slate-500\">No action buttons</span>".to_string()
    } else {
        payload
            .buttons
            .iter()
            .map(|(text, url)| {
                format!(
                    r#"<a href="{url}" target="_blank" rel="noopener noreferrer"
                        class="inline-flex items-center gap-2 px-3 py-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 text-emerald-300 text-xs hover:bg-emerald-500/20 transition-colors">
                        {text}
                    </a>"#,
                    url = escape_html(url),
                    text = escape_html(text)
                )
            })
            .collect::<Vec<String>>()
            .join("\n")
    };

    format!(
        r#"
        <div class="mt-3 rounded-2xl border border-cyan-500/20 bg-cyan-500/5 p-4 space-y-3">
            <div class="flex flex-wrap items-center gap-2 text-[11px]">
                <span class="rounded-full border border-white/10 bg-slate-950/40 px-2.5 py-1 text-slate-300">Mode: <span class="text-white">{mode}</span></span>
                <span class="rounded-full border border-white/10 bg-slate-950/40 px-2.5 py-1 text-slate-300">Media: <span class="text-white">{media}</span></span>
                <span class="rounded-full border border-white/10 bg-slate-950/40 px-2.5 py-1 text-slate-300">Link preview: <span class="text-white">{link_preview}</span></span>
            </div>

            <div class="rounded-xl border border-white/10 bg-slate-950/50 p-4 text-sm text-slate-100 leading-relaxed">
                {body}
            </div>

            {media_html}

            <div class="flex flex-wrap gap-2">
                {buttons_html}
            </div>
        </div>
        "#,
        mode = mode_label,
        media = media_label,
        link_preview = if payload.disable_link_preview {
            "disabled"
        } else {
            "enabled"
        },
        body = message_body,
        media_html = media_html,
        buttons_html = buttons_html
    )
}

// Helper function
fn format_duration(duration: chrono::TimeDelta) -> String {
    if duration.num_seconds() < 60 {
        format!("{} sec", duration.num_seconds())
    } else if duration.num_minutes() < 60 {
        format!("{} min", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{} hr", duration.num_hours())
    } else {
        format!("{} days", duration.num_days())
    }
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_users(
    State(state): State<AppState>,
    jar: CookieJar,
    query: Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let search = query.get("search").cloned().unwrap_or_default();
    let users = if search.is_empty() {
        state.user_service.get_all().await.unwrap_or_default()
    } else {
        state.user_service.search(&search).await.unwrap_or_default()
    };

    if let Err(e) = ensure_notification_tables(&state.pool).await {
        error!("Failed to ensure notification tables: {}", e);
    }
    let campaigns = match fetch_campaign_history(&state.pool).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to fetch notification campaign history: {}", e);
            Vec::new()
        }
    };

    let template = UsersTemplate {
        users,
        search,
        campaigns,
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(),
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

pub async fn admin_gift_subscription(
    Path(user_id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<AdminGiftForm>,
) -> impl IntoResponse {
    let duration = match state
        .catalog_service
        .get_plan_duration_by_id(form.duration_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => {
            return (axum::http::StatusCode::BAD_REQUEST, "Invalid duration ID").into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("DB Error: {}", e),
            )
                .into_response();
        }
    };

    match state
        .subscription_service
        .admin_gift_subscription(user_id, duration.plan_id, duration.duration_days)
        .await
    {
        Ok(sub) => {
            if let Ok(Some(user)) = state.user_service.get_by_id(user_id).await {
                let msg = format!(
                    "🎁 *Gift Received\\!*\\n\\nYou have received a new subscription\\.\\nExpires: {}",
                    sub.expires_at.format("%Y-%m-%d")
                );
                let _ = state.bot_manager.send_notification(user.tg_id, &msg).await;
            }

            let admin_path = state.admin_path.clone();
            return axum::response::Redirect::to(&format!("{}/users/{}", admin_path, user_id))
                .into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to gift subscription: {}", e),
            )
                .into_response();
        }
    }
}

pub async fn get_user_details(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let user = state.user_service.get_by_id(id).await.unwrap_or(None);

    let user = match user {
        Some(u) => u,
        None => return (axum::http::StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let raw_subscriptions = match state
        .subscription_service
        .get_subscriptions_with_details_for_admin(id)
        .await
    {
        Ok(subs) => subs,
        Err(e) => {
            error!("Failed to fetch user subscriptions: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch subs: {}", e),
            )
                .into_response();
        }
    };

    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let panel_url = state.settings.get_or_default("panel_url", "").await;
    let base_domain = if !sub_domain.is_empty() {
        sub_domain
    } else if !panel_url.is_empty() {
        panel_url
    } else {
        env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
    };
    let base_url = if base_domain.starts_with("http") {
        base_domain
    } else {
        format!("https://{}", base_domain)
    };

    let mut subscriptions = Vec::with_capacity(raw_subscriptions.len());
    for sub in raw_subscriptions {
        let full_sub = match state.subscription_service.get_by_id(sub.id).await {
            Ok(Some(full)) => Some(full),
            Ok(None) => None,
            Err(e) => {
                error!("Failed to fetch full subscription {}: {}", sub.id, e);
                None
            }
        };
        let sub_uuid = full_sub
            .as_ref()
            .map(|full| full.subscription_uuid.clone())
            .unwrap_or_else(|| format!("legacy-{}", sub.id));

        let last_sub_access_label = full_sub.as_ref().and_then(|full| {
            full.last_sub_access
                .as_ref()
                .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        });

        let last_node_label = if let Some(node_id) = full_sub.as_ref().and_then(|full| full.node_id)
        {
            match sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT name, flag FROM nodes WHERE id = $1",
            )
            .bind(node_id)
            .fetch_optional(&state.pool)
            .await
            {
                Ok(Some((name, flag))) => Some(match flag {
                    Some(f) if !f.trim().is_empty() => format!("{} {} (#{})", f, name, node_id),
                    _ => format!("{} (#{})", name, node_id),
                }),
                Ok(None) => Some(format!("#{}", node_id)),
                Err(e) => {
                    error!(
                        "Failed to resolve node label for subscription {} node {}: {}",
                        sub.id, node_id, e
                    );
                    Some(format!("#{}", node_id))
                }
            }
        } else {
            None
        };

        let direct_links = match state
            .subscription_service
            .get_subscription_links(sub.id)
            .await
        {
            Ok(links) => links,
            Err(e) => {
                error!(
                    "Failed to fetch direct connection links for sub {}: {}",
                    sub.id, e
                );
                Vec::new()
            }
        };
        let vless_links: Vec<String> = direct_links
            .into_iter()
            .filter(|link| link.starts_with("vless://"))
            .collect();
        let primary_vless_link = vless_links.first().cloned();

        subscriptions.push(AdminSubscriptionView {
            id: sub.id,
            plan_name: sub.plan_name,
            expires_at: sub.expires_at,
            created_at: sub.created_at,
            status: sub.status,
            price: sub.price,
            active_devices: sub.active_devices,
            device_limit: sub.device_limit,
            subscription_url: format!("{}/sub/{}", base_url, sub_uuid),
            primary_vless_link,
            vless_links_count: vless_links.len(),
            last_node_label,
            last_sub_access_label,
        });
    }

    let db_orders = state
        .billing_service
        .get_user_orders(id)
        .await
        .map_err(|e| {
            error!("Failed to fetch user orders: {}", e);
            e
        })
        .unwrap_or_default();

    let orders = db_orders
        .into_iter()
        .map(|o| UserOrderDisplay {
            id: o.id,
            total_amount: format!("{:.2}", o.total_amount as f64 / 100.0),
            status: o.status,
            created_at: o.created_at.format("%Y-%m-%d").to_string(),
        })
        .collect();

    let referrals =
        crate::services::referral_service::ReferralService::get_user_referrals(&state.pool, id)
            .await
            .unwrap_or_default();
    let earnings_cents =
        crate::services::referral_service::ReferralService::get_user_referral_earnings(
            &state.pool,
            id,
        )
        .await
        .unwrap_or(0);

    let available_plans = state
        .catalog_service
        .get_active_plans()
        .await
        .unwrap_or_default();

    let template = UserDetailsTemplate {
        user,
        subscriptions,
        orders,
        referrals,
        total_referral_earnings: format!("{:.2}", earnings_cents as f64 / 100.0),
        available_plans,
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(),
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

pub async fn update_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateUserForm>,
) -> impl IntoResponse {
    let old_user = state.user_service.get_by_id(id).await.unwrap_or(None);

    let res = state
        .user_service
        .update_profile(
            id,
            form.balance,
            form.is_banned,
            form.referral_code.as_deref().map(|s| s.trim()),
        )
        .await;

    // Update parent if changed
    let pid = form
        .parent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|&id| id > 0);

    let _ = state.store_service.set_user_parent(id, pid).await;

    match res {
        Ok(_) => {
            let _ = crate::services::activity_service::ActivityService::log(
                &state.pool,
                "User",
                &format!(
                    "User {} updated: Balance={}, Banned={}",
                    id, form.balance, form.is_banned
                ),
            )
            .await;

            if let Some(u) = old_user {
                if u.is_banned != form.is_banned {
                    let msg = if form.is_banned {
                        "🚫 *Account Banned*\\n\\nYour account has been suspended by an administrator\\."
                    } else {
                        "✅ *Account Unbanned*\\n\\nYour account has been reactivated\\. Welcome back\\!"
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, msg).await;
                }

                if u.balance != form.balance {
                    let diff = form.balance - u.balance;
                    let amount = format!("{:.2}", diff.abs() as f64 / 100.0);
                    let msg = if diff > 0 {
                        format!(
                            "💰 *Balance Updated*\\n\\nAdministrator added *${}* to your account\\.",
                            amount
                        )
                    } else {
                        format!(
                            "📉 *Balance Updated*\\n\\nAdministrator deducted *${}* from your account\\.",
                            amount
                        )
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, &msg).await;
                }
            }

            let admin_path = state.admin_path.clone();
            (
                [(("HX-Redirect", format!("{}/users/{}", admin_path, id)))],
                "Updated",
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to update user {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update user",
            )
                .into_response()
        }
    }
}

pub async fn update_user_balance(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let balance_str = form.get("balance").unwrap_or(&"0".to_string()).clone();
    let balance: i64 = balance_str.parse().unwrap_or(0);

    let res = state.user_service.set_balance(id, balance).await;

    match res {
        Ok(_) => {
            let _ = LoggingService::log_system(
                &state.pool,
                "admin_update_balance",
                &format!("Admin updated user {} balance to {} cents", id, balance),
            )
            .await;

            let admin_path = state.admin_path.clone();
            (
                [(("HX-Redirect", format!("{}/users", admin_path)))],
                "Updated",
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to update balance for user {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update balance",
            )
                .into_response()
        }
    }
}

pub async fn delete_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete subscription ID: {}", id);
    match state.subscription_service.admin_delete(id).await {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(),
        Err(e) => {
            error!("Failed to delete subscripton {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn refund_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<RefundForm>,
) -> impl IntoResponse {
    info!(
        "Request to refund subscription ID: {} with amount {}",
        id, form.amount
    );
    match state
        .catalog_service
        .admin_refund_subscription(id, form.amount)
        .await
    {
        Ok(_) => ([(("HX-Refresh", "true"))], "Refunded").into_response(),
        Err(e) => {
            error!("Failed to refund subscripton {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to refund: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn extend_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<ExtendForm>,
) -> impl IntoResponse {
    info!(
        "Request to extend subscription ID: {} by {} days",
        id, form.days
    );
    match state.subscription_service.admin_extend(id, form.days).await {
        Ok(_) => ([(("HX-Refresh", "true"))], "Extended").into_response(),
        Err(e) => {
            error!("Failed to extend subscripton {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to extend: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn notify_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<NotifyForm>,
) -> impl IntoResponse {
    let payload = match build_notification_payload(form) {
        Ok(payload) => payload,
        Err(msg) => return (axum::http::StatusCode::BAD_REQUEST, msg).into_response(),
    };

    let user = match state.user_service.get_by_id(id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (axum::http::StatusCode::NOT_FOUND, "User not found").into_response();
        }
        Err(e) => {
            error!("Failed to fetch user {} for notification: {}", id, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch user",
            )
                .into_response();
        }
    };

    match state
        .bot_manager
        .send_rich_notification(user.tg_id, payload)
        .await
    {
        Ok(_) => (
            axum::http::StatusCode::OK,
            format!("Notification sent to user {}", id),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to send notification: {}", e),
        )
            .into_response(),
    }
}

pub async fn notify_preview(Form(form): Form<NotifyForm>) -> impl IntoResponse {
    match build_notification_payload(form) {
        Ok(payload) => Html(render_notification_preview_html(&payload)).into_response(),
        Err(msg) => (
            axum::http::StatusCode::BAD_REQUEST,
            Html(format!(
                "<div class=\"mt-3 rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300\">{}</div>",
                escape_html(&msg)
            )),
        )
            .into_response(),
    }
}

pub async fn notify_all_users(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<NotifyForm>,
) -> impl IntoResponse {
    let segment = BroadcastSegment::parse(form.segment.as_deref());
    let campaign_title = normalize_optional(form.campaign_title.clone())
        .unwrap_or_else(|| "Untitled broadcast".to_string());
    let payload = match build_notification_payload(form.clone()) {
        Ok(payload) => payload,
        Err(msg) => return (axum::http::StatusCode::BAD_REQUEST, msg).into_response(),
    };

    if let Err(e) = ensure_notification_tables(&state.pool).await {
        error!(
            "Failed to ensure notification tables before broadcast: {}",
            e
        );
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to prepare notification storage",
        )
            .into_response();
    }

    let users_all = match state.user_service.get_all().await {
        Ok(users) => users,
        Err(e) => {
            error!("Failed to fetch users for broadcast: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch users",
            )
                .into_response();
        }
    };

    let users = match filter_users_by_segment(&state.pool, users_all, segment).await {
        Ok(users) => users,
        Err(e) => {
            error!("Failed to filter users for broadcast segment: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to filter users by segment",
            )
                .into_response();
        }
    };

    let created_by = get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());
    let parse_mode_db = notification_parse_mode_db_value(payload.parse_mode);
    let media_type_db = notification_media_type_db_value(payload.media_type);
    let planned_count = users.len().min(i32::MAX as usize) as i32;
    let primary_button_text = payload.buttons.first().map(|b| b.0.clone());
    let primary_button_url = payload.buttons.first().map(|b| b.1.clone());
    let buttons_json: serde_json::Value = serde_json::Value::Array(
        payload
            .buttons
            .iter()
            .map(|(text, url)| {
                serde_json::json!({
                    "text": text,
                    "url": url,
                })
            })
            .collect(),
    );

    let campaign_id = match sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO notification_campaigns
            (title, created_by_username, target_segment, parse_mode, media_type, media_url, button_text, button_url, buttons_json, message_text, planned_count, status)
        VALUES
            ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'running')
        RETURNING id
        "#,
    )
    .bind(campaign_title)
    .bind(created_by)
    .bind(segment.as_db())
    .bind(parse_mode_db)
    .bind(media_type_db)
    .bind(payload.media_url.clone())
    .bind(primary_button_text)
    .bind(primary_button_url)
    .bind(buttons_json)
    .bind(payload.text.clone())
    .bind(planned_count)
    .fetch_one(&state.pool)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            error!("Failed to create notification campaign row: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create notification campaign",
            )
                .into_response();
        }
    };

    let mut sent = 0usize;
    let mut failed = 0usize;
    for user in users {
        if user.tg_id <= 0 || user.is_banned {
            continue;
        }
        let send_result = state
            .bot_manager
            .send_rich_notification(user.tg_id, payload.clone())
            .await;

        match send_result {
            Ok(_) => {
                sent += 1;
                if let Err(e) = sqlx::query(
                    "INSERT INTO notification_deliveries (campaign_id, user_id, tg_id, status, error_text) VALUES ($1, $2, $3, 'sent', NULL)",
                )
                .bind(campaign_id)
                .bind(user.id)
                .bind(user.tg_id)
                .execute(&state.pool)
                .await
                {
                    error!(
                        "Failed to store notification delivery success (campaign {}, user {}): {}",
                        campaign_id, user.id, e
                    );
                }
            }
            Err(e) => {
                failed += 1;
                error!(
                    "Failed to send broadcast notification to tg_id {}: {}",
                    user.tg_id, e
                );
                let err_text = e.to_string();
                if let Err(db_err) = sqlx::query(
                    "INSERT INTO notification_deliveries (campaign_id, user_id, tg_id, status, error_text) VALUES ($1, $2, $3, 'failed', $4)",
                )
                .bind(campaign_id)
                .bind(user.id)
                .bind(user.tg_id)
                .bind(err_text.clone())
                .execute(&state.pool)
                .await
                {
                    error!(
                        "Failed to store notification delivery failure (campaign {}, user {}): {}",
                        campaign_id, user.id, db_err
                    );
                }
            }
        }
    }

    let status = if sent == 0 && failed > 0 {
        "failed"
    } else if failed > 0 {
        "partial"
    } else {
        "completed"
    };

    if let Err(e) = sqlx::query(
        "UPDATE notification_campaigns SET sent_count = $2, failed_count = $3, status = $4 WHERE id = $1",
    )
    .bind(campaign_id)
    .bind(sent.min(i32::MAX as usize) as i32)
    .bind(failed.min(i32::MAX as usize) as i32)
    .bind(status)
    .execute(&state.pool)
    .await
    {
        error!(
            "Failed to finalize notification campaign {} with counters: {}",
            campaign_id, e
        );
    }

    (
        axum::http::StatusCode::OK,
        format!(
            "Broadcast complete ({}): sent={}, failed={}",
            segment.label(),
            sent,
            failed
        ),
    )
        .into_response()
}

pub async fn get_subscription_devices(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let ips_raw = match state.subscription_service.get_active_ips(sub_id).await {
        Ok(ips) => ips,
        Err(e) => {
            error!("Failed to fetch IPs for sub {}: {}", sub_id, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch devices",
            )
                .into_response();
        }
    };

    let infra_rows: Vec<String> = sqlx::query_scalar("SELECT ip FROM nodes")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    let infra_ips: HashSet<IpAddr> = infra_rows
        .into_iter()
        .filter_map(|ip| parse_ip_maybe(&ip))
        .collect();

    let ips: Vec<_> = ips_raw
        .into_iter()
        .filter(|row| !should_hide_device_ip(&row.client_ip, &infra_ips))
        .collect();

    let admin_path = state.admin_path.clone();

    let mut html = String::new();

    html.push_str(&format!(
        r##"
        <div class="flex justify-between items-center mb-6 p-4 rounded-2xl bg-orange-500/10 border border-orange-500/10 shadow-lg shadow-orange-500/5">
            <div>
                <p class="text-sm font-bold text-orange-400 mb-0.5">Manage Active Sessions</p>
                <p class="text-[11px] text-slate-500">Disconnect all current devices immediately</p>
            </div>
            <button hx-post="{}/subs/{}/devices/kill" hx-target="#devices_content" hx-confirm="This will disconnect ALL currently connected users for this subscription. Continue?"
                class="px-4 py-2 rounded-xl bg-orange-600 hover:bg-orange-500 text-white text-xs font-bold transition-all shadow-lg shadow-orange-500/20 active:scale-95">
                Reset All
            </button>
        </div>
        "##, admin_path, sub_id
    ));

    if ips.is_empty() {
        html.push_str("<div class='py-12 text-center text-slate-500 border border-white/5 rounded-2xl bg-slate-950/20'><p class='text-sm'>No active devices detected in the last 15 minutes.</p></div>");
        return Html(html).into_response();
    }

    html.push_str("<div class='overflow-hidden rounded-2xl border border-white/5 bg-slate-950/30 shadow-inner'>");
    html.push_str("<table class='w-full text-left border-collapse'>");
    html.push_str("<thead><tr class='text-[10px] font-bold text-slate-500 uppercase tracking-widest bg-white/5'><th class='px-6 py-3'>Device / Client</th><th class='px-6 py-3'>Address</th><th class='px-6 py-3'>Activity</th></tr></thead>");
    html.push_str("<tbody class='divide-y divide-white/5'>");
    for ip_record in ips {
        let time_ago = format_duration(chrono::Utc::now() - ip_record.last_seen_at);
        let device = ip_record
            .user_agent
            .unwrap_or_else(|| "Unknown".to_string());
        html.push_str(&format!(
            "<tr class='hover:bg-white/5 transition-colors'><td class='px-6 py-4 text-xs font-semibold text-white'>{}</td><td class='px-6 py-4 text-xs text-indigo-400 font-mono'>{}</td><td class='px-6 py-4 text-[10px] text-slate-400 font-medium'>{} ago</td></tr>",
            device, ip_record.client_ip, time_ago
        ));
    }
    html.push_str("</tbody></table></div>");

    Html(html).into_response()
}

pub async fn admin_kill_subscription_sessions(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let sub = match state.subscription_service.get_by_id(sub_id).await {
        Ok(Some(s)) => s,
        _ => return (axum::http::StatusCode::NOT_FOUND, "Subscription not found").into_response(),
    };

    if let Err(e) = state
        .connection_service
        .kill_subscription_connections(sub.id)
        .await
    {
        error!("Admin failed to kill sessions for sub {}: {}", sub_id, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to kill sessions: {}", e),
        )
            .into_response();
    }

    let success_html = format!(
        r##"
        <div class="flex flex-col items-center justify-center py-12 text-center animate-fade-in">
            <div class="w-20 h-20 rounded-3xl bg-emerald-500/10 flex items-center justify-center mb-6 text-emerald-400 border border-emerald-500/20 shadow-xl shadow-emerald-500/10 transform rotate-3">
                <i data-lucide='check-circle' class="w-10 h-10"></i>
            </div>
            <h4 class="text-xl font-bold text-white mb-2 tracking-tight">Sessions Reset Successfully</h4>
            <p class="text-sm text-slate-500 mb-8 px-12 leading-relaxed">All active connections for subscription #{} have been terminated. It may take up to 60 seconds for all caches to clear.</p>
            <button hx-get="{}/subs/{}/devices" hx-target="#devices_content"
                class="px-5 py-2.5 rounded-xl bg-white/10 hover:bg-white/20 border border-white/10 text-white text-sm font-bold transition-all active:scale-95" style="backdrop-filter: blur(10px);">
                Refresh Device List
            </button>
        </div>
        "##,
        sub_id, state.admin_path, sub_id
    );

    Html(success_html).into_response()
}
