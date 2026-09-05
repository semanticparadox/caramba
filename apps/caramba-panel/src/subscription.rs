use axum::{
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, warn};

use caramba_shared::csm::directive::{ReasonCode, Status};

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

pub(crate) fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
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

/// Страна клиента: заголовок обратного прокси важнее GeoIP.
///
/// `None` означает ровно «не знаем», а не «нигде»: MaxMind-базы может не быть,
/// внешний сервис мог не ответить, адрес мог оказаться приватным. Все решения,
/// принимаемые по этому значению, обязаны отдельно описывать ветку `None` —
/// молча подставлять страну нельзя.
pub(crate) async fn resolve_client_country(
    state: &AppState,
    client_ip: &str,
    header_cc: Option<&str>,
) -> Option<String> {
    if let Some(cc) = header_cc {
        return Some(cc.to_string());
    }
    state
        .geo_service
        .get_location(client_ip)
        .await
        .map(|geo| geo.country_code.to_uppercase())
}

/// Страны активных релеев этой установки, ISO-2 в верхнем регистре.
///
/// Кешируется на минуту в процессе: значение меняется только когда админ
/// добавляет или гасит релей, а спрашивают его на каждой выдаче подписки.
async fn active_relay_countries(state: &AppState) -> Vec<String> {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    static CACHE: OnceLock<Mutex<Option<(Vec<String>, Instant)>>> = OnceLock::new();
    let cell = CACHE.get_or_init(|| Mutex::new(None));

    if let Ok(guard) = cell.lock()
        && let Some((cached, ts)) = guard.as_ref()
        && ts.elapsed() < Duration::from_secs(60)
    {
        return cached.clone();
    }

    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT UPPER(country_code) FROM nodes \
         WHERE is_relay = TRUE AND status = 'active' AND country_code IS NOT NULL",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    if let Ok(mut guard) = cell.lock() {
        *guard = Some((rows.clone(), Instant::now()));
    }
    rows
}

/// Обслуживает ли зеркало подписки (`subscription_domain`) клиента из этой
/// страны — то есть надо ли отправлять его туда вместо панели.
///
/// Настройка `subscription_domain_countries`:
///   `relays` (умолчание) — страны, где у установки есть активный релей;
///   `*`                  — все (прежнее безусловное поведение);
///   пустая строка        — никто, тело всегда отдаёт панель;
///   `RU,BY`              — явный список ISO-2.
///
/// Неизвестная страна НЕ отправляется на зеркало (кроме режима `*`). Причина
/// не в осторожности, а в том, что известно: запрос уже дошёл до панели, то
/// есть прямой путь у этого клиента заведомо работает. Гнать его в обход по
/// догадке значит менять доказанное на предполагаемое.
///
/// Режим `relays` при отсутствии активных релеев вырождается в `*`. Установка,
/// где домен подписки задан, а релеев нет, использует его не как «вход для
/// страны», а как обычный фронт (CDN, отдельный домен), и отнимать его молча
/// нельзя.
pub(crate) async fn mirror_serves_country(state: &AppState, client_cc: Option<&str>) -> bool {
    let raw = state
        .settings
        .get_or_default("subscription_domain_countries", "relays")
        .await;
    // Список релеев спрашиваем только в режиме `relays` — в остальных он на
    // решение не влияет, а это запрос к базе.
    let relays = if raw.trim().eq_ignore_ascii_case("relays") {
        active_relay_countries(state).await
    } else {
        Vec::new()
    };
    mirror_country_decision(&raw, &relays, client_cc)
}

/// Чистая половина [`mirror_serves_country`]: всё решение без базы и настроек.
/// Вынесена ради тестов — ветки здесь такие, что ошибка в любой из них тихо
/// уводит целую страну не туда.
fn mirror_country_decision(
    mode: &str,
    relay_countries: &[String],
    client_cc: Option<&str>,
) -> bool {
    let mode = mode.trim();

    if mode == "*" {
        return true;
    }
    if mode.is_empty() {
        return false;
    }

    let list: Vec<String> = if mode.eq_ignore_ascii_case("relays") {
        if relay_countries.is_empty() {
            return true;
        }
        relay_countries.to_vec()
    } else {
        mode.split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| s.len() == 2)
            .collect()
    };

    match client_cc {
        Some(cc) => list.iter().any(|c| c.eq_ignore_ascii_case(cc)),
        None => false,
    }
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    req: Request,
) -> Response {
    // 0. Кто спрашивает. Разбор заголовков стоит ноль и поднят сюда потому, что
    // от страны клиента теперь зависит сам способ доставки (шаг 1.5).
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

    // 1.5 Страна клиента: заголовок обратного прокси важнее GeoIP.
    //
    // Считается ДО решения о доставке и переиспользуется ниже (фильтр релеев,
    // ключ кеша) — второго обращения к GeoIP в этом хендлере больше нет.
    // Порядок «сначала лимит, потом GeoIP» намеренный: промах кеша GeoIP это
    // исходящий HTTP-запрос, и он не должен выполняться чаще, чем 30 раз в
    // минуту на подписку. Побочный эффект: клиент, упёршийся в лимит, получает
    // 429 там, где раньше получал редирект. Это правильнее — редирект теперь
    // решение, а не константа, и стоит запроса к геобазе.
    let client_cc =
        resolve_client_country(&state, &client_ip, client_country_header.as_deref()).await;

    // 2. Куда отдавать тело: с панели напрямую или через зеркало подписки.
    //
    // Раньше здесь стоял безусловный 308 на `subscription_domain`, и он гнал
    // ЛЮБОГО клиента через зеркало. На этой установке зеркало — российский
    // релей, поэтому американский пользователь ходил за своим конфигом в
    // Россию и обратно в Польшу.
    //
    // Важно понять, чего редирект не умеет: чтобы его увидеть, надо СНАЧАЛА
    // дотянуться до панели. Клиенту, у которого панель заблокирована, он не
    // помогает никак — тот просто не доходит до этой строки. Значит редирект
    // никогда не покупает достижимость, он только закрепляет клиента за
    // доменом зеркала. Это ценно ровно для той страны, ради которой зеркало и
    // стоит, и является чистым крюком для всех остальных.
    //
    // Отсюда правило: зеркало получает тех, кого оно обслуживает
    // (`subscription_domain_countries`), остальные — тело прямо здесь.
    // Мёртвой ссылки не появляется ни у кого: запрос уже дошёл, а адрес
    // зеркала не изменился.
    //
    // Редирект остался ПОСТОЯННЫМ (308) сознательно. Для страны, которую
    // зеркало обслуживает, «запомни этот адрес навсегда» — не ложь, а ровно то,
    // что нужно: клиент переписывает у себя ссылку на домен, который переживёт
    // блокировку панельного. Цена известна и названа: россиянин, переехавший в
    // США, останется с закешированным крюком, пока не обновит подписку заново.
    //
    // Порядок проверок важен для стоимости: сравнение хостов бесплатно, а
    // решение о стране может стоить запроса к базе. Запрос, ПРИШЕДШИЙ с
    // зеркала, узнаётся по Host и не платит ничего — а это весь российский
    // трафик: caramba-sub обращается к панели с `Host: app.exarobot.top`
    // (FRONTEND_DOMAIN), и для него условие ниже ложно всегда.
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

        if host_clean != sub_domain_clean
            && mirror_serves_country(&state, client_cc.as_deref()).await
        {
            let proto = "https";
            let query = req
                .uri()
                .query()
                .map(|q| format!("?{}", q))
                .unwrap_or_default();
            let full_url = format!("{}://{}/sub/{}{}", proto, sub_domain, uuid, query);
            tracing::info!(
                "Subscription delivery: mirror, client_ip={}, country={}, to={}",
                client_ip,
                client_cc.as_deref().unwrap_or("unknown"),
                sub_domain_clean
            );
            return axum::response::Redirect::permanent(&full_url).into_response();
        }
    }

    // 3. Get subscription
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

    // 3.1 Факты плана и расхода — ДО первого отказа, а не после него.
    //
    // Раньше весь этот блок стоял в шаге 4.5, то есть за всеми тремя
    // проверками, и отказ уходил клиенту раньше, чем панель успевала посчитать,
    // сколько трафика израсходовано и из какого потолка. Отсюда и брался
    // единственный сигнал отказа — код 403: цифр в тот момент ещё не
    // существовало. Теперь они считаются первыми и едут вместе с отказом.
    //
    // Цена перестановки: подписка, которой откажут, платит те же три запроса,
    // что и успешная. Путь ограничен 30 запросами в минуту на UUID (шаг 1), так
    // что верхняя граница известна и мала, а покупается на неё ровно то, ради
    // чего всё это делается.
    //
    // Один запрос к `plans` вместо прежней пары «get_user_subscriptions +
    // SELECT is_free»: имя тарифа и потолок лежат в одной строке, а прежний
    // вызов тянул ВСЕ подписки пользователя с двумя JOIN'ами ради двух полей.
    let plan_facts: Option<(String, i32, bool, i32, i32)> = sqlx::query_as(
        "SELECT COALESCE(name, 'VPN Plan'), COALESCE(traffic_limit_gb, 0), \
         COALESCE(is_free, FALSE), COALESCE(daily_traffic_mb, 0), COALESCE(device_limit, 0) \
         FROM plans WHERE id = $1",
    )
    .bind(sub.plan_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);
    let (plan_name, traffic_limit_gb, is_free, daily_traffic_mb, plan_device_limit) =
        plan_facts.unwrap_or_else(|| ("VPN Plan".to_string(), 0, false, 0, 0));

    let banned: bool =
        sqlx::query_scalar("SELECT COALESCE(is_banned, FALSE) FROM users WHERE id = $1")
            .bind(sub.user_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
            .unwrap_or(false);

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
    // вместе с ним (миграция 20260831120000), — но Subscription-Userinfo уходит
    // клиенту, и отрицательный download в нём нестандартен в любом случае.
    let used_traffic_bytes = (sub.used_traffic as i64).max(0);
    let expire_timestamp = sub.expires_at.timestamp();

    // Заготовка фактов доступа. `device_used` дозаполняется на шаге 3.5 — до
    // него число активных адресов ещё не прочитано, а врать нулём в ответе,
    // который про устройства и не говорит, дешевле, чем делать лишний запрос
    // на каждом отказе по совершенно другой причине.
    let mut access_facts = access::AccessFacts {
        status: sub.status.clone(),
        banned,
        expires_at: expire_timestamp,
        used_bytes: used_traffic_bytes,
        limit_bytes: enforced_limit_bytes,
        is_free,
        daily_traffic_mb: daily_traffic_mb as i64,
        device_used: 0,
        device_limit: plan_device_limit as i64,
        now: chrono::Utc::now().timestamp(),
    };

    // Тот же заголовок расхода, что уезжает с успешным конфигом. На отказе он
    // важнее, чем на успехе: для клиента за зеркалом подписки это единственная
    // цифра, которая доедет (caramba-sub копирует ровно три заголовка, и
    // `subscription-userinfo` — один из них).
    let userinfo =
        access::userinfo_header(used_traffic_bytes, enforced_limit_bytes, expire_timestamp);

    // 3.2 Статус подписки. Тело и код те же, что были всегда; новое — заголовки.
    if sub.status != "active" {
        let a = access::compute(&access_facts, None);
        return access::refusal_response(
            StatusCode::FORBIDDEN,
            "Subscription inactive or expired",
            &a,
            Some(userinfo),
        );
    }

    // 3.3 Проверка квоты на самом запросе конфига — энфорсмент в реальном
    // времени, а не раз в десять минут свипом.
    match state
        .subscription_service
        .ensure_subscription_within_quota(sub.id)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            // Исход навязан, а не выведен из строки: `ensure_...` только что
            // перевёл её в 'throttled'/'expired', и наш снимок `sub` этого
            // перевода уже не видит. Спрашивать базу второй раз ради статуса,
            // который мы и так знаем, незачем.
            let a = access::compute_as(
                &access_facts,
                Some((Status::QuotaExceeded, access::quota_reason(&access_facts))),
                None,
            );
            return access::refusal_response(
                StatusCode::FORBIDDEN,
                "Traffic limit reached. Subscription is expired.",
                &a,
                Some(userinfo),
            );
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

            // Лимит устройств — свойство ЗАПРОСА, а не строки подписки: сама
            // подписка в этот момент исправна, и вывести отказ из её статуса
            // невозможно. Поэтому исход навязывается явно — только этот путь
            // его и видит.
            access_facts.device_used = active_ips.len() as i64;
            access_facts.device_limit = device_limit as i64;
            let a = access::compute_as(
                &access_facts,
                Some((Status::DeviceLimit, ReasonCode::DEVICE_LIMIT_REACHED)),
                None,
            );
            return access::refusal_response(
                StatusCode::FORBIDDEN,
                "Device limit reached",
                &a,
                Some(userinfo),
            );
        }
    }

    // 4. Update access tracking
    let _ = state
        .subscription_service
        .track_access(sub.id, &client_ip, user_agent.as_deref())
        .await;

    // 4.5 Заголовок расхода для Hiddify/sing-box.
    //
    // Считать здесь больше нечего: тариф, потолок и расход посчитаны на шаге
    // 3.1, до первого отказа. Одна и та же строка уезжает и с успешным
    // конфигом, и с отказом — иначе клиент видел бы цифры только тогда, когда
    // они ему не нужны.
    //
    // upload=0; download=used; total=limit; expire=timestamp. Безлимит отдаём
    // без `total`, чтобы клиент нарисовал ∞. На суточном плане `total` — это
    // норма на сутки: клиент покажет расход из 200 МБ, что совпадает с тем, за
    // что его отключат, а не из 10 ГБ.
    let user_info_header = userinfo;

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
            plan_name = plan_name,
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

    // «Серверов нет» — это отказ ПАРКА, а не подписки, и клиент обязан уметь
    // отличить его от исчерпанного трафика: в первом случае надо подождать, во
    // втором — заплатить. Без кода причины оба приезжали одинаковым «не удалось
    // загрузить подписку», и подсказать человеку было нечего.
    let fleet_unavailable = || {
        let a = access::compute_as(
            &access_facts,
            Some((Status::Suspended, ReasonCode::FLEET_UNAVAILABLE)),
            None,
        );
        access::refusal_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "No servers available",
            &a,
            None,
        )
    };

    // Fetch and filter nodes (Refactored Phase 1.8: Use Plan Groups)
    // Fallback to all active nodes if plan bindings are temporarily missing.
    let nodes_raw = match state.store_service.get_user_nodes(sub.user_id).await {
        Ok(nodes) if !nodes.is_empty() => nodes,
        Ok(_) => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return fleet_unavailable();
            }
        },
        Err(_) => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return fleet_unavailable();
            }
        },
    };
    if nodes_raw.is_empty() {
        return fleet_unavailable();
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
        // Тело динамическое (в него подставлен номер узла), поэтому здесь
        // используется не `refusal_response`, а те же заголовки поверх готового
        // ответа: тело остаётся ровно тем, что было, а причина становится
        // читаемой. Для клиента это тот же «парк недоступен» — просто в срезе
        // одной подписки.
        let a = access::compute_as(
            &access_facts,
            Some((Status::Suspended, ReasonCode::FLEET_UNAVAILABLE)),
            None,
        );
        return (StatusCode::NOT_FOUND, access::refusal_headers(&a), reason).into_response();
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

    // Страна клиента уже определена на шаге 1.5 — она понадобилась там, чтобы
    // выбрать способ доставки. Здесь её только используют: фильтр релеев,
    // ключ кеша (разные страны — разные тела) и заголовок ответа.
    let client_country_header_value = client_cc.clone().unwrap_or_else(|| "unknown".to_string());
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
                    plan_name.as_str(),
                ),
                (
                    header::HeaderName::from_static("profile-update-interval"),
                    "2",
                ),
                // Страна, которую панель увидела у этого клиента. Клиент сам её не
                // знает — ни ядро, ни приложение геобазы не носят, — а от неё
                // зависит выбор пресета и домашнего резолвера. Отдаём то, что
                // есть, включая честное "unknown": подставлять страну по догадке
                // здесь значит увести пользователя в чужой национальный режим.
                (
                    header::HeaderName::from_static("x-client-country"),
                    client_country_header_value.as_str(),
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
                plan_name.as_str(),
            ),
            (
                header::HeaderName::from_static("profile-update-interval"),
                "2",
            ),
            // Страна, которую панель увидела у этого клиента. Клиент сам её не
            // знает — ни ядро, ни приложение геобазы не носят, — а от неё
            // зависит выбор пресета и домашнего резолвера. Отдаём то, что
            // есть, включая честное "unknown": подставлять страну по догадке
            // здесь значит увести пользователя в чужой национальный режим.
            (
                header::HeaderName::from_static("x-client-country"),
                client_country_header_value.as_str(),
            ),
        ],
        content,
    )
        .into_response()
}

// ============================================================================
// Причина отказа как данные, а не как проза
// ============================================================================

/// Почему панель отказала и что с этим делать — в машинном виде.
///
/// # Зачем модуль вообще существует
///
/// До него отказ выражался ровно одним числом — HTTP-кодом — и одной английской
/// строкой в теле. Три разные жизненные ситуации (подписка не активна, кончился
/// трафик, занято устройство) отдавали 403 и текст, который клиент не разбирает.
/// Go-ядро на любом не-200 выбрасывало и заголовки, и тело
/// (`transport/ladder.go`, «код состояния %d»), четыре слоя обёрток дописывали
/// свои префиксы, и до человека доезжало «transport: код состояния 403» — фраза,
/// по которой нельзя понять ни причины, ни что нажать.
///
/// Дальше это чинили догадками: сначала подозревали протухший токен, потом
/// российский прокси, потом блокировку. Настоящей причиной было «на бесплатном
/// тарифе кончились 200 МБ за сегодня» — факт, который панель знала всё это
/// время и не сказала.
///
/// Поэтому здесь считается ОДНО состояние доступа, и его отдают все три пути:
/// JSON приложения (`api/v2/app.rs`, `api/v2/app_account.rs`) и заголовки
/// отказа на `/sub/{uuid}`. Один источник — единственная защита от того, чтобы
/// экран приложения и заголовок ответа рассказывали про одного пользователя
/// разные истории.
///
/// # Что здесь принципиально не делается
///
/// Не меняется НИ ОДИН код состояния и НИ ОДНО тело ответа на `/sub/{uuid}`.
/// Happ, Hiddify и clash-клиенты читают «конфиг или не-200 с текстом», и
/// `06-MIGRATION.md` 6.1 фиксирует эти тела побайтово. Всё новое едет
/// заголовками, которых старый клиент не запрашивал и потому игнорирует.
///
/// Слово `throttled` наружу не выходит. Оно остаётся в базе и в логах, где оно
/// и означает то, что означает; клиенту едет `quota_exceeded` + `rc` 3003 и
/// русский текст без единого внутреннего термина.
pub mod access {
    use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
    use axum::response::{IntoResponse, Response};
    use caramba_shared::csm::directive::{ReasonCode, Status};
    use serde::Serialize;

    /// Насколько поздно может приехать суточное пополнение относительно 00:00 UTC.
    ///
    /// `monitoring::start` тикает раз в 30 секунд и запускает
    /// `daily_traffic_topup` каждые 60 тиков — то есть раз в полчаса, а не в
    /// полночь. Пользователю нельзя обещать «ровно в 00:00»: он проверит в
    /// 00:05, увидит тот же отказ и решит, что его обманули. Обещаем окно.
    pub const RESET_LAG_SECONDS: i64 = 1800;

    /// Факты, от которых зависит состояние доступа. Собираются вызывающим из
    /// строки подписки и колонок плана; сама функция в базу не ходит и потому
    /// проверяема целиком.
    #[derive(Debug, Clone)]
    pub struct AccessFacts {
        /// Сырой статус из базы (`active` | `throttled` | `expired` | …).
        /// Наружу он не попадает — только внутрь `classify`.
        pub status: String,
        pub banned: bool,
        /// Unix-время окончания подписки.
        pub expires_at: i64,
        pub used_bytes: i64,
        /// Тот же потолок, по которому работает энфорсмент
        /// (`bonus_traffic::plan_quota_limit_bytes`). `None` — безлимит.
        pub limit_bytes: Option<i64>,
        pub is_free: bool,
        /// Суточная норма плана в МБ (без бонуса). 0 — суточной нормы нет.
        pub daily_traffic_mb: i64,
        pub device_used: i64,
        /// 0 — устройства не ограничены.
        pub device_limit: i64,
        pub now: i64,
    }

    /// Период, за который посчитаны `used_bytes` и `limit_bytes`.
    ///
    /// Выводится ровно из той же ветки, по которой потолок и считался: суточная
    /// колонка участвует только на бесплатном плане с включённым ограничением.
    /// Отдельно хранить период нельзя — разъехавшись с потолком, он превращает
    /// счётчик в число без смысла.
    fn period_of(f: &AccessFacts) -> &'static str {
        if f.is_free && f.daily_traffic_mb > 0 && f.limit_bytes.is_some() {
            "day"
        } else {
            "total"
        }
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct Devices {
        pub used: i64,
        /// 0 — план не ограничивает число устройств.
        pub limit: i64,
    }

    /// Куда идти платить. `None`, если у установки не настроен бот — тогда
    /// приложению нечего показать, и врать ссылкой хуже, чем промолчать.
    #[derive(Debug, Clone, Serialize, PartialEq, Eq)]
    pub struct PayLinks {
        /// Экран тарифов мини-аппа, `https://` — работает везде.
        pub miniapp_url: Option<String>,
        /// Он же в нативной форме: открывает уже установленный Telegram без
        /// прыжка через браузер. Приложение пробует его первым.
        pub miniapp_native: Option<String>,
        /// Чат бота. Последний рубеж: экрана тарифов там нет, но первая кнопка
        /// главного меню — «Купить».
        pub bot_url: String,
    }

    /// Состояние доступа — то, против чего кодирует приложение.
    #[derive(Debug, Clone, Serialize)]
    pub struct AccessState {
        /// ЕДИНСТВЕННЫЙ флаг, по которому приложение решает, пускать ли в
        /// подключение. Всё остальное здесь — текст и цифры для экрана.
        pub may_connect: bool,
        /// Имя статуса из словаря CSM (`02-SPEC.md` 4.6.1).
        pub state: &'static str,
        /// Числовой `st` того же словаря.
        pub st: u64,
        /// Числовой код причины `rc`. Клиент ОБЯЗАН принять незнакомый код и
        /// показать общий текст своего статуса.
        pub rc: u64,
        /// Имя `rc` в snake_case; `unknown` для кода вне словаря.
        pub reason: &'static str,
        pub used_bytes: i64,
        /// `null` — безлимит.
        pub limit_bytes: Option<i64>,
        /// `day` | `total`.
        pub period: &'static str,
        /// Когда придёт следующее суточное пополнение, RFC3339. `null` для
        /// накопительного периода.
        pub resets_at: Option<String>,
        /// Насколько пополнение может опоздать относительно `resets_at`.
        pub reset_lag_seconds: Option<i64>,
        /// Сколько трафика останется сразу после пополнения. `0` — честное
        /// «завтра доступ ещё не откроется»: пополнение вычитает суточную норму,
        /// а не обнуляет расход, поэтому перерасход переносится на следующие
        /// сутки. `null` для накопительного периода.
        pub bytes_after_reset: Option<i64>,
        pub devices: Devices,
        pub pay: Option<PayLinks>,
        /// Готовая русская строка. Приложение вправе показать её как есть —
        /// это гарантия, что даже клиент, не знающий нового `rc`, покажет
        /// человеку правду, а не «код состояния 403».
        pub message_ru: String,
    }

    fn state_name(s: Status) -> &'static str {
        match s {
            Status::PendingApproval => "pending_approval",
            Status::Onboarding => "onboarding",
            Status::Active => "active",
            Status::Expired => "expired",
            Status::Revoked => "revoked",
            Status::Suspended => "suspended",
            Status::QuotaExceeded => "quota_exceeded",
            Status::DeviceLimit => "device_limit",
        }
    }

    fn reason_name(rc: ReasonCode) -> &'static str {
        match rc {
            ReasonCode::NONE => "none",
            ReasonCode::AWAITING_APPROVAL => "awaiting_approval",
            ReasonCode::ACCOUNT_SUSPENDED => "account_suspended",
            ReasonCode::ACCOUNT_CLOSED => "account_closed",
            ReasonCode::TERM_ENDED => "term_ended",
            ReasonCode::PAYMENT_FAILED => "payment_failed",
            ReasonCode::TRIAL_ENDED => "trial_ended",
            ReasonCode::TRAFFIC_QUOTA_EXHAUSTED => "traffic_quota_exhausted",
            ReasonCode::ONBOARDING_GRANT_EXHAUSTED => "onboarding_grant_exhausted",
            ReasonCode::DAILY_ALLOWANCE_EXHAUSTED => "daily_allowance_exhausted",
            ReasonCode::DEVICE_LIMIT_REACHED => "device_limit_reached",
            ReasonCode::DEVICE_REVOKED_BY_USER => "device_revoked_by_user",
            ReasonCode::DEVICE_REVOKED_BY_OPERATOR => "device_revoked_by_operator",
            ReasonCode::PLAN_WITHDRAWN => "plan_withdrawn",
            ReasonCode::FLEET_UNAVAILABLE => "fleet_unavailable",
            _ => "unknown",
        }
    }

    /// Начало ближайших суток UTC строго после `now`.
    fn next_utc_midnight(now: i64) -> i64 {
        const DAY: i64 = 86_400;
        // Целочисленное деление в Rust усекает к нулю, поэтому для
        // доисторических отрицательных отметок нужен пол, а не усечение.
        let day_start = now.div_euclid(DAY) * DAY;
        day_start + DAY
    }

    /// Сколько трафика останется сразу после суточного пополнения.
    ///
    /// Повторяет арифметику `monitoring::daily_traffic_topup`: пополнение НЕ
    /// обнуляет расход, оно вычитает из него ровно одну суточную норму с полом
    /// в нуле. Разница видна сразу: человек, спаливший 263 МБ при норме 200,
    /// завтра начнёт не с нуля, а с 63 — и ему останется 137 МБ, а не 200. Тот,
    /// кто спалил 450, завтра всё ещё будет за потолком, и приложение обязано
    /// сказать это заранее, а не заставлять его ждать полуночи впустую.
    fn bytes_after_reset(used: i64, limit: i64, daily_mb: i64) -> i64 {
        let daily_bytes = daily_mb.max(0).saturating_mul(1024 * 1024);
        let used_after = (used - daily_bytes).max(0);
        (limit - used_after).max(0)
    }

    /// «263 МБ», «1.4 ГБ» — то, что человек прочитает вслух.
    fn human_bytes(b: i64) -> String {
        const MIB: f64 = 1024.0 * 1024.0;
        const GIB: f64 = MIB * 1024.0;
        let b = b.max(0) as f64;
        if b >= GIB {
            format!("{:.1} ГБ", b / GIB)
        } else if b >= MIB {
            format!("{:.0} МБ", b / MIB)
        } else {
            format!("{:.0} КБ", (b / 1024.0).ceil())
        }
    }

    /// Дата окончания в виде, привычном русскому глазу. Бесплатный план живёт
    /// до 9999-12-31 — это не дата, а «никогда», и печатать её нельзя.
    fn human_date(ts: i64) -> Option<String> {
        let dt = chrono::DateTime::from_timestamp(ts, 0)?;
        if chrono::Datelike::year(&dt) >= 9999 {
            return None;
        }
        Some(dt.format("%d.%m.%Y").to_string())
    }

    /// Русский текст состояния.
    ///
    /// Пишется для человека, который не знает слова «throttled» и никогда не
    /// узнает. Ни «403», ни «inactive», ни «expired», ни имени статуса из базы
    /// здесь появиться не может — только что случилось, сколько израсходовано и
    /// что сделать дальше.
    fn message_ru(f: &AccessFacts, st: Status, rc: ReasonCode, period: &str) -> String {
        let used = human_bytes(f.used_bytes);
        let limit = f.limit_bytes.map(human_bytes);

        match (st, rc) {
            (Status::QuotaExceeded, _) if period == "day" => {
                let limit = limit.unwrap_or_else(|| "суточной нормы".to_string());
                let left = f
                    .limit_bytes
                    .map(|l| bytes_after_reset(f.used_bytes, l, f.daily_traffic_mb));
                let tail = match left {
                    Some(0) => " Израсходовано больше суточной нормы, поэтому одного пополнения не хватит — доступ откроется через сутки после этого."
                        .to_string(),
                    Some(left) => format!(
                        " После пополнения останется {}.",
                        human_bytes(left)
                    ),
                    None => String::new(),
                };
                format!(
                    "Дневной лимит бесплатного тарифа израсходован: {} из {}. \
                     Новая порция придёт после 00:00 UTC (03:00 МСК), обычно в течение получаса.{} \
                     Не ждать — оплатить тариф.",
                    used, limit, tail
                )
            }
            (Status::QuotaExceeded, _) => {
                let limit = limit.unwrap_or_else(|| "лимита тарифа".to_string());
                format!(
                    "Трафик тарифа израсходован: {} из {}. Чтобы продолжить, продлите или смените тариф.",
                    used, limit
                )
            }
            (Status::DeviceLimit, _) => {
                if f.device_limit > 0 {
                    format!(
                        "На этом тарифе можно подключить {} устройств(а), и они уже заняты. \
                         Отключите VPN на другом устройстве или перейдите на тариф, где устройств больше.",
                        f.device_limit
                    )
                } else {
                    "Свободных мест для устройств нет. Отключите VPN на другом устройстве и попробуйте снова."
                        .to_string()
                }
            }
            (Status::Expired, _) => match human_date(f.expires_at) {
                Some(date) => format!(
                    "Подписка закончилась {}. Продлите её, чтобы снова подключаться.",
                    date
                ),
                None => "Подписка закончилась. Продлите её, чтобы снова подключаться.".to_string(),
            },
            (Status::PendingApproval, _) => {
                "Подписка ждёт подтверждения. Обычно это занимает несколько минут.".to_string()
            }
            (Status::Suspended, _) | (Status::Revoked, _) => {
                "Доступ приостановлен. Напишите в поддержку — там видно, что произошло.".to_string()
            }
            (Status::Onboarding, _) if period == "day" => {
                let limit = limit.unwrap_or_else(|| "суточной нормы".to_string());
                format!(
                    "Бесплатный тариф: сегодня израсходовано {} из {}. \
                     Норма обновляется после 00:00 UTC (03:00 МСК).",
                    used, limit
                )
            }
            (Status::Onboarding, _) | (Status::Active, _) => match &limit {
                Some(limit) => format!("Подписка активна: израсходовано {} из {}.", used, limit),
                None => "Подписка активна, трафик не ограничен.".to_string(),
            },
        }
    }

    /// Ссылки на оплату из настроек установки.
    ///
    /// Формат ссылки не сочиняется на месте: `https`-форма берётся у
    /// `notification_templates::deep_link` — той же функции, которая печатает
    /// кнопки в уведомлениях бота. Нативная форма (`tg://`) собирается здесь,
    /// потому что нужна только приложению: она открывает уже установленный
    /// Telegram, не прогоняя человека через браузер.
    pub fn pay_links(bot_username: &str, mini_app_short_name: &str) -> Option<PayLinks> {
        let bot = bot_username.trim().trim_start_matches('@');
        if bot.is_empty() {
            return None;
        }
        let short = mini_app_short_name.trim();
        let (miniapp_url, miniapp_native) = if short.is_empty() {
            (None, None)
        } else {
            (
                crate::services::notification_templates::deep_link(
                    Some(bot),
                    short,
                    crate::services::notification_templates::ButtonTarget::Plans,
                ),
                Some(format!(
                    "tg://resolve?domain={}&appname={}&startapp=plans",
                    bot, short
                )),
            )
        };
        Some(PayLinks {
            miniapp_url,
            miniapp_native,
            bot_url: format!("https://t.me/{}", bot),
        })
    }

    /// Код причины для «трафик кончился».
    ///
    /// `classify` не различает бесплатный план с СУТОЧНОЙ нормой и бесплатный
    /// план с разовым онбординг-грантом: у обоих `is_free`, и оба получают
    /// 3002. Для человека это два разных мира — «приходи после полуночи» против
    /// «грант кончился насовсем», — и различает их ровно период. На этой
    /// установке бесплатный план суточный, так что почти всегда это 3003; ветка
    /// 3002 сохранена для плана, у которого суточной нормы нет.
    pub fn quota_reason(f: &AccessFacts) -> ReasonCode {
        if period_of(f) == "day" {
            ReasonCode::DAILY_ALLOWANCE_EXHAUSTED
        } else if f.is_free {
            ReasonCode::ONBOARDING_GRANT_EXHAUSTED
        } else {
            ReasonCode::TRAFFIC_QUOTA_EXHAUSTED
        }
    }

    /// Состояние доступа по фактам подписки.
    pub fn compute(f: &AccessFacts, pay: Option<PayLinks>) -> AccessState {
        compute_as(f, None, pay)
    }

    /// То же, но с навязанным исходом.
    ///
    /// Нужно ровно одному вызывающему: лимит устройств — это свойство ЗАПРОСА
    /// (пришёл новый адрес, а мест нет), а не строки подписки. Из базы его не
    /// вывести: подписка в этот момент совершенно исправна, и `classify` честно
    /// скажет «active». Поэтому путь `/sub/{uuid}`, который единственный это
    /// видит, передаёт исход явно.
    pub fn compute_as(
        f: &AccessFacts,
        forced: Option<(Status, ReasonCode)>,
        pay: Option<PayLinks>,
    ) -> AccessState {
        let (mut st, mut rc) = forced.unwrap_or_else(|| {
            caramba_shared::csm::directive::classify(&caramba_shared::csm::directive::StatusFacts {
                status: &f.status,
                expires_at: f.expires_at,
                used_traffic: f.used_bytes,
                // `StatusFacts` кодирует безлимит нулём, а не `None`.
                limit_bytes: f.limit_bytes.unwrap_or(0),
                free_plan: f.is_free,
                banned: f.banned,
                now: f.now,
            })
        });

        // Два уточнения там, где `classify` физически не может различить
        // случаи, разные для человека. Оба применяются ТОЛЬКО к выведенному
        // исходу: сфорсированный приходит с пути, который знает про запрос
        // больше, чем строка подписки, и перебивать его нельзя.
        if forced.is_none() {
            let over_quota = f.limit_bytes.is_some_and(|l| l > 0 && f.used_bytes >= l);

            // Статус 'expired' ставят ДВА разных гейта: срок
            // (`monitoring::check_expirations`) и трафик
            // (`expire_over_quota_subscriptions`). В строке они неразличимы, а
            // для человека это противоположные новости. Различает их дата:
            // подписка, у которой срок ещё впереди, закрыта трафиком.
            //
            // Без этого уточнения платный пользователь, упёршийся в лимит
            // гигабайтов, получал «подписка закончилась» — и шёл продлевать то,
            // что не истекло, вместо того чтобы взять тариф потолще. Хуже того,
            // он получал ДВА разных ответа на один и тот же вопрос: 3001 на
            // первом отказе (пока строка ещё 'active') и 2001 на всех
            // последующих.
            if st == Status::Expired && over_quota && f.expires_at > f.now {
                st = Status::QuotaExceeded;
            }

            if st == Status::QuotaExceeded {
                rc = quota_reason(f);
            }
        }

        let period = period_of(f);
        let daily = period == "day";

        AccessState {
            may_connect: st.may_connect(),
            state: state_name(st),
            st: st as u64,
            rc: rc.0,
            reason: reason_name(rc),
            used_bytes: f.used_bytes.max(0),
            limit_bytes: f.limit_bytes,
            period,
            resets_at: if daily {
                chrono::DateTime::from_timestamp(next_utc_midnight(f.now), 0)
                    .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            } else {
                None
            },
            reset_lag_seconds: if daily { Some(RESET_LAG_SECONDS) } else { None },
            bytes_after_reset: if daily {
                f.limit_bytes
                    .map(|l| bytes_after_reset(f.used_bytes, l, f.daily_traffic_mb))
            } else {
                None
            },
            devices: Devices {
                used: f.device_used.max(0),
                limit: f.device_limit.max(0),
            },
            pay,
            message_ru: message_ru(f, st, rc, period),
        }
    }

    // ---------------------------------------------------------------- заголовки

    /// Префикс всех заголовков причины. Ни один существующий клиент их не
    /// запрашивал, поэтому добавление не может ничего сломать: HTTP обязывает
    /// получателя игнорировать незнакомый заголовок.
    pub const HDR_STATE: &str = "x-caramba-state";
    pub const HDR_ST: &str = "x-caramba-st";
    pub const HDR_REASON: &str = "x-caramba-reason";
    pub const HDR_REASON_NAME: &str = "x-caramba-reason-name";
    pub const HDR_USED: &str = "x-caramba-used";
    pub const HDR_LIMIT: &str = "x-caramba-limit";
    pub const HDR_PERIOD: &str = "x-caramba-period";
    pub const HDR_RESETS_AT: &str = "x-caramba-resets-at";
    pub const HDR_RESET_LAG: &str = "x-caramba-reset-lag";
    pub const HDR_BYTES_AFTER_RESET: &str = "x-caramba-bytes-after-reset";

    fn put(map: &mut HeaderMap, name: &'static str, value: String) {
        // Имена — константы этого модуля, они валидны по построению; значения
        // собираются из чисел и ASCII-идентификаторов. Отбрасываем молча ту
        // единственную теоретическую ветку, где значение оказалось непечатным:
        // потерять один заголовок причины лучше, чем уронить ответ, который
        // клиент ждёт.
        if let Ok(v) = HeaderValue::from_str(&value) {
            map.insert(HeaderName::from_static(name), v);
        }
    }

    /// Заголовки причины для устаревшего пути `/sub/{uuid}`.
    ///
    /// Только ASCII и только цифры/идентификаторы: `message_ru` сюда НЕ едет.
    /// Заголовок обязан быть латиницей (RFC 9110 полагает значения ISO-8859-1),
    /// а кодировать русский текст в заголовок ради клиента, который всё равно
    /// покажет свой перевод, — способ получить мусор на экране Happ.
    pub fn refusal_headers(a: &AccessState) -> HeaderMap {
        let mut map = HeaderMap::new();
        put(&mut map, HDR_STATE, a.state.to_string());
        put(&mut map, HDR_ST, a.st.to_string());
        put(&mut map, HDR_REASON, a.rc.to_string());
        put(&mut map, HDR_REASON_NAME, a.reason.to_string());
        put(&mut map, HDR_USED, a.used_bytes.to_string());
        if let Some(limit) = a.limit_bytes {
            put(&mut map, HDR_LIMIT, limit.to_string());
        }
        put(&mut map, HDR_PERIOD, a.period.to_string());
        // Unix-секунды, а не RFC3339: клиент отказа — Go-ядро, ему нужна
        // отметка времени, а не строка на разбор.
        if let Some(resets_at) = a.resets_at.as_deref()
            && let Ok(dt) = chrono::DateTime::parse_from_rfc3339(resets_at)
        {
            put(&mut map, HDR_RESETS_AT, dt.timestamp().to_string());
        }
        if let Some(lag) = a.reset_lag_seconds {
            put(&mut map, HDR_RESET_LAG, lag.to_string());
        }
        if let Some(left) = a.bytes_after_reset {
            put(&mut map, HDR_BYTES_AFTER_RESET, left.to_string());
        }
        map
    }

    /// Заголовок `Subscription-Userinfo` в грамматике, которую читают Hiddify,
    /// Happ и clash-клиенты.
    ///
    /// Он стоит отдельно от `X-Caramba-*` по одной практической причине:
    /// зеркало подписки (`caramba-sub`) переписывает ответ панели и копирует
    /// ровно три заголовка, и `subscription-userinfo` — один из них. Значит для
    /// клиента за зеркалом это ЕДИНСТВЕННЫЙ способ узнать цифры расхода в
    /// момент отказа. Пока белый список зеркала не расширен, `X-Caramba-*`
    /// доезжают только до тех, кто ходит на панель напрямую.
    pub fn userinfo_header(used_bytes: i64, limit_bytes: Option<i64>, expires_at: i64) -> String {
        match limit_bytes {
            Some(total) => format!(
                "upload=0; download={}; total={}; expire={}",
                used_bytes.max(0),
                total,
                expires_at
            ),
            None => format!(
                "upload=0; download={}; expire={}",
                used_bytes.max(0),
                expires_at
            ),
        }
    }

    /// Отказ на устаревшем пути: прежний код, прежнее тело, новые заголовки.
    ///
    /// `body` берётся `&'static str` намеренно — тела отказов зафиксированы
    /// побайтово (`06-MIGRATION.md` 6.1), и подстановка в них чего-либо
    /// вычисляемого была бы началом их расползания.
    pub fn refusal_response(
        code: StatusCode,
        body: &'static str,
        a: &AccessState,
        userinfo: Option<String>,
    ) -> Response {
        let mut headers = refusal_headers(a);
        if let Some(info) = userinfo {
            put(&mut headers, "subscription-userinfo", info);
        }
        (code, headers, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        filter_nodes_for_subscription, mirror_country_decision, normalize_relay_param,
        promote_pinned_node,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestNode {
        id: i64,
    }

    fn ru() -> Vec<String> {
        vec!["RU".to_string()]
    }

    /// Живая расстановка этой установки: один активный релей, RU. Российский
    /// клиент идёт на зеркало (домен релея — то, ради чего оно стоит),
    /// американский получает тело с панели.
    ///
    /// Это ровно та жалоба владельца: «я просил делать так, чтобы те, кто не из
    /// России, подписочный сервис напрямую из Польши брали».
    #[test]
    fn relays_mode_sends_only_the_relay_country_through_the_mirror() {
        assert!(mirror_country_decision("relays", &ru(), Some("RU")));
        assert!(mirror_country_decision("relays", &ru(), Some("ru")));
        assert!(!mirror_country_decision("relays", &ru(), Some("US")));
        assert!(!mirror_country_decision("relays", &ru(), Some("DE")));
    }

    /// Неизвестная страна остаётся на панели, и это не осторожность, а вывод из
    /// факта: раз запрос дошёл, прямой путь у клиента работает. Единственное
    /// исключение — явный режим `*`, где оператор сказал «все».
    #[test]
    fn an_unknown_country_is_never_guessed_into_the_mirror() {
        assert!(!mirror_country_decision("relays", &ru(), None));
        assert!(!mirror_country_decision("RU,BY", &[], None));
        assert!(mirror_country_decision("*", &[], None));
    }

    /// Аварийный выход оператора в обе стороны: `*` — прежнее безусловное
    /// поведение, пустая строка — зеркало не используется никем.
    #[test]
    fn the_operator_can_force_the_mirror_on_or_off_for_everyone() {
        assert!(mirror_country_decision("*", &[], Some("US")));
        assert!(!mirror_country_decision("", &ru(), Some("RU")));
        assert!(!mirror_country_decision("   ", &ru(), Some("RU")));
    }

    /// Установка с доменом подписки, но без релеев, использует его как обычный
    /// фронт. Режим `relays` там вырождается в прежнее поведение — иначе
    /// апгрейд молча отнял бы у оператора домен, который он настроил сам.
    #[test]
    fn relays_mode_without_any_relay_keeps_the_old_blanket_behaviour() {
        assert!(mirror_country_decision("relays", &[], Some("US")));
        assert!(mirror_country_decision("relays", &[], None));
    }

    /// Явный список читается как список ISO-2: регистр и пробелы не значат
    /// ничего, мусор длиной не в два символа отбрасывается и не становится
    /// «страной», под которую случайно подойдёт чей-то код.
    #[test]
    fn an_explicit_country_list_is_parsed_as_iso2_and_nothing_else() {
        let list = ["RU".to_string()];
        assert!(mirror_country_decision(" ru , by ", &list, Some("BY")));
        assert!(!mirror_country_decision(" ru , by ", &list, Some("US")));
        // "rus" — не ISO-2, попасть в список не должен ни в каком виде.
        assert!(!mirror_country_decision("rus", &list, Some("RU")));
        assert!(!mirror_country_decision("rus", &list, Some("RUS")));
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

// ==========================================================================
// Причина отказа: словарь, арифметика и совместимость со сторонними клиентами
// ==========================================================================
#[cfg(test)]
mod access_tests {
    use super::access::*;
    use axum::http::StatusCode;
    use caramba_shared::csm::directive::{ReasonCode, Status};

    const MIB: i64 = 1024 * 1024;
    const GIB: i64 = MIB * 1024;

    /// 2026-09-05 03:04:09 UTC — время, в которое живая панель отдала последний
    /// отказ, снятый вручную. Ближайшая полночь после него — 2026-09-06.
    const NOW: i64 = 1_788_577_449;
    const NEXT_MIDNIGHT: i64 = 1_788_652_800;

    /// Живой бесплатный тариф этой установки: план 3, `is_free`,
    /// `traffic_limit_gb` 10 (включатель ограничения), `daily_traffic_mb` 200,
    /// `device_limit` 1. Потолок — 200 МБ, а НЕ 10 ГБ.
    fn free(status: &str, used_mb: i64) -> AccessFacts {
        AccessFacts {
            status: status.to_string(),
            banned: false,
            // Бесплатная подписка выдана «навсегда»: 9999-12-31.
            expires_at: 253_402_300_799,
            used_bytes: used_mb * MIB,
            limit_bytes: Some(200 * MIB),
            is_free: true,
            daily_traffic_mb: 200,
            device_used: 1,
            device_limit: 1,
            now: NOW,
        }
    }

    /// Платный тариф с накопительным лимитом (Starter: 100 ГБ, 1 устройство).
    fn paid(status: &str, used_gb: i64, expires_at: i64) -> AccessFacts {
        AccessFacts {
            status: status.to_string(),
            banned: false,
            expires_at,
            used_bytes: used_gb * GIB,
            limit_bytes: Some(100 * GIB),
            is_free: false,
            daily_traffic_mb: 0,
            device_used: 1,
            device_limit: 1,
            now: NOW,
        }
    }

    // ---------------------------------------------------------------- словарь

    /// Тот самый случай владельца: подписка 27, бесплатный тариф, 263 МБ при
    /// норме 200. До этого изменения клиент получал 403 и текст
    /// «Subscription inactive or expired» — ту же строку, что и человек с
    /// закончившимся платным тарифом, и что человек, чью подписку ещё не
    /// подтвердили. Теперь три ситуации различимы числом.
    #[test]
    fn the_owners_case_is_a_daily_allowance_not_an_expiry() {
        let a = compute(&free("throttled", 263), None);
        assert_eq!(a.state, "quota_exceeded");
        assert_eq!(a.st, Status::QuotaExceeded as u64);
        assert_eq!(a.rc, ReasonCode::DAILY_ALLOWANCE_EXHAUSTED.0);
        assert_eq!(a.reason, "daily_allowance_exhausted");
        assert!(!a.may_connect);
    }

    /// Каждая причина отказа получает СВОЮ пару (st, rc). Тест держит именно
    /// различимость: до него все эти строки приезжали клиенту одним 403.
    #[test]
    fn every_refusal_reason_is_distinct_on_the_wire() {
        let daily = compute(&free("throttled", 263), None);
        let paid_quota = compute(&paid("active", 100, NOW + 86_400), None);
        let term = compute(&paid("expired", 1, NOW - 86_400), None);
        let pending = compute(&free("pending", 0), None);
        let device = compute_as(
            &free("active", 10),
            Some((Status::DeviceLimit, ReasonCode::DEVICE_LIMIT_REACHED)),
            None,
        );
        let mut banned_facts = free("active", 10);
        banned_facts.banned = true;
        let banned = compute(&banned_facts, None);
        let fleet = compute_as(
            &free("active", 10),
            Some((Status::Suspended, ReasonCode::FLEET_UNAVAILABLE)),
            None,
        );

        let seen: Vec<(u64, u64)> = [
            &daily,
            &paid_quota,
            &term,
            &pending,
            &device,
            &banned,
            &fleet,
        ]
        .iter()
        .map(|a| (a.st, a.rc))
        .collect();

        let mut uniq = seen.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), seen.len(), "коды причин слиплись: {:?}", seen);

        assert_eq!(daily.rc, 3003);
        assert_eq!(paid_quota.rc, 3001);
        assert_eq!(term.rc, 2001);
        assert_eq!(pending.rc, 1001);
        assert_eq!(device.rc, 4001);
        assert_eq!(banned.rc, 1002);
        assert_eq!(fleet.rc, 5002);
    }

    /// Единственный флаг, по которому приложению разрешено принимать решение.
    /// Онбординг (бесплатный активный) подключаться МОЖЕТ — иначе бесплатный
    /// тариф перестал бы существовать.
    #[test]
    fn may_connect_is_true_only_where_the_tunnel_would_actually_come_up() {
        assert!(compute(&free("active", 10), None).may_connect);
        assert!(compute(&paid("active", 1, NOW + 86_400), None).may_connect);
        assert!(!compute(&free("throttled", 263), None).may_connect);
        assert!(!compute(&paid("expired", 1, NOW - 86_400), None).may_connect);
        assert!(!compute(&free("pending", 0), None).may_connect);
    }

    /// Бесплатный план с суточной нормой и бесплатный план без неё — разные
    /// причины: в первом случае человеку надо дождаться полуночи, во втором
    /// ждать нечего. `classify` их не различает, поэтому уточнение живёт здесь.
    #[test]
    fn a_daily_allowance_is_not_the_same_reason_as_a_one_off_grant() {
        assert_eq!(
            compute(&free("throttled", 263), None).rc,
            ReasonCode::DAILY_ALLOWANCE_EXHAUSTED.0
        );

        // Бесплатный план БЕЗ суточной нормы: потолок накопительный.
        let mut grant = free("active", 11 * 1024);
        grant.daily_traffic_mb = 0;
        grant.limit_bytes = Some(10 * GIB);
        grant.used_bytes = 11 * GIB;
        let a = compute(&grant, None);
        assert_eq!(a.rc, ReasonCode::ONBOARDING_GRANT_EXHAUSTED.0);
        assert_eq!(a.period, "total");
        assert_eq!(a.resets_at, None, "у накопительного лимита нет полуночи");
    }

    /// Один и тот же платный пользователь, упёршийся в лимит гигабайтов,
    /// обязан получать ОДИН ответ — и на первом отказе, когда строка ещё
    /// 'active', и на всех следующих, когда гейт уже переписал её в 'expired'.
    ///
    /// До уточнения он получал сначала «трафик кончился», потом «подписка
    /// закончилась» — и шёл продлевать срок, который не истёк.
    #[test]
    fn a_paid_plan_out_of_traffic_says_so_before_and_after_the_gate_rewrites_the_row() {
        let future = NOW + 30 * 86_400;
        let first = compute(&paid("active", 100, future), None);
        let after_sweep = compute(&paid("expired", 100, future), None);

        assert_eq!(first.state, "quota_exceeded");
        assert_eq!(first.rc, ReasonCode::TRAFFIC_QUOTA_EXHAUSTED.0);
        assert_eq!(after_sweep.state, first.state);
        assert_eq!(after_sweep.rc, first.rc);
        assert!(after_sweep.message_ru.contains("Трафик тарифа"));

        // А настоящее истечение по сроку так и остаётся истечением по сроку.
        let by_time = compute(&paid("expired", 1, NOW - 86_400), None);
        assert_eq!(by_time.state, "expired");
        assert_eq!(by_time.rc, ReasonCode::TERM_ENDED.0);
    }

    /// Незнакомый код обязан оставаться числом, а не превращаться в панику или
    /// пустую строку: клиент по контракту показывает общий текст статуса.
    #[test]
    fn an_unknown_reason_code_survives_as_a_number() {
        let a = compute_as(
            &free("active", 10),
            Some((Status::Suspended, ReasonCode(1099))),
            None,
        );
        assert_eq!(a.rc, 1099);
        assert_eq!(a.reason, "unknown");
        assert_eq!(a.state, "suspended");
    }

    // ------------------------------------------------------- арифметика сброса

    /// Ровно случай владельца: 263 МБ при норме 200. Пополнение НЕ обнуляет
    /// расход — оно вычитает одну норму, — поэтому завтра останется 137 МБ, а
    /// не 200. Приложение обязано назвать это число: «завтра будет как новый»
    /// было бы враньём на 63 МБ.
    #[test]
    fn a_reset_subtracts_one_allowance_it_does_not_zero_the_counter() {
        let a = compute(&free("throttled", 263), None);
        assert_eq!(a.period, "day");
        assert_eq!(a.bytes_after_reset, Some(137 * MIB));
    }

    /// Перерасход больше нормы: одного пополнения не хватит, и человек должен
    /// узнать это СЕЙЧАС, а не прождав полночь впустую.
    #[test]
    fn an_overrun_larger_than_the_allowance_reports_zero_not_a_full_tank() {
        let a = compute(&free("throttled", 450), None);
        assert_eq!(a.bytes_after_reset, Some(0));
        assert!(
            a.message_ru.contains("одного пополнения не хватит"),
            "{}",
            a.message_ru
        );
    }

    /// Момент сброса — ближайшая полночь UTC, и вместе с ним едет окно
    /// опоздания: пополнение крутится раз в полчаса, а не в 00:00:00.
    #[test]
    fn the_reset_is_the_next_utc_midnight_with_an_honest_lag() {
        let a = compute(&free("throttled", 263), None);
        assert_eq!(a.resets_at.as_deref(), Some("2026-09-06T00:00:00Z"));
        assert_eq!(a.reset_lag_seconds, Some(RESET_LAG_SECONDS));
        assert_eq!(RESET_LAG_SECONDS, 1800);

        let dt = chrono::DateTime::parse_from_rfc3339(a.resets_at.as_deref().unwrap()).unwrap();
        assert_eq!(dt.timestamp(), NEXT_MIDNIGHT);
        assert!(dt.timestamp() > NOW);
        assert!(dt.timestamp() - NOW <= 86_400);
    }

    /// Ровно в полночь сброс — СЛЕДУЮЩАЯ полночь, а не текущая секунда: иначе
    /// приложение показало бы «обновится прямо сейчас» и осталось бы так висеть.
    #[test]
    fn exactly_at_midnight_the_next_reset_is_a_full_day_out() {
        let mut f = free("throttled", 263);
        f.now = NEXT_MIDNIGHT;
        let a = compute(&f, None);
        let dt = chrono::DateTime::parse_from_rfc3339(a.resets_at.as_deref().unwrap()).unwrap();
        assert_eq!(dt.timestamp(), NEXT_MIDNIGHT + 86_400);
    }

    /// Накопительный период не получает ни момента сброса, ни остатка после
    /// него: сбрасывать нечему, и `null` здесь — единственный честный ответ.
    #[test]
    fn a_total_period_carries_no_reset_fields() {
        let a = compute(&paid("active", 100, NOW + 86_400), None);
        assert_eq!(a.period, "total");
        assert_eq!(a.resets_at, None);
        assert_eq!(a.reset_lag_seconds, None);
        assert_eq!(a.bytes_after_reset, None);
    }

    /// Безлимит: потолка нет, значит нет и периода со сбросом. Ноль в
    /// `limit_bytes` был бы «израсходовано всё», поэтому именно `null`.
    #[test]
    fn an_unlimited_plan_reports_a_null_ceiling_not_a_zero_one() {
        let mut f = paid("active", 500, NOW + 86_400);
        f.limit_bytes = None;
        let a = compute(&f, None);
        assert_eq!(a.limit_bytes, None);
        assert_eq!(a.period, "total");
        assert!(a.may_connect);
        assert!(a.message_ru.contains("не ограничен"), "{}", a.message_ru);
    }

    // ------------------------------------------------------------------ текст

    /// Внутренние слова не выходят наружу. `throttled` живёт в базе и в логах;
    /// человек, который его увидит, не поймёт ничего — ровно это и случилось.
    #[test]
    fn no_internal_vocabulary_ever_reaches_a_human() {
        let cases = [
            compute(&free("throttled", 263), None),
            compute(&free("throttled", 450), None),
            compute(&paid("expired", 1, NOW - 86_400), None),
            compute(&paid("active", 100, NOW + 86_400), None),
            compute(&free("pending", 0), None),
            compute(&free("active", 10), None),
            compute_as(
                &free("active", 10),
                Some((Status::DeviceLimit, ReasonCode::DEVICE_LIMIT_REACHED)),
                None,
            ),
        ];
        for a in &cases {
            let text = a.message_ru.to_lowercase();
            for banned in [
                "throttl",
                "expired",
                "inactive",
                "403",
                "код состояния",
                "quota",
                "device limit",
                "subscription",
            ] {
                assert!(
                    !text.contains(banned),
                    "внутреннее слово {:?} уехало в текст: {}",
                    banned,
                    a.message_ru
                );
            }
            assert!(!a.message_ru.is_empty());
        }
    }

    /// Текст суточного отказа обязан нести ТРИ вещи, иначе он бесполезен:
    /// сколько израсходовано, из чего, и когда снова можно.
    #[test]
    fn the_daily_message_carries_the_numbers_a_person_needs() {
        let a = compute(&free("throttled", 263), None);
        assert!(a.message_ru.contains("263 МБ"), "{}", a.message_ru);
        assert!(a.message_ru.contains("200 МБ"), "{}", a.message_ru);
        assert!(a.message_ru.contains("00:00 UTC"), "{}", a.message_ru);
        assert!(a.message_ru.contains("137 МБ"), "{}", a.message_ru);
    }

    /// «Бессрочная» подписка бесплатного тарифа живёт до 9999-12-31. Печатать
    /// эту дату человеку нельзя — это не срок, это отсутствие срока.
    #[test]
    fn the_year_9999_placeholder_is_never_printed_as_a_date() {
        let mut f = free("expired", 0);
        f.expires_at = 253_402_300_799;
        let a = compute(&f, None);
        assert!(!a.message_ru.contains("9999"), "{}", a.message_ru);
        assert!(!a.message_ru.contains("31.12"), "{}", a.message_ru);

        // Настоящая дата, наоборот, обязана быть названа.
        let real = compute(&paid("expired", 1, 1_788_480_000), None);
        assert!(real.message_ru.contains("2026"), "{}", real.message_ru);
    }

    // ------------------------------------------------------------ оплата

    /// Ссылки берутся из настроек установки; нативная форма — та, что открывает
    /// уже установленный Telegram, `https` — та, что работает всегда.
    #[test]
    fn pay_links_offer_a_native_hop_an_https_hop_and_the_bare_bot() {
        let p = pay_links("exa_robot", "exaconnect").unwrap();
        assert_eq!(
            p.miniapp_url.as_deref(),
            Some("https://t.me/exa_robot/exaconnect?startapp=plans")
        );
        assert_eq!(
            p.miniapp_native.as_deref(),
            Some("tg://resolve?domain=exa_robot&appname=exaconnect&startapp=plans")
        );
        assert_eq!(p.bot_url, "https://t.me/exa_robot");
    }

    /// Собачка перед именем бота — обычная опечатка оператора в настройках, и
    /// она не должна превращать ссылку в мёртвую.
    #[test]
    fn a_leading_at_sign_in_the_setting_does_not_break_the_link() {
        let p = pay_links("@exa_robot", " exaconnect ").unwrap();
        assert_eq!(p.bot_url, "https://t.me/exa_robot");
        assert_eq!(
            p.miniapp_url.as_deref(),
            Some("https://t.me/exa_robot/exaconnect?startapp=plans")
        );
    }

    /// Без мини-аппа остаётся чат бота: экрана тарифов там нет, но первая
    /// кнопка главного меню — «Купить». Без бота вообще — `None`: мёртвая
    /// кнопка хуже отсутствующей.
    #[test]
    fn a_missing_mini_app_degrades_to_the_bot_and_a_missing_bot_to_nothing() {
        let p = pay_links("exa_robot", "").unwrap();
        assert_eq!(p.miniapp_url, None);
        assert_eq!(p.miniapp_native, None);
        assert_eq!(p.bot_url, "https://t.me/exa_robot");

        assert_eq!(pay_links("", "exaconnect"), None);
        assert_eq!(pay_links("   ", ""), None);
        assert_eq!(pay_links("@", "x"), None);
    }

    // ------------------------------------- совместимость со сторонними клиентами

    /// Исторические тела трёх отказов. Happ, Hiddify и clash-клиенты читают
    /// «конфиг или не-200 с текстом», а `06-MIGRATION.md` 6.1 фиксирует эти
    /// строки побайтово. Тест держит их именно как байты: изменить их можно
    /// только осознанно, сломав этот тест.
    const BODY_INACTIVE: &str = "Subscription inactive or expired";
    const BODY_QUOTA: &str = "Traffic limit reached. Subscription is expired.";
    const BODY_DEVICE: &str = "Device limit reached";

    async fn parts(r: axum::response::Response) -> (StatusCode, axum::http::HeaderMap, String) {
        let (p, body) = r.into_parts();
        let bytes = axum::body::to_bytes(body, 64 * 1024).await.unwrap();
        (
            p.status,
            p.headers,
            String::from_utf8(bytes.to_vec()).unwrap(),
        )
    }

    /// Главная гарантия обратной совместимости: код и тело отказа не менялись.
    /// Клиент, который умеет только «200 или ошибка», видит ровно то же, что
    /// видел вчера.
    #[tokio::test]
    async fn the_legacy_refusal_bodies_are_unchanged_to_the_byte() {
        let a = compute(&free("throttled", 263), None);
        for (body, code) in [
            (BODY_INACTIVE, StatusCode::FORBIDDEN),
            (BODY_QUOTA, StatusCode::FORBIDDEN),
            (BODY_DEVICE, StatusCode::FORBIDDEN),
        ] {
            let (status, _, got) = parts(refusal_response(code, body, &a, None)).await;
            assert_eq!(status, code);
            assert_eq!(got, body);
            assert_eq!(got.as_bytes(), body.as_bytes());
        }
    }

    /// Всё новое едет заголовками, и ни один из них не трогает то, что старый
    /// клиент читает. Разрешённый список ровно два вида имён: `x-caramba-*`
    /// (никто их не запрашивал, HTTP обязывает игнорировать незнакомое) и
    /// `subscription-userinfo` — заголовок, который Hiddify и Happ уже читают и
    /// который на успешном ответе и так был.
    #[tokio::test]
    async fn everything_added_is_a_header_a_third_party_client_already_ignores() {
        let a = compute(&free("throttled", 263), None);
        let userinfo = userinfo_header(263 * MIB, Some(200 * MIB), 253_402_300_799);
        let (_, headers, _) = parts(refusal_response(
            StatusCode::FORBIDDEN,
            BODY_QUOTA,
            &a,
            Some(userinfo),
        ))
        .await;

        for name in headers.keys() {
            let n = name.as_str();
            let benign = n.starts_with("x-caramba-")
                || n == "subscription-userinfo"
                || n == "content-type"
                || n == "content-length";
            assert!(benign, "неожиданный заголовок в отказе: {}", n);
        }

        // Content-Type не поменялся: тело как было текстом, так и осталось.
        assert_eq!(
            headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default(),
            "text/plain; charset=utf-8"
        );
    }

    /// Значения заголовков — только печатный ASCII. Русский текст в заголовок
    /// не едет: RFC 9110 трактует значения как ISO-8859-1, и Happ показал бы
    /// мусор вместо объяснения. Объяснение живёт в JSON приложения.
    #[tokio::test]
    async fn header_values_stay_ascii_so_no_client_renders_mojibake() {
        let a = compute(&free("throttled", 263), None);
        let (_, headers, _) = parts(refusal_response(
            StatusCode::FORBIDDEN,
            BODY_QUOTA,
            &a,
            Some(userinfo_header(263 * MIB, Some(200 * MIB), 1_788_652_800)),
        ))
        .await;
        for (name, value) in headers.iter() {
            let v = value
                .to_str()
                .expect("значение заголовка должно быть ASCII");
            assert!(
                v.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                "непечатное значение в {}: {:?}",
                name,
                v
            );
        }
    }

    /// Цифры, которые едут с отказом, — те же, что и в JSON: заголовок и экран
    /// приложения не могут показать разное, потому что источник один.
    #[tokio::test]
    async fn the_refusal_headers_carry_the_same_numbers_as_the_json() {
        let a = compute(&free("throttled", 263), None);
        let (_, h, _) = parts(refusal_response(
            StatusCode::FORBIDDEN,
            BODY_QUOTA,
            &a,
            None,
        ))
        .await;
        let get = |k: &str| {
            h.get(k)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        };

        assert_eq!(get(HDR_STATE).as_deref(), Some("quota_exceeded"));
        assert_eq!(get(HDR_ST).as_deref(), Some("7"));
        assert_eq!(get(HDR_REASON).as_deref(), Some("3003"));
        assert_eq!(
            get(HDR_REASON_NAME).as_deref(),
            Some("daily_allowance_exhausted")
        );
        assert_eq!(get(HDR_USED), Some((263 * MIB).to_string()));
        assert_eq!(get(HDR_LIMIT), Some((200 * MIB).to_string()));
        assert_eq!(get(HDR_PERIOD).as_deref(), Some("day"));
        // Отметка времени — Unix-секунды: клиент отказа это Go-ядро, ему нужно
        // число, а не строка на разбор.
        assert_eq!(get(HDR_RESETS_AT), Some(NEXT_MIDNIGHT.to_string()));
        assert_eq!(get(HDR_RESET_LAG).as_deref(), Some("1800"));
        assert_eq!(get(HDR_BYTES_AFTER_RESET), Some((137 * MIB).to_string()));

        // Ни один заголовок не содержит внутреннего слова.
        for value in h.values() {
            let v = value.to_str().unwrap_or_default();
            assert!(!v.contains("throttl"), "{}", v);
        }
    }

    /// Безлимитный план не печатает `x-caramba-limit`: пустой заголовок или
    /// ноль клиент прочитал бы как «лимит равен нулю».
    #[tokio::test]
    async fn an_unlimited_plan_omits_the_limit_header_rather_than_sending_zero() {
        let mut f = paid("expired", 5, NOW - 10);
        f.limit_bytes = None;
        let a = compute(&f, None);
        let (_, h, _) = parts(refusal_response(
            StatusCode::FORBIDDEN,
            BODY_INACTIVE,
            &a,
            None,
        ))
        .await;
        assert!(h.get(HDR_LIMIT).is_none());
        assert!(h.get(HDR_RESETS_AT).is_none());
        assert_eq!(
            h.get(HDR_PERIOD).and_then(|v| v.to_str().ok()),
            Some("total")
        );
    }

    /// `Subscription-Userinfo` в грамматике, которую Hiddify/Happ уже разбирают.
    /// На отказе он ВАЖНЕЕ, чем на успехе: зеркало подписки (`caramba-sub`)
    /// копирует ровно три заголовка, и это единственный из наших, который
    /// доезжает до клиента за зеркалом.
    #[test]
    fn the_userinfo_header_keeps_the_grammar_third_party_clients_parse() {
        let with_limit = userinfo_header(263 * MIB, Some(200 * MIB), 253_402_300_799);
        assert_eq!(
            with_limit,
            "upload=0; download=275775488; total=209715200; expire=253402300799"
        );

        // Разбор в стиле клиента: пары `ключ=значение` через `; `.
        let pairs: std::collections::BTreeMap<&str, &str> = with_limit
            .split("; ")
            .filter_map(|kv| kv.split_once('='))
            .collect();
        assert_eq!(pairs.get("upload"), Some(&"0"));
        assert_eq!(pairs.get("download"), Some(&"275775488"));
        assert_eq!(pairs.get("total"), Some(&"209715200"));
        assert!(pairs.contains_key("expire"));

        // Безлимит — БЕЗ `total`: клиент рисует ∞. Ноль он показал бы как
        // «исчерпано полностью».
        let unlimited = userinfo_header(5 * GIB, None, 1_788_652_800);
        assert_eq!(
            unlimited,
            "upload=0; download=5368709120; expire=1788652800"
        );
        assert!(!unlimited.contains("total"));
    }

    // --------------------------------------------------------- форма на проводе

    /// Точная форма объекта `access`. Приложение кодирует ПРОТИВ этого снимка,
    /// поэтому тест держит и набор ключей, и значения: молча переименованное
    /// поле — это молча сломанный экран у всех, кто уже обновился.
    #[test]
    fn the_access_object_has_exactly_this_shape_on_the_wire() {
        let a = compute(
            &free("throttled", 263),
            pay_links("exa_robot", "exaconnect"),
        );
        let v = serde_json::to_value(&a).unwrap();

        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "bytes_after_reset",
                "devices",
                "limit_bytes",
                "may_connect",
                "message_ru",
                "pay",
                "period",
                "rc",
                "reason",
                "reset_lag_seconds",
                "resets_at",
                "st",
                "state",
                "used_bytes",
            ]
        );

        assert_eq!(v["may_connect"], serde_json::json!(false));
        assert_eq!(v["state"], serde_json::json!("quota_exceeded"));
        assert_eq!(v["st"], serde_json::json!(7));
        assert_eq!(v["rc"], serde_json::json!(3003));
        assert_eq!(v["reason"], serde_json::json!("daily_allowance_exhausted"));
        assert_eq!(v["used_bytes"], serde_json::json!(275_775_488i64));
        assert_eq!(v["limit_bytes"], serde_json::json!(209_715_200i64));
        assert_eq!(v["period"], serde_json::json!("day"));
        assert_eq!(v["resets_at"], serde_json::json!("2026-09-06T00:00:00Z"));
        assert_eq!(v["reset_lag_seconds"], serde_json::json!(1800));
        assert_eq!(v["bytes_after_reset"], serde_json::json!(143_654_912i64));
        assert_eq!(v["devices"], serde_json::json!({"used": 1, "limit": 1}));
        assert_eq!(
            v["pay"],
            serde_json::json!({
                "miniapp_url": "https://t.me/exa_robot/exaconnect?startapp=plans",
                "miniapp_native": "tg://resolve?domain=exa_robot&appname=exaconnect&startapp=plans",
                "bot_url": "https://t.me/exa_robot"
            })
        );
        assert!(v["message_ru"].as_str().unwrap().contains("263 МБ"));
    }

    /// Необязательные поля приезжают как `null`, а не пропадают: клиент,
    /// читающий ключ, обязан получить ответ «не применимо», а не наткнуться на
    /// его отсутствие и решить, что панель старая.
    #[test]
    fn inapplicable_fields_are_null_not_absent() {
        let mut f = paid("expired", 5, NOW - 10);
        f.limit_bytes = None;
        let v = serde_json::to_value(compute(&f, None)).unwrap();
        for key in [
            "limit_bytes",
            "resets_at",
            "reset_lag_seconds",
            "bytes_after_reset",
            "pay",
        ] {
            assert!(v.get(key).is_some(), "ключ {} пропал", key);
            assert!(v[key].is_null(), "ключ {} должен быть null", key);
        }
    }

    /// Отрицательного расхода в базе больше нет, но заголовок уходит наружу, и
    /// `download=-1` для стороннего клиента — нестандартное значение.
    #[test]
    fn a_negative_usage_never_leaks_into_the_wire() {
        assert!(userinfo_header(-1, Some(200 * MIB), 0).contains("download=0"));
        let mut f = free("throttled", 0);
        f.used_bytes = -5;
        assert_eq!(compute(&f, None).used_bytes, 0);
    }
}
