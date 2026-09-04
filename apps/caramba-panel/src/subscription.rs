use axum::{
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, warn};

use crate::AppState;

/// Rate-limits the "device limit reached" Telegram notification so a client that keeps
/// retrying a rejected connection does not spam the user. Returns true at most once per
/// 10 minutes per subscription.
fn should_notify_device_block(sub_id: i64) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    static LAST: OnceLock<Mutex<HashMap<i64, Instant>>> = OnceLock::new();
    let map = LAST.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    let Ok(mut guard) = map.lock() else {
        // Poisoned lock: fail open (allow the notification) rather than panic.
        return true;
    };
    if guard.len() > 10_000 {
        guard.retain(|_, t| now.duration_since(*t) < Duration::from_secs(3600));
    }
    match guard.get(&sub_id) {
        Some(t) if now.duration_since(*t) < Duration::from_secs(600) => false,
        _ => {
            guard.insert(sub_id, now);
            true
        }
    }
}

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>, // "clash" | "v2ray" | "singbox"
    pub node_id: Option<i64>,
    pub variant: Option<String>,
    pub relay_country: Option<String>, // e.g. "RU", "US", "none" — override geo-based relay selection
}

/// Redis key marking "the app owns this subscription's node/relay selection".
///
/// Written by `PUT /api/v2/app/subscriptions/{id}/selection`. While the marker
/// lives, a config GET never persists `node_id` / `relay_country`: the query
/// parameters stay request-scoped filters instead of silently becoming the
/// user's stored choice. Without it a stale subscription URL — the app hands
/// out URLs that still carry `?node_id=` / `?relay_country=` from an earlier
/// pick — would overwrite the selection the user just made in the app.
pub fn app_selection_marker_key(sub_id: i64) -> String {
    format!("app_selection_owner:{}", sub_id)
}

/// Marker TTL: 180 days. The marker only suppresses the implicit write-on-GET,
/// and the two columns degrade differently if Redis loses it: `node_id` still
/// has its `WHERE ... IS NULL` guard and cannot be clobbered, while
/// `relay_country` falls back to last-write-wins (see
/// [`persist_relay_from_url`] for why it must). Losing the marker therefore has
/// one visible consequence — a device replaying an old `?relay_country=` can
/// re-pin the relay. It is self-healing: the app sends its OWN current relay on
/// every panel-path config fetch, so its next fetch writes the right value back.
pub const APP_SELECTION_MARKER_TTL_SECS: usize = 60 * 60 * 24 * 180;

/// True when the app has claimed authority over this subscription's selection.
/// Only consulted right before a would-be write, so the common config fetch
/// pays no extra Redis round-trip.
async fn app_owns_selection(state: &AppState, sub_id: i64) -> bool {
    matches!(
        state.redis.get(&app_selection_marker_key(sub_id)).await,
        Ok(Some(_))
    )
}

/// Records a node the config path picked, but only while the column is still
/// unset. The `IS NULL` predicate lives in the statement rather than in Rust so
/// that a concurrent `PUT .../selection` cannot slip in between our read of
/// `sub.node_id` (taken far earlier in this handler) and this write.
async fn persist_node_if_unset(state: &AppState, sub_id: i64, node_id: i64) {
    let _ = sqlx::query("UPDATE subscriptions SET node_id = $1 WHERE id = $2 AND node_id IS NULL")
        .bind(node_id)
        .bind(sub_id)
        .execute(&state.pool)
        .await;
}

/// Canonical form of a `?relay_country=` value, or `None` if it is not a
/// selection at all.
///
/// Two shapes are a selection: `"none"` (deliberately no relay) and an ISO-2
/// alpha country code, stored upper-case. Everything else — an empty string, a
/// stray `"auto"`, a three-letter typo — is a client artefact, not a user
/// choice, and must never reach the column. Production still carries one row
/// with `relay_country = ''`, written back when this path stored whatever
/// arrived; that row is what this guard exists to stop repeating.
///
/// Read-side matching (`relay_filter_cc` below) is already case-insensitive, so
/// normalising on write only makes the stored value canonical — it does not
/// change which relays anyone gets.
fn normalize_relay_param(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Some("none".to_string());
    }
    if trimmed.len() == 2 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(trimmed.to_ascii_uppercase());
    }
    None
}

/// Records the relay carried by `?relay_country=` as the subscription's stored
/// choice.
///
/// Deliberately NOT `..._if_unset`, unlike [`persist_node_if_unset`]. That is
/// the shape this used to have, and on a table where every production row
/// already has a non-NULL `relay_country` it means the column freezes at its
/// first-ever value: the mini-app's relay picker has no other writer — it
/// expresses a pick only by baking it into the subscription URL — so an
/// `IS NULL` guard silently retires the picker for every existing subscriber.
/// The request served still filtered correctly, which is why nothing looked
/// broken; the NEXT fetch without the parameter served the old relay again.
///
/// What protects a newer choice instead is the caller's `app_owns_selection`
/// check plus `IS DISTINCT FROM` here: re-posting the value already stored
/// touches no row, so a client re-fetching its config produces no write at all.
async fn persist_relay_from_url(state: &AppState, sub_id: i64, relay_country: &str) {
    let Some(value) = normalize_relay_param(relay_country) else {
        return;
    };
    let _ = sqlx::query(
        "UPDATE subscriptions SET relay_country = $1 \
         WHERE id = $2 AND relay_country IS DISTINCT FROM $1",
    )
    .bind(&value)
    .bind(sub_id)
    .execute(&state.pool)
    .await;
}

fn parse_ip_maybe(value: &str) -> Option<std::net::IpAddr> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Ok(ip) = value.parse::<std::net::IpAddr>() {
        return Some(canonicalize_ip(ip));
    }
    if let Ok(sock) = value.parse::<std::net::SocketAddr>() {
        return Some(canonicalize_ip(sock.ip()));
    }
    if let Some((host, _port)) = value.rsplit_once(':')
        && let Ok(ip) = host.parse::<std::net::IpAddr>()
    {
        return Some(canonicalize_ip(ip));
    }
    None
}

fn canonicalize_ip(ip: std::net::IpAddr) -> std::net::IpAddr {
    match ip {
        std::net::IpAddr::V6(v6) => v6
            .to_ipv4()
            .map(std::net::IpAddr::V4)
            .unwrap_or(std::net::IpAddr::V6(v6)),
        other => other,
    }
}

fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
    let raw = headers
        .get("cf-connecting-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0");

    parse_ip_maybe(raw)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

fn filter_nodes_for_subscription<T, F>(
    nodes: Vec<T>,
    requested_node_id: Option<i64>,
    node_id_of: F,
) -> Vec<T>
where
    T: Clone,
    F: Fn(&T) -> i64,
{
    match requested_node_id {
        Some(nid) => nodes
            .into_iter()
            .filter(|node| node_id_of(node) == nid)
            .collect(),
        None => nodes,
    }
}

/// Moves the pinned node to the front of the served list, leaving every other
/// node in place. An ordering hint, not a selection — nothing is removed.
///
/// It exists because the pin stopped narrowing the body: the proxies the user
/// last chose should still be the first ones their client lists. What it
/// deliberately cannot do is pick for them. `generate_clash_config` puts the
/// `Auto-All` url-test group at the head of the CARAMBA selector, so a client
/// with no saved selection still latency-tests the whole fleet; only the group
/// builder could make the pin the default, and it owns the groups, not this
/// file.
///
/// A pin naming a node the subscription cannot reach (removed from the plan
/// group, or a relay) is silently a no-op — an unreachable preference must not
/// cost the user the rest of the fleet.
fn promote_pinned_node<T, F>(nodes: &mut Vec<T>, pinned_node_id: Option<i64>, node_id_of: F)
where
    F: Fn(&T) -> i64,
{
    let Some(pinned) = pinned_node_id else {
        return;
    };
    let Some(pos) = nodes.iter().position(|node| node_id_of(node) == pinned) else {
        return;
    };
    let node = nodes.remove(pos);
    nodes.insert(0, node);
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    req: Request,
) -> Response {
    // 0. Smart Routing: Redirect if subscription_domain is set and we are not on it
    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    if !sub_domain.is_empty()
        && let Some(host) = req
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
    {
        let host_clean = host.split(':').next().unwrap_or(host);
        let sub_domain_clean = sub_domain.split(':').next().unwrap_or(&sub_domain);

        if host_clean != sub_domain_clean {
            let proto = "https";
            let query = req
                .uri()
                .query()
                .map(|q| format!("?{}", q))
                .unwrap_or_default();
            let full_url = format!("{}://{}/sub/{}{}", proto, sub_domain, uuid, query);
            return axum::response::Redirect::permanent(&full_url).into_response();
        }
    }

    // 0.5 Extract IP, User-Agent, and country header for tracking
    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let client_ip = extract_client_ip(req.headers());
    // Try geo headers from reverse proxy (Caddy geo module, Cloudflare, etc.)
    let client_country_header = req
        .headers()
        .get("x-country-code")
        .or_else(|| req.headers().get("cf-ipcountry"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_uppercase())
        .filter(|cc| cc.len() == 2 && cc != "XX" && cc != "T1");

    // 1. Rate Limit (30 req / min per UUID)
    let rate_key = format!("rate:sub:{}", uuid);
    match state.redis.check_rate_limit(&rate_key, 30, 60).await {
        Ok(allowed) => {
            if !allowed {
                warn!("Rate limit exceeded for subscription {}", uuid);
                return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
            }
        }
        Err(e) => {
            error!("Rate limit check failed: {}", e);
        }
    }

    // 2. Get subscription
    let sub = match state
        .subscription_service
        .get_subscription_by_uuid(&uuid)
        .await
    {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "Subscription not found").into_response();
        }
    };

    // 3. Check if active
    if sub.status != "active" {
        return (StatusCode::FORBIDDEN, "Subscription inactive or expired").into_response();
    }

    // 3.2 Check traffic quota immediately on subscription fetch to enforce limits in real-time.
    match state
        .subscription_service
        .ensure_subscription_within_quota(sub.id)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                "Traffic limit reached. Subscription is expired.",
            )
                .into_response();
        }
        Err(e) => {
            error!(
                "Failed to evaluate quota for subscription {}: {}",
                sub.id, e
            );
        }
    }

    // 3.5 Enforce device limit (Phase 7)
    let active_ips = state
        .subscription_service
        .get_active_ips(sub.id)
        .await
        .unwrap_or_default();
    let current_ip = &client_ip;

    // Check if this is a new IP or if we're already at the limit
    let is_new_device = !active_ips.iter().any(|rec| rec.client_ip == *current_ip);

    if is_new_device {
        let device_limit = state
            .subscription_service
            .get_subscription_device_limit(sub.id)
            .await
            .unwrap_or(0);
        if device_limit > 0 && active_ips.len() >= device_limit as usize {
            warn!(
                "Device limit reached for subscription {}. Limit: {}, Active: {}",
                uuid,
                device_limit,
                active_ips.len()
            );

            // Notify the user in Telegram that a new device was rejected, throttled to at
            // most once per 10 minutes per subscription so retries don't spam the chat.
            if should_notify_device_block(sub.id) {
                let tg_id: Option<i64> =
                    sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                        .bind(sub.user_id)
                        .fetch_optional(&state.pool)
                        .await
                        .unwrap_or(None);

                if let Some(tg_id) = tg_id {
                    let lang = crate::bot::utils::lang_by_tg_id(&state, tg_id).await;
                    let msg = crate::bot::translations::tf(
                        lang,
                        "devices.blocked_dm",
                        &[
                            &crate::bot::utils::escape_html(current_ip),
                            &device_limit.to_string(),
                        ],
                    );
                    let _ = state
                        .bot_manager
                        .send_rich_notification(
                            tg_id,
                            crate::bot_manager::NotificationPayload::html(msg),
                        )
                        .await;
                }
            }

            return (StatusCode::FORBIDDEN, "Device limit reached").into_response();
        }
    }

    // 4. Update access tracking
    let _ = state
        .subscription_service
        .track_access(sub.id, &client_ip, user_agent.as_deref())
        .await;

    // 4.5 Prepare Usage Headers (for Hiddify/Sing-box)
    let plan_details = match state
        .subscription_service
        .get_user_subscriptions(sub.user_id)
        .await
    {
        Ok(subs) => subs
            .iter()
            .find(|s| s.sub.id == sub.id)
            .map(|s| (s.plan_name.clone(), s.traffic_limit_gb.unwrap_or(0)))
            .unwrap_or(("VPN Plan".to_string(), 0)),
        Err(_) => ("VPN Plan".to_string(), 0),
    };

    let traffic_limit_gb = plan_details.1;

    // Класс тарифа. На бесплатном плане энфорсмент считает по daily_traffic_mb,
    // а не по traffic_limit_gb, и без этих двух колонок весь показ ниже
    // (Subscription-Userinfo и HTML-страница) врал бы ровно на разницу между
    // 10 ГБ в колонке и 200 МБ, которые на самом деле разрешены.
    let (is_free, daily_traffic_mb): (bool, i32) = sqlx::query_as(
        "SELECT COALESCE(is_free, FALSE), COALESCE(daily_traffic_mb, 0) FROM plans WHERE id = $1",
    )
    .bind(sub.plan_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
    .unwrap_or((false, 0));
    let bonus_traffic_mb = crate::services::bonus_traffic::balance_mb(&state.pool, sub.user_id)
        .await
        .unwrap_or(0);
    // Ровно тот потолок, по которому подписку истекают/троттлят: лимит плана
    // (суточный на free, общий на платном) плюс бонусный трафик. None = безлимит.
    let enforced_limit_bytes = crate::services::bonus_traffic::plan_quota_limit_bytes(
        is_free,
        traffic_limit_gb as i64,
        daily_traffic_mb as i64,
        bonus_traffic_mb,
    );

    // Клампим used_traffic к нулю. Отрицательных значений в базе больше нет —
    // одноразовый онбординг-headroom засевал used_traffic в минус и был убран
    // вместе с ним (миграция 20260831120000), — но заголовок
    // Subscription-Userinfo уходит клиенту, и отрицательный download в нём
    // нестандартен в любом случае.
    let used_traffic_bytes = (sub.used_traffic as i64).max(0);
    let expire_timestamp = sub.expires_at.timestamp();

    // upload=0; download=used; total=limit; expire=timestamp.
    // Безлимит (None) отдаём без `total`, чтобы клиент нарисовал ∞. На
    // суточном плане `total` — это норма на сутки: клиент покажет расход из
    // 200 МБ, что совпадает с тем, за что его отключат, а не из 10 ГБ.
    let user_info_header = match enforced_limit_bytes {
        Some(total_traffic_bytes) => format!(
            "upload=0; download={}; total={}; expire={}",
            used_traffic_bytes, total_traffic_bytes, expire_timestamp
        ),
        None => format!(
            "upload=0; download={}; expire={}",
            used_traffic_bytes, expire_timestamp
        ),
    };

    // ===================================================================
    // client autodetection or raw config mode
    // ===================================================================
    let mut selected_client = params.client.clone();

    // Autodetect if client is not specified
    if selected_client.is_none() {
        let detected = state
            .subscription_service
            .detect_client_type(user_agent.as_deref());
        if detected != "html" {
            selected_client = Some(detected);
        }
    }

    // If still no client (or it's explicitly "html" detected), serve HTML
    if selected_client.is_none() {
        // Use already fetched plan_details
        let plan_name = plan_details;

        // Страница считает по тому же потолку в байтах, что и заголовок выше:
        // проценты и подпись должны сходиться с причиной блокировки, иначе
        // отключённый бесплатник видит «2% из 10 ГБ» и решает, что сервис сломан.
        let used_bytes = used_traffic_bytes as f64;
        let traffic_pct = match enforced_limit_bytes {
            Some(limit) if limit > 0 => ((used_bytes / limit as f64) * 100.0).min(100.0) as i32,
            _ => 0,
        };
        let days_left = (sub.expires_at - chrono::Utc::now()).num_days().max(0);
        let duration_days = (sub.expires_at - sub.created_at).num_days();

        // Build base URL for config links
        let panel_url_setting = state.settings.get_or_default("panel_url", "").await;
        let base_url = if !sub_domain.is_empty() {
            if sub_domain.starts_with("http") {
                sub_domain.clone()
            } else {
                format!("https://{}", sub_domain)
            }
        } else if !panel_url_setting.is_empty() {
            if panel_url_setting.starts_with("http") {
                panel_url_setting.clone()
            } else {
                format!("https://{}", panel_url_setting)
            }
        } else {
            let panel = std::env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string());
            if panel.starts_with("http") {
                panel
            } else {
                format!("https://{}", panel)
            }
        };
        let sub_url = format!("{}/sub/{}", base_url, uuid);

        let expires_display = if duration_days == 0 {
            "No expiration (Traffic Plan)".to_string()
        } else {
            format!(
                "{} ({} days left)",
                sub.expires_at.format("%Y-%m-%d"),
                days_left
            )
        };

        // На суточном плане и расход, и потолок — мегабайты за сутки: писать их
        // в гигабайтах («0.02 GB / 0.20 GB») бесполезно.
        const BYTES_IN_GB: f64 = 1024.0 * 1024.0 * 1024.0;
        const BYTES_IN_MB: f64 = 1024.0 * 1024.0;
        let traffic_display = match enforced_limit_bytes {
            Some(limit) if is_free => format!(
                "{:.1} MB / {:.1} MB today",
                used_bytes / BYTES_IN_MB,
                limit as f64 / BYTES_IN_MB
            ),
            Some(limit) => format!(
                "{:.2} GB / {:.2} GB",
                used_bytes / BYTES_IN_GB,
                limit as f64 / BYTES_IN_GB
            ),
            None => format!("{:.2} GB / ∞", used_bytes / BYTES_IN_GB),
        };

        let html = format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>CARAMBA — Subscription</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap');
body{{
  font-family:'Inter',system-ui,sans-serif;
  background:#0D0D1A;
  color:#E8E8F0;
  min-height:100vh;
  display:flex;
  justify-content:center;
  padding:24px 16px;
}}
.container{{max-width:460px;width:100%}}
.logo{{text-align:center;margin-bottom:32px}}
.logo h1{{
  font-size:28px;font-weight:800;
  background:linear-gradient(135deg,#7C3AED 0%,#3B82F6 50%,#06B6D4 100%);
  -webkit-background-clip:text;-webkit-text-fill-color:transparent;
}}
.logo p{{color:rgba(255,255,255,0.4);font-size:13px;margin-top:4px}}
.card{{
  background:rgba(255,255,255,0.06);
  border:1px solid rgba(255,255,255,0.08);
  border-radius:16px;
  padding:20px;
  margin-bottom:16px;
  backdrop-filter:blur(20px);
}}
.plan-name{{font-size:20px;font-weight:700}}
.badge{{
  display:inline-block;
  padding:4px 12px;border-radius:20px;
  font-size:11px;font-weight:600;text-transform:uppercase;
}}
.badge-active{{background:rgba(16,185,129,0.15);color:#10B981}}
.header-row{{display:flex;align-items:center;justify-content:space-between;margin-bottom:16px}}
.stat-row{{display:flex;justify-content:space-between;font-size:13px;color:rgba(255,255,255,0.6);margin-bottom:8px}}
.progress{{height:6px;background:rgba(255,255,255,0.06);border-radius:3px;overflow:hidden;margin:8px 0 16px}}
.progress-fill{{height:100%;border-radius:3px;background:linear-gradient(90deg,#7C3AED,#3B82F6)}}
.section-label{{font-size:11px;text-transform:uppercase;letter-spacing:1px;color:rgba(255,255,255,0.3);margin-bottom:12px}}
.config-grid{{display:flex;flex-direction:column;gap:10px}}
.config-btn{{
  display:flex;align-items:center;gap:12px;
  background:rgba(255,255,255,0.04);
  border:1px solid rgba(255,255,255,0.08);
  border-radius:12px;padding:14px 16px;
  color:#E8E8F0;font-size:14px;font-weight:500;
  cursor:pointer;text-decoration:none;
  transition:all 0.2s;
}}
.config-btn:hover{{background:rgba(255,255,255,0.08);border-color:rgba(124,58,237,0.3)}}
.config-btn .icon{{font-size:20px;width:32px;text-align:center}}
.config-btn .label{{flex:1}}
.config-btn .dl{{color:rgba(255,255,255,0.3);font-size:12px}}
.copy-section{{margin-top:16px}}
.link-input{{
  width:100%;padding:12px 14px;
  background:rgba(255,255,255,0.04);
  border:1px solid rgba(255,255,255,0.08);
  border-radius:10px;
  color:#E8E8F0;font-family:'SF Mono','Fira Code',monospace;
  font-size:11px;outline:none;
}}
.link-input:focus{{border-color:rgba(124,58,237,0.4)}}
.copy-btn{{
  width:100%;margin-top:10px;padding:14px;
  background:linear-gradient(135deg,#7C3AED 0%,#3B82F6 100%);
  border:none;border-radius:12px;
  color:white;font-size:14px;font-weight:600;
  cursor:pointer;transition:opacity 0.2s;
}}
.copy-btn:active{{opacity:0.8}}
.copy-btn.copied{{background:linear-gradient(135deg,#10B981 0%,#059669 100%)}}
.qr-wrap{{
  display:flex;justify-content:center;
  margin:16px 0;
  padding:16px;background:white;border-radius:12px;
}}
.footer{{text-align:center;margin-top:24px;font-size:11px;color:rgba(255,255,255,0.2)}}
</style>
</head>
<body>
<div class="container">
  <div class="logo">
    <h1>🚀 CARAMBA</h1>
    <p>Your VPN Subscription</p>
  </div>

  <div class="card">
    <div class="header-row">
      <span class="plan-name">{plan_name}</span>
      <span class="badge badge-active">✅ Active</span>
    </div>
    <div class="stat-row"><span>📊 Traffic</span><span>{traffic_display}</span></div>
    {progress_bar}
    <div class="stat-row"><span>⏳ Expires</span><span>{expires_display}</span></div>
  </div>

  <div class="card">
    <div class="section-label">Download Config</div>
    <div class="config-grid">
      <a href="{sub_url}?client=singbox" class="config-btn">
        <span class="icon">📦</span>
        <span class="label">Sing-box / Hiddify</span>
        <span class="dl">JSON →</span>
      </a>
      <a href="{sub_url}?client=v2ray" class="config-btn">
        <span class="icon">⚡</span>
        <span class="label">V2Ray / Xray</span>
        <span class="dl">Base64 →</span>
      </a>
      <a href="{sub_url}?client=clash" class="config-btn">
        <span class="icon">🔥</span>
        <span class="label">Clash / Clash Meta</span>
        <span class="dl">YAML →</span>
      </a>
    </div>
  </div>

  <div class="card">
    <div class="section-label">Subscription Link</div>
    <div class="qr-wrap">
      <img src="https://api.qrserver.com/v1/create-qr-code/?size=180x180&data={sub_url_encoded}" width="180" height="180" alt="QR Code" />
    </div>
    <div class="copy-section">
      <input type="text" class="link-input" id="subLink" value="{sub_url}" readonly onclick="this.select()" />
      <button class="copy-btn" id="copyBtn" onclick="copyLink()">📋 Copy Link</button>
    </div>
  </div>

  <div class="footer">CARAMBA VPN Panel · Powered by Xray</div>
</div>
<script>
function copyLink(){{
  const btn=document.getElementById('copyBtn');
  const input=document.getElementById('subLink');
  navigator.clipboard.writeText(input.value).then(()=>{{
    btn.textContent='✓ Copied!';
    btn.classList.add('copied');
    setTimeout(()=>{{btn.textContent='📋 Copy Link';btn.classList.remove('copied')}},2000);
  }});
}}
</script>
</body>
</html>"##,
            plan_name = plan_name.0,
            traffic_display = traffic_display,
            expires_display = expires_display,
            sub_url = sub_url,
            sub_url_encoded = urlencoding::encode(&sub_url),
            progress_bar = if enforced_limit_bytes.is_some() {
                format!(
                    r#"<div class="progress"><div class="progress-fill" style="width:{}%"></div></div>"#,
                    traffic_pct
                )
            } else {
                String::new()
            },
        );

        return (
            [
                (header::CONTENT_TYPE, "text/html"),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    user_info_header.as_str(),
                ),
                (header::HeaderName::from_static("profile-title"), "CARAMBA"),
            ],
            html,
        )
            .into_response();
    }

    // ===================================================================
    // Raw config mode: ?client=clash|v2ray|singbox
    // ===================================================================

    // 5. Get user keys
    let user_keys = match state.subscription_service.get_user_keys(&sub).await {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to get user keys for sub {}: {}", uuid, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Fetch and filter nodes (Refactored Phase 1.8: Use Plan Groups)
    // Fallback to all active nodes if plan bindings are temporarily missing.
    let nodes_raw = match state.store_service.get_user_nodes(sub.user_id).await {
        Ok(nodes) if !nodes.is_empty() => nodes,
        Ok(_) => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
            }
        },
        Err(_) => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
            }
        },
    };
    if nodes_raw.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
    }

    // `?node_id=` is the ONLY thing that narrows the body. The stored pin
    // (`sub.node_id`) no longer does.
    //
    // What the old `params.node_id.or(sub.node_id)` protected, and why each
    // reason has run out:
    //
    //  * Response size. The auto-narrow below it was commented "prevents dumping
    //    40+ outbounds"; the real fleet is 3 nodes / 15 enabled inbounds, of
    //    which 13 render as Clash proxies (~5 KB). The Go core's own ceiling is
    //    `subscription.MaxProfileBytes` = 4 MiB. Nothing was being protected.
    //  * "One subscription = one server" for clients that cannot add a query
    //    parameter (Happ, Hiddify, a bare URL in any Clash client). This one WAS
    //    real, and giving it up is the deliberate cost of this change: for those
    //    clients the pin stops steering and becomes a preference the picker in
    //    their own app overrides. See the ordering hint below for what is left
    //    of it, and the `/servers` + selection endpoints for where the choice
    //    now lives.
    //
    // It also hid a fleet from every client at once, which is the bug being
    // fixed: a subscription whose plan permits three nodes served exactly one,
    // so the app's server picker had one row and its protocol picker showed the
    // inbounds of that single machine as if they were servers.
    //
    // One failure mode disappears for free: a pin pointing at a node that has
    // since left the plan group used to filter the list down to nothing and
    // 404 the whole subscription. Only an explicit `?node_id=` can do that now,
    // and there a 404 is the honest answer — the caller asked for one node.
    let requested_node_id = params.node_id;

    let mut filtered_nodes = filter_nodes_for_subscription(nodes_raw, requested_node_id, |n| n.id);

    // Always remove pure relay infrastructure nodes – they are not
    // user-facing destinations, only inter-node transport hops.
    filtered_nodes.retain(|n| !n.is_relay);

    if filtered_nodes.is_empty() {
        // Two distinct causes share this status, so the body has to separate
        // them: a client that asked for a node it may not have needs a
        // different next step than a plan whose whole node group is relays.
        let reason = match requested_node_id {
            Some(nid) => format!(
                "Requested server {} is not an exit node available to this subscription",
                nid
            ),
            None => "No exit servers available to this subscription".to_string(),
        };
        return (StatusCode::NOT_FOUND, reason).into_response();
    }

    if requested_node_id.is_none() {
        promote_pinned_node(&mut filtered_nodes, sub.node_id, |n| n.id);
    }

    // Persist last explicitly selected node so UI/miniapp can show where the user
    // last pulled config from. Write-on-GET only: it must never overwrite a stored
    // choice, so it is doubly gated (app marker + `node_id IS NULL`).
    //
    // The write survives the change above, but its meaning narrowed with it: the
    // column is now "what the user last picked", read by the app and the mini-app
    // for display and by the ordering hint, and it no longer decides what a
    // parameter-less fetch receives.
    if let Some(selected_node_id) = requested_node_id
        && filtered_nodes.iter().any(|n| n.id == selected_node_id)
        && !app_owns_selection(&state, sub.id).await
    {
        persist_node_if_unset(&state, sub.id, selected_node_id).await;
    }

    let node_infos = match state
        .subscription_service
        .get_node_infos_with_relays(&filtered_nodes)
        .await
    {
        Ok(infos) => infos,
        Err(e) => {
            error!("Failed to generate node infos: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to process nodes").into_response();
        }
    };

    // Нормализуем тип клиента: "hiddify" — это псевдоним singbox.
    let client_type = match selected_client.as_deref().unwrap_or("singbox") {
        "hiddify" => "singbox",
        other => other,
    };

    // Determine client country: header from reverse proxy > GeoIP service.
    // Needed for: 1) relay geo-filtering, 2) cache key (different countries = different configs).
    let client_cc: Option<String> = match &client_country_header {
        Some(cc) => Some(cc.clone()),
        None => state
            .geo_service
            .get_location(&client_ip)
            .await
            .map(|geo| geo.country_code.to_uppercase()),
    };
    if client_cc.is_none() {
        warn!(
            "GeoIP lookup failed for client_ip={}, country_header={:?} — relay filtering will include all relays",
            client_ip, client_country_header
        );
    } else {
        tracing::debug!(
            "Subscription geo: client_ip={}, country={}",
            client_ip,
            client_cc.as_deref().unwrap_or("?")
        );
    }

    // Relay selection priority:
    // 1. Explicit relay_country URL param (from TMA picker)
    // 2. Persisted relay_country in subscription DB record (app / TMA choice)
    // 3. Auto-detected client country via GeoIP
    // 4. Fallback: include all relays
    // Resolved before the cache key on purpose: the key must move the moment the
    // stored choice changes, otherwise a `PUT .../selection` would be invisible
    // for the lifetime of the cached entry on a URL that carries no parameters.
    let effective_relay = params
        .relay_country
        .clone()
        .or_else(|| sub.relay_country.clone());

    // Keyed on the FILTER, not on any stored value: `?node_id=1` and a bare URL
    // now produce different bodies for the same subscription, so they must not
    // share an entry. The pin joins the key too — it no longer selects nodes,
    // but it still orders them, and an entry cached before a `PUT .../selection`
    // would otherwise keep serving the old order for the rest of the TTL.
    let cache_node_id = requested_node_id.unwrap_or(0);
    let cache_pin = sub.node_id.unwrap_or(0);
    let cache_variant = params.variant.as_deref().unwrap_or("default");
    let cache_cc = client_cc.as_deref().unwrap_or("XX");
    let cache_relay = effective_relay
        .as_deref()
        .map(|r| r.to_ascii_uppercase())
        .unwrap_or_else(|| "auto".to_string());
    let cache_key = format!(
        "sub_config_v6:{}:{}:{}:{}:{}:{}:{}",
        uuid, client_type, cache_node_id, cache_pin, cache_variant, cache_cc, cache_relay
    );

    if let Ok(Some(cached_config)) = state.redis.get(&cache_key).await {
        let content_type = match client_type {
            "clash" => "text/yaml; charset=utf-8",
            "v2ray" => "text/plain; charset=utf-8",
            _ => "application/json; charset=utf-8",
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    user_info_header.as_str(),
                ),
                (
                    header::HeaderName::from_static("profile-title"),
                    plan_details.0.as_str(),
                ),
                (
                    header::HeaderName::from_static("profile-update-interval"),
                    "2",
                ),
            ],
            cached_config,
        )
            .into_response();
    }

    // Fetch relay nodes for auto-relay chain generation.
    // Only include relays whose country matches the client's geo — the user
    // should only see relay paths through their own country (e.g. a Russian
    // user gets `via 🇷🇺` chains, not `via 🇺🇸`).
    let all_relay_nodes = state
        .subscription_service
        .get_all_active_relay_infos()
        .await
        .unwrap_or_default();

    // Persist the relay choice carried by the URL param.
    //
    // Two clients write this column and they are not symmetric. The app has a
    // real writer — `PUT /api/v2/app/subscriptions/{id}/selection` — and taking
    // it sets the Redis ownership marker; from then on no URL parameter touches
    // the column, so a months-old baked-in `?relay_country=` cannot undo a pick
    // made in the app. The mini-app has NO writer: baking the value into the
    // subscription URL is the entire mechanism by which its picker reaches the
    // database, so for a subscription the app has never claimed, the parameter
    // IS the user's choice and the latest one must win.
    //
    // Between two URL-only clients the request itself carries nothing that
    // separates "the user just picked this" from "this device is replaying a
    // URL it was handed in June" — no timestamp, no nonce, and the column has
    // no `updated_at` to compare against. So last-write-wins is chosen
    // deliberately: it loses relay stability when two devices hold different
    // stale URLs (each fetch re-pins its own), and it is the only option that
    // keeps the picker working at all. Freezing instead loses the picker for
    // 100% of existing subscribers, which is strictly worse.
    if let Some(rc) = &params.relay_country
        && !app_owns_selection(&state, sub.id).await
    {
        persist_relay_from_url(&state, sub.id, rc).await;
    }

    let relay_filter_cc: Option<String> = match effective_relay.as_deref() {
        // Case-insensitive: the app normalises to "none", the TMA has historically
        // sent "NONE", and a mixed-case value must not fall through to geo-auto.
        Some(v) if v.eq_ignore_ascii_case("none") => Some("NONE".to_string()),
        Some(cc) if cc.len() == 2 => Some(cc.to_uppercase()),
        _ => client_cc.clone(),
    };

    let relay_nodes: Vec<_> = match relay_filter_cc.as_deref() {
        Some("NONE") => vec![], // No relays
        Some(cc) => all_relay_nodes
            .into_iter()
            .filter(|r| {
                r.country_code
                    .as_ref()
                    .map(|rc| rc.eq_ignore_ascii_case(cc))
                    .unwrap_or(false)
            })
            .collect(),
        // Unknown geo, no explicit choice — include all relays as fallback.
        None => all_relay_nodes,
    };

    let (content, content_type, _filename): (String, &'static str, &'static str) = match client_type
    {
        "clash" => {
            match state.subscription_service.generate_clash(
                &sub,
                &node_infos,
                &user_keys,
                &relay_nodes,
            ) {
                Ok(c) => (c, "text/yaml; charset=utf-8", "config.yaml"),
                Err(e) => {
                    error!("Clash gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed")
                        .into_response();
                }
            }
        }
        "v2ray" => {
            match state.subscription_service.generate_v2ray(
                &sub,
                &node_infos,
                &user_keys,
                &relay_nodes,
            ) {
                Ok(c) => (c, "text/plain; charset=utf-8", "config.txt"),
                Err(e) => {
                    error!("V2Ray gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed")
                        .into_response();
                }
            }
        }
        _ => {
            match state.subscription_service.generate_singbox(
                &sub,
                &node_infos,
                &user_keys,
                params.variant.as_deref(),
                &relay_nodes,
            ) {
                Ok(c) => (c, "application/json; charset=utf-8", "config.json"),
                Err(e) => {
                    error!("Singbox gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed")
                        .into_response();
                }
            }
        }
    };

    // Cache
    let _ = state.redis.set(&cache_key, &content, 60).await; // 1 min cache

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::HeaderName::from_static("subscription-userinfo"),
                user_info_header.as_str(),
            ),
            (
                header::HeaderName::from_static("profile-title"),
                plan_details.0.as_str(),
            ),
            (
                header::HeaderName::from_static("profile-update-interval"),
                "2",
            ),
        ],
        content,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{filter_nodes_for_subscription, normalize_relay_param, promote_pinned_node};

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestNode {
        id: i64,
    }

    /// Только две формы — «релей страны XX» и «без релея» — являются выбором.
    /// Всё остальное приходит от клиента как артефакт и в колонку попадать не
    /// должно: в проде уже лежит строка `relay_country = ''`, записанная тогда,
    /// когда этот путь хранил всё подряд.
    #[test]
    fn only_a_country_or_an_explicit_none_is_a_relay_choice() {
        assert_eq!(normalize_relay_param("ru").as_deref(), Some("RU"));
        assert_eq!(normalize_relay_param("RU").as_deref(), Some("RU"));
        assert_eq!(normalize_relay_param(" nl ").as_deref(), Some("NL"));
        // "none" канонизируется в нижний регистр — именно так его пишет
        // PUT /selection (normalize_relay_country), и read-side сравнивает
        // без учёта регистра в обоих случаях.
        assert_eq!(normalize_relay_param("none").as_deref(), Some("none"));
        assert_eq!(normalize_relay_param("NONE").as_deref(), Some("none"));

        for junk in ["", "   ", "auto", "rus", "r", "1", "р у"] {
            assert!(
                normalize_relay_param(junk).is_none(),
                "{junk:?} — не выбор пользователя, писать нечего"
            );
        }
    }

    #[test]
    fn keeps_full_node_set_when_subscription_has_last_selected_node() {
        let nodes = vec![TestNode { id: 1 }, TestNode { id: 2 }, TestNode { id: 3 }];

        let filtered = filter_nodes_for_subscription(nodes.clone(), None, |node| node.id);

        assert_eq!(filtered, nodes);
    }

    #[test]
    fn scopes_nodes_only_when_request_explicitly_targets_node() {
        let nodes = vec![TestNode { id: 1 }, TestNode { id: 2 }, TestNode { id: 3 }];

        let filtered = filter_nodes_for_subscription(nodes, Some(2), |node| node.id);

        assert_eq!(filtered, vec![TestNode { id: 2 }]);
    }

    /// Закреплённый узел поднимается наверх и НИ ОДИН не выпадает. Это и есть
    /// граница между «подсказкой порядка» и прежним поведением, где пин
    /// вырезал весь остальной флот из тела подписки.
    #[test]
    fn a_pin_reorders_the_fleet_and_never_shortens_it() {
        let mut nodes = vec![TestNode { id: 1 }, TestNode { id: 2 }, TestNode { id: 5 }];

        promote_pinned_node(&mut nodes, Some(5), |node| node.id);

        assert_eq!(
            nodes,
            vec![TestNode { id: 5 }, TestNode { id: 1 }, TestNode { id: 2 }]
        );
    }

    /// Пин на узел, которого в списке нет (выведен из группы плана, либо это
    /// relay и его уже отфильтровали), — это не ошибка и не повод отдать
    /// пустое тело: недостижимое предпочтение просто игнорируется.
    #[test]
    fn an_unreachable_pin_costs_nothing() {
        let original = vec![TestNode { id: 1 }, TestNode { id: 5 }];

        let mut nodes = original.clone();
        promote_pinned_node(&mut nodes, Some(42), |node| node.id);
        assert_eq!(nodes, original);

        let mut nodes = original.clone();
        promote_pinned_node(&mut nodes, None, |node| node.id);
        assert_eq!(nodes, original);
    }
}
