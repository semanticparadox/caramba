//! JWT-защищённые эндпоинты standalone-приложения.
//!
//! Все хендлеры здесь требуют валидный access-токен (middleware
//! `app_auth::require_app_jwt` кладёт `AuthUser` в extensions). Отдаём профиль,
//! данные подписки (включая готовый URL mihomo/clash-конфига, который тянет
//! Go-ядро) и список серверов.

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Serialize;
use sqlx::Row;

/// Определяет базовый URL для ссылок подписки (subscription_domain → panel_url →
/// Host-заголовок). Логика дублирует api/client.rs::resolve_subscription_base_url,
/// но локальна, чтобы не делать ту функцию публичной.
async fn resolve_base_url(state: &AppState, headers: &HeaderMap) -> String {
    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let base_domain = if !sub_domain.is_empty() {
        sub_domain
    } else {
        let panel = state.settings.get_or_default("panel_url", "").await;
        if !panel.is_empty() {
            panel
        } else if let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) {
            host.to_string()
        } else {
            std::env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
        }
    };

    if base_domain.starts_with("http") {
        base_domain
    } else {
        let proto = if base_domain.contains("localhost") || base_domain.contains("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        format!("{}://{}", proto, base_domain)
    }
}

/// GET /api/v2/app/me — профиль пользователя: баланс, кол-во активных подписок,
/// имя текущего плана.
pub async fn get_me(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let row = sqlx::query(
        "SELECT id, tg_id, email, username, full_name, balance, referral_code, email_verified, auth_provider \
         FROM users WHERE id = $1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let r = match row {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let balance: i64 = r.try_get("balance").unwrap_or(0);

    let active_subs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscriptions WHERE user_id = $1 AND status = 'active'",
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    // Имя текущего плана (самая поздняя по сроку активная подписка).
    let plan_name: Option<String> = sqlx::query_scalar(
        "SELECT p.name FROM subscriptions s JOIN plans p ON s.plan_id = p.id \
         WHERE s.user_id = $1 AND s.status = 'active' \
         ORDER BY s.expires_at DESC LIMIT 1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    Json(serde_json::json!({
        "id": auth.user_id,
        "tg_id": r.try_get::<Option<i64>, _>("tg_id").ok().flatten(),
        "email": r.try_get::<Option<String>, _>("email").ok().flatten(),
        "username": r.try_get::<Option<String>, _>("username").ok().flatten(),
        "full_name": r.try_get::<Option<String>, _>("full_name").ok().flatten(),
        "balance": balance as f64 / 100.0,
        "balance_cents": balance,
        "referral_code": r.try_get::<Option<String>, _>("referral_code").ok().flatten(),
        "email_verified": r.try_get::<Option<bool>, _>("email_verified").ok().flatten().unwrap_or(false),
        "auth_provider": r.try_get::<Option<String>, _>("auth_provider").ok().flatten(),
        "active_subscriptions": active_subs,
        "plan_name": plan_name,
    }))
    .into_response()
}

/// GET /api/v2/app/subscription — данные подписки и готовые URL конфигов.
///
/// Возвращает subscription_uuid + набор URL для разных клиентов. Ключевой для
/// Go-ядра — `clash_url` (mihomo тянет именно его, amnezia-wg уже в конфиге).
pub async fn get_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let subs = match state
        .subscription_service
        .get_user_subscriptions(auth.user_id)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(err = %e, "app: failed to fetch subscriptions");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch subscription")
                .into_response();
        }
    };

    // Берём активную подписку, иначе первую доступную.
    let sub = subs
        .iter()
        .find(|s| s.sub.status == "active")
        .or_else(|| subs.first());

    let sub = match sub {
        Some(s) => s,
        None => return (StatusCode::NOT_FOUND, "No subscription found").into_response(),
    };

    let base_url = resolve_base_url(&state, &headers).await;
    let uuid = &sub.sub.subscription_uuid;

    let used_gb = sub.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
    let days_left = (sub.sub.expires_at - chrono::Utc::now()).num_days().max(0);

    Json(serde_json::json!({
        "id": sub.sub.id,
        "subscription_uuid": uuid,
        "plan_name": sub.plan_name,
        "status": sub.sub.status,
        "used_traffic_bytes": sub.sub.used_traffic,
        "used_traffic_gb": format!("{:.2}", used_gb),
        "traffic_limit_gb": sub.traffic_limit_gb,
        "expires_at": sub.sub.expires_at.to_rfc3339(),
        "days_left": days_left,
        // URL'ы конфигов. Go-ядро (mihomo) использует clash_url.
        "clash_url": format!("{}/sub/{}?client=clash", base_url, uuid),
        "config_url": format!("{}/sub/{}?client=clash", base_url, uuid),
        "singbox_url": format!("{}/sub/{}?client=singbox", base_url, uuid),
        "v2ray_url": format!("{}/sub/{}?client=v2ray", base_url, uuid),
        "subscription_url": format!("{}/sub/{}", base_url, uuid),
    }))
    .into_response()
}

#[derive(Serialize)]
struct AppServer {
    id: i64,
    name: String,
    country_code: Option<String>,
    flag: String,
    latency_ms: Option<i32>,
    load_pct: f64,
    status: String,
}

/// Эмодзи-флаг по ISO-2 коду страны (без unwrap на данных из БД).
fn country_flag(country: &str) -> String {
    let chars: Vec<char> = country
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    if chars.len() != 2 {
        return "🌐".to_string();
    }
    let offset = 127397u32;
    match (
        char::from_u32(chars[0] as u32 + offset),
        char::from_u32(chars[1] as u32 + offset),
    ) {
        (Some(f), Some(s)) => format!("{}{}", f, s),
        _ => "🌐".to_string(),
    }
}

/// GET /api/v2/app/servers — список доступных пользователю exit-серверов.
///
/// Переиспользует пул узлов из store_service (как api/client.rs::get_active_servers),
/// скрывает relay-инфраструктуру и перегруженные узлы.
pub async fn list_servers(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let nodes: Vec<caramba_db::models::node::Node> = state
        .store_service
        .get_user_nodes(auth.user_id)
        .await
        .unwrap_or_default();

    let servers: Vec<AppServer> = nodes
        .into_iter()
        .filter(|n| {
            // Прячем relay-узлы и перегруженные машины.
            !n.is_relay
                && n.last_cpu.unwrap_or(0.0) < 95.0
                && n.last_ram.unwrap_or(0.0) < 98.0
        })
        .map(|n| {
            let cpu = n.last_cpu.unwrap_or(0.0);
            let ram = n.last_ram.unwrap_or(0.0);
            let load = (cpu + ram) / 2.0;
            let connections = n.active_connections.unwrap_or(0);
            let is_full = n.max_users > 0 && connections >= n.max_users;
            let status = if is_full {
                "full".to_string()
            } else if cpu > 80.0 {
                "busy".to_string()
            } else {
                n.status.clone()
            };
            AppServer {
                id: n.id,
                name: format!("Node #{} ({} Mbps)", n.id, n.current_speed_mbps),
                flag: country_flag(n.country_code.as_deref().unwrap_or("US")),
                country_code: n.country_code.clone(),
                latency_ms: n.last_latency.map(|l| l as i32),
                load_pct: load,
                status,
            }
        })
        .collect();

    Json(servers).into_response()
}
