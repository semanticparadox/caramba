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

    // 'throttled' — временная суточная блокировка бесплатного плана; для
    // пользователя такая подписка всё ещё «его тариф», а не отсутствие оного.
    let active_subs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscriptions WHERE user_id = $1 AND status IN ('active', 'throttled')",
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    // Имя текущего плана (самая поздняя по сроку активная подписка).
    let plan_name: Option<String> = sqlx::query_scalar(
        "SELECT p.name FROM subscriptions s JOIN plans p ON s.plan_id = p.id \
         WHERE s.user_id = $1 AND s.status IN ('active', 'throttled') \
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch subscription",
            )
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

    // Тот же потолок, что и в энфорсменте: лимит тарифа + бонусный трафик.
    let bonus_traffic_mb = crate::services::bonus_traffic::balance_mb(&state.pool, auth.user_id)
        .await
        .unwrap_or(0);

    // `SubscriptionWithDetails` несёт из плана только traffic_limit_gb, а на
    // бесплатном плане энфорсмент считает совсем по другой колонке
    // (daily_traffic_mb). Без этих двух флагов потолок пришлось бы угадывать —
    // и раньше он угадывался как «план платный», из-за чего бесплатный
    // пользователь, задушенный на 200 МБ, видел в приложении 2% от 10 ГБ.
    let (is_free, daily_traffic_mb): (bool, i32) = sqlx::query_as(
        "SELECT COALESCE(is_free, FALSE), COALESCE(daily_traffic_mb, 0) FROM plans WHERE id = $1",
    )
    .bind(sub.sub.plan_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
    .unwrap_or((false, 0));

    let traffic_limit_gb = sub.traffic_limit_gb.unwrap_or(0);
    let traffic_limit_bytes = crate::services::bonus_traffic::plan_quota_limit_bytes(
        is_free,
        traffic_limit_gb as i64,
        daily_traffic_mb as i64,
        bonus_traffic_mb,
    );
    // Период, за который посчитаны traffic_limit_bytes и used_traffic_bytes:
    // "day" ровно тогда, когда потолок взят из суточной колонки. Приложению
    // это нужно, чтобы рисовать суточный счётчик, а не бессмысленный «всего».
    let quota_period = if is_free && traffic_limit_gb > 0 && daily_traffic_mb > 0 {
        "day"
    } else {
        "total"
    };

    Json(serde_json::json!({
        "id": sub.sub.id,
        "subscription_uuid": uuid,
        "plan_name": sub.plan_name,
        "status": sub.sub.status,
        // На суточном плане это расход, ещё не прощённый суточным пополнением
        // (monitoring::daily_traffic_topup вычитает норму с полом 0), то есть
        // фактически «сегодня»; на остальных — расход за весь срок подписки.
        // Что именно — говорит quota_period.
        "used_traffic_bytes": sub.sub.used_traffic,
        "used_traffic_gb": format!("{:.2}", used_gb),
        // Сырая колонка плана. На бесплатном плане она НЕ является потолком —
        // клиент обязан считать по traffic_limit_bytes / quota_period.
        "traffic_limit_gb": sub.traffic_limit_gb,
        "traffic_limit_bytes": traffic_limit_bytes,
        "quota_period": quota_period,
        "is_free": is_free,
        // Суточная норма плана в МБ (0 — суточной нормы нет). Отдаём сырую
        // колонку отдельно от потолка: в traffic_limit_bytes уже подмешан
        // бонусный трафик, а нарисовать «200 МБ в сутки» надо без него.
        "daily_traffic_mb": daily_traffic_mb,
        "bonus_traffic_mb": bonus_traffic_mb,
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

/// Статус узла в словаре, который понимает приложение: `online | busy | full`.
///
/// Клиент (`ExitLocation` в caramba-client) рисует `full` как «не принимает
/// подключения», а ВСЁ, что не `online` и не `busy` — как «не в сети». Раньше
/// сюда как есть проваливался `nodes.status` из БД, где у всех живых узлов
/// стоит `'active'`: этой строки в словаре нет, поэтому клиент читал каждый
/// узел как offline и список выходов был мёртв целиком. Маппинг обязан жить в
/// панели, а не в клиенте: контракт уже записан на стороне Dart, и этот
/// эндпоинт читают не только Flutter-клиенты.
///
/// Ветки на `'maintenance'` / `'disabled'` здесь намеренно нет: до этой функции
/// доезжают только узлы, прошедшие `status = 'active'` (фильтр в
/// `node_repo::{get_nodes_for_plan, get_active_nodes}` плюс его же повтор в
/// `list_servers`), так что колонка статуса на этом шаге не несёт информации —
/// её несут загрузка и вместимость.
fn server_status(is_full: bool, cpu_pct: f64) -> &'static str {
    if is_full {
        "full"
    } else if cpu_pct > 80.0 {
        "busy"
    } else {
        "online"
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
            //
            // `status == "active"` повторяет предикат, который уже применили
            // node_repo::{get_nodes_for_plan, get_active_nodes}. Повтор не
            // лишний: ниже статус узла для приложения собирается ТОЛЬКО из
            // загрузки и вместимости, и это законно ровно потому, что сюда не
            // доезжает ни один неактивный узел. Если фильтр наверху когда-нибудь
            // ослабят, узел в обслуживании просто исчезнет из списка (он и не
            // выбираем), а не притворится живым.
            n.status == "active"
                && !n.is_relay
                && n.last_cpu.unwrap_or(0.0) < 95.0
                && n.last_ram.unwrap_or(0.0) < 98.0
        })
        .map(|n| {
            let cpu = n.last_cpu.unwrap_or(0.0);
            let ram = n.last_ram.unwrap_or(0.0);
            let load = (cpu + ram) / 2.0;
            let connections = n.active_connections.unwrap_or(0);
            let is_full = n.max_users > 0 && connections >= n.max_users;
            let status = server_status(is_full, cpu).to_string();
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

#[cfg(test)]
mod tests {
    use super::server_status;

    /// Словарь фиксирован: приложение знает только online/busy/full. Ни одно
    /// значение колонки `nodes.status` («active») наружу выйти не должно —
    /// именно оно клало весь список выходов.
    #[test]
    fn the_wire_vocabulary_is_online_busy_full() {
        assert_eq!(server_status(false, 0.0), "online");
        assert_eq!(server_status(false, 80.0), "online");
        assert_eq!(server_status(false, 80.1), "busy");
        assert_eq!(server_status(false, 94.9), "busy");
        // Вместимость важнее загрузки: полный узел нельзя выбрать вообще, а
        // busy — можно.
        assert_eq!(server_status(true, 0.0), "full");
        assert_eq!(server_status(true, 99.0), "full");

        for status in [
            server_status(false, 0.0),
            server_status(false, 90.0),
            server_status(true, 0.0),
        ] {
            assert!(
                matches!(status, "online" | "busy" | "full"),
                "статус {status:?} вне словаря приложения"
            );
            assert_ne!(status, "active", "колонка БД не должна утекать на провод");
        }
    }
}
