//! JWT-защищённые эндпоинты управления аккаунтом standalone-приложения.
//!
//! Дополняют `app.rs` (профиль/подписка/серверы) разделами, которые рисует
//! Flutter-клиент: устройства, рефералы, семья, список подписок, выбор relay.
//! Все хендлеры идут за `app_auth::require_app_jwt` — `AuthUser` берётся из
//! extensions. Стиль запросов повторяет `app.rs`/`api/client.rs`: сырой sqlx,
//! локальные DTO с `Serialize`, JSON-формы согласованы с Flutter-моделями.

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use crate::services::referral_service::ReferralService;
use crate::services::subscription_service::DeviceIdentity;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

// ============================================================
// Общие хелперы
// ============================================================

/// Маскирует IP для отдачи клиенту (последний октет/группа скрывается).
/// Дублирует логику api/client.rs::mask_ip, чтобы не тащить её в pub.
fn mask_ip(ip: &str) -> String {
    if let Some(idx) = ip.rfind('.') {
        // IPv4: 203.0.113.42 -> 203.0.113.*
        format!("{}.*", &ip[..idx])
    } else if let Some(idx) = ip.rfind(':') {
        // IPv6: скрываем последнюю группу
        format!("{}:*", &ip[..idx])
    } else {
        ip.to_string()
    }
}

/// Базовый URL панели — для построения реферальной ссылки на веб, если бот не задан.
async fn panel_base_url(state: &AppState, headers: &HeaderMap) -> String {
    let panel = state.settings.get_or_default("panel_url", "").await;
    let base = if !panel.is_empty() {
        panel
    } else if let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) {
        host.to_string()
    } else {
        std::env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
    };
    if base.starts_with("http") {
        base
    } else {
        let proto = if base.contains("localhost") || base.contains("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        format!("{}://{}", proto, base)
    }
}

/// Проверяет, что подписка принадлежит пользователю. Возвращает true/false.
async fn sub_owned_by(state: &AppState, sub_id: i64, user_id: i64) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT user_id FROM subscriptions WHERE id = $1")
        .bind(sub_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        == Some(user_id)
}

// ============================================================
// DEVICES — из subscription_device_leases
// ============================================================

#[derive(Serialize)]
struct AppDevice {
    /// id lease-записи (используется в PATCH/DELETE).
    id: i64,
    subscription_id: i64,
    /// Имя устройства: пользовательское (display_name) либо авто из User-Agent.
    name: String,
    /// То же имя отдельным полем — контракт с приложением (B3).
    display_name: String,
    /// android/ios/macos/windows/linux, если устройство представилось.
    platform: Option<String>,
    /// Стабильный идентификатор установки приложения; у сторонних клиентов NULL.
    client_device_id: Option<String>,
    last_ip: String,
    user_agent: Option<String>,
    first_seen_at: String,
    last_seen_at: String,
    /// Онлайн в последние 15 минут (та же эвристика, что и в api/client.rs).
    online: bool,
    /// Это устройство, с которого пришёл запрос.
    is_current: bool,
}

/// GET /api/v2/app/devices — все устройства аккаунта.
///
/// Список строится по владельцу, а не по подписке: устройство принадлежит
/// аккаунту и переживает смену тарифа (см. миграцию 20260911150000).
///
/// `is_current` определяется по заголовку `X-Caramba-Device-Id`, который шлёт
/// приложение: сравнение по адресу врало на любом NAT — все телефоны в одной
/// квартире помечались как «это устройство».
pub async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let caller = DeviceIdentity::from_headers(&headers);
    // Запасной вариант для клиента без идентификатора установки — сравнение по
    // адресу. Оно врёт на общем NAT, поэтому используется только когда опознать
    // устройство нечем.
    let caller_ip = crate::subscription::extract_client_ip(&headers);
    let leases = state
        .subscription_service
        .list_user_devices(auth.user_id)
        .await
        .unwrap_or_default();

    let devices: Vec<AppDevice> = leases
        .into_iter()
        .map(|l| {
            let label = l.label();
            let is_current = match (
                caller.client_device_id.as_deref(),
                l.client_device_id.as_deref(),
            ) {
                (Some(mine), Some(theirs)) => mine == theirs,
                (Some(_), None) => false,
                _ => !caller_ip.is_empty() && caller_ip == l.last_ip,
            };
            AppDevice {
                id: l.id,
                subscription_id: l.subscription_id,
                name: label.clone(),
                display_name: label,
                platform: l.platform,
                client_device_id: l.client_device_id,
                last_ip: mask_ip(&l.last_ip),
                user_agent: l.user_agent,
                first_seen_at: l.first_seen_at.to_rfc3339(),
                last_seen_at: l.last_seen_at.to_rfc3339(),
                online: l.online,
                is_current,
            }
        })
        .collect();

    Json(devices).into_response()
}

#[derive(Deserialize)]
pub struct RenameDeviceRequest {
    /// Новое имя; пустая строка/null сбрасывает на авто-имя.
    pub name: Option<String>,
}

/// PATCH /api/v2/app/devices/{id} — переименование устройства (display_name).
///
/// Пустое имя сбрасывает на авто-имя из User-Agent. Принадлежность лизы
/// аккаунту проверяет сам UPDATE (`WHERE user_id = $`), поэтому отдельного
/// запроса на проверку владельца больше нет.
pub async fn rename_device(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(device_id): Path<i64>,
    Json(payload): Json<RenameDeviceRequest>,
) -> impl IntoResponse {
    match state
        .subscription_service
        .rename_user_device(auth.user_id, device_id, payload.name.as_deref())
        .await
    {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Device not found").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: rename device failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE /api/v2/app/devices/{id} — отзыв (kick) устройства.
///
/// Удаляет лизу аккаунта вместе с легаси-записью IP-трекинга, затем асинхронно
/// рвёт соединения подписки, чтобы отвязанное устройство отвалилось сразу, а не
/// на следующем пятиминутном опросе.
pub async fn revoke_device(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(device_id): Path<i64>,
) -> impl IntoResponse {
    let sub_id = match state
        .subscription_service
        .revoke_user_device(auth.user_id, device_id)
        .await
    {
        Ok(Some(sub_id)) => sub_id,
        Ok(None) => return (StatusCode::NOT_FOUND, "Device not found").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: revoke device failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let conn_service = state.connection_service.clone();
    tokio::spawn(async move {
        if let Err(e) = conn_service.kill_subscription_connections(sub_id).await {
            tracing::warn!(sub_id, error = %e, "app: kill connections after revoke failed");
        }
    });

    Json(serde_json::json!({ "ok": true, "message": "Device revoked" })).into_response()
}

// ============================================================
// REFERRALS
// ============================================================

/// Одна запись о приглашённом пользователе в сводке рефералов.
#[derive(Serialize)]
struct AppReferralEntry {
    /// Маскированный логин приглашённого.
    username_masked: String,
    /// Дата присоединения (RFC3339).
    joined_at: String,
    /// 'registered' (ещё не платил) | 'purchased' (есть оплаченная покупка).
    status: &'static str,
    /// Заработано рефереру с этого приглашённого, минорные единицы (центы).
    earned: i64,
}

/// Authoritative referral-money contract (panel emits, Flutter consumes).
/// Field names are the contract; see the app referral money-reward spec.
#[derive(Serialize)]
struct AppReferrals {
    referral_code: String,
    /// https://exarobot.top/r/CODE или https://t.me/<bot>?start=ref_CODE.
    referral_link: String,
    invited_count: i64,
    /// Текущий баланс пользователя, минорные единицы (центы).
    balance: i64,
    /// Совокупно начислено рефереру с рефералов за всё время, центы.
    balance_earned: i64,
    /// % платежа приглашённого, который зачисляется рефереру.
    reward_percent: i64,
    /// % скидки приглашённому на ПЕРВУЮ платную покупку.
    referee_discount_percent: i64,
    referrals: Vec<AppReferralEntry>,
}

/// GET /api/v2/app/referrals — реферальная сводка пользователя (money model).
pub async fn get_referrals(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let code: Option<String> = sqlx::query_scalar("SELECT referral_code FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    let balance: i64 = sqlx::query_scalar("SELECT balance::BIGINT FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

    let invited_count = ReferralService::get_referral_count(&state.pool, auth.user_id)
        .await
        .unwrap_or(0);
    let balance_earned = ReferralService::get_user_referral_earnings(&state.pool, auth.user_id)
        .await
        .unwrap_or(0);

    // reward_percent: per-user override (user_referral_rates.bonus_percent) ->
    // global setting -> default 20. Reuse ReferralService logic via a short tx.
    let reward_percent = match state.pool.begin().await {
        Ok(mut tx) => {
            let pct = ReferralService::reward_percent(&mut tx, auth.user_id)
                .await
                .unwrap_or(20);
            let _ = tx.rollback().await;
            pct
        }
        Err(_) => 20,
    };

    // referee_discount_percent: global referee discount setting (contract
    // default 15). This is the headline rate the referee would receive on a
    // first purchase, independent of whether THIS user has already purchased.
    let referee_discount_percent: i64 = state
        .settings
        .get_or_default("referral_referee_discount_percent", "15")
        .await
        .parse()
        .unwrap_or(15);

    // Per-referral breakdown: status + earned for each invited user.
    let referrals = ReferralService::get_user_referrals(&state.pool, auth.user_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let username_masked = mask_username(
                r.username
                    .as_deref()
                    .or(r.full_name.as_deref())
                    .unwrap_or("user"),
            );
            // 'purchased' once the referee earned us a payout (first paid
            // purchase fulfilled); otherwise 'registered'.
            let status = if r.total_earned > 0 {
                "purchased"
            } else {
                "registered"
            };
            AppReferralEntry {
                username_masked,
                joined_at: r.created_at.to_rfc3339(),
                status,
                earned: r.total_earned,
            }
        })
        .collect();

    // Ссылки: deeplink в бота (если задан bot_username) + веб-ссылка панели.
    let code_str = code.clone().unwrap_or_default();
    let bot_username = state.settings.get_or_default("bot_username", "").await;
    let bot_username = bot_username.trim().trim_start_matches('@').to_string();
    let referral_link = if !bot_username.is_empty() && !code_str.is_empty() {
        format!("https://t.me/{}?start=ref_{}", bot_username, code_str)
    } else {
        let base = panel_base_url(&state, &headers).await;
        format!("{}/r/{}", base, code_str)
    };

    Json(AppReferrals {
        referral_code: code_str,
        referral_link,
        invited_count,
        balance,
        balance_earned,
        reward_percent,
        referee_discount_percent,
        referrals,
    })
    .into_response()
}

/// Маскирует логин для выдачи клиенту (часть символов скрывается).
/// Char-safe: full_name может содержать многобайтовые символы.
fn mask_username(username: &str) -> String {
    let username = username.trim();
    let char_count = username.chars().count();
    if char_count <= 3 {
        return "***".to_string();
    }
    let visible = if char_count > 6 { 3 } else { 1 };
    let prefix: String = username.chars().take(visible).collect();
    format!("{}***", prefix)
}

// ============================================================
// FAMILY — users.parent_id + family_invites
// ============================================================

#[derive(Deserialize)]
pub struct FamilyQuery {
    /// Опционально: ограничить выборку конкретной подпиской пользователя.
    pub subscription_id: Option<i64>,
}

#[derive(Serialize)]
struct FamilyMember {
    user_id: i64,
    username: Option<String>,
    full_name: Option<String>,
    /// Есть ли у участника активная семейная подписка.
    has_active_sub: bool,
    joined_at: String,
}

#[derive(Serialize)]
struct FamilyResponse {
    /// Является ли текущий пользователь главой семьи (есть дети).
    is_parent: bool,
    members: Vec<FamilyMember>,
}

/// GET /api/v2/app/family — участники семьи (дети по users.parent_id).
///
/// `subscription_id` принимается для совместимости с UI (картинка «слотов»
/// конкретной подписки), но членство в семье в текущей схеме привязано к
/// пользователю, а не к подписке, поэтому используется лишь для валидации
/// владения. Семейные подписки помечены note = 'Family'.
pub async fn get_family(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Query(q): Query<FamilyQuery>,
) -> impl IntoResponse {
    if let Some(sid) = q.subscription_id
        && !sub_owned_by(&state, sid, auth.user_id).await
    {
        return (StatusCode::FORBIDDEN, "Not your subscription").into_response();
    }

    let rows = sqlx::query(
        r#"SELECT u.id,
                  u.username,
                  u.full_name,
                  u.created_at,
                  EXISTS(
                      SELECT 1 FROM subscriptions s
                      WHERE s.user_id = u.id AND s.status = 'active' AND s.note = 'Family'
                  ) AS has_active_sub
           FROM users u
           WHERE u.parent_id = $1
           ORDER BY u.created_at ASC"#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let members: Vec<FamilyMember> = rows
        .into_iter()
        .map(|r| FamilyMember {
            user_id: r.try_get("id").unwrap_or(0),
            username: r.try_get::<Option<String>, _>("username").ok().flatten(),
            full_name: r.try_get::<Option<String>, _>("full_name").ok().flatten(),
            has_active_sub: r.try_get::<bool, _>("has_active_sub").unwrap_or(false),
            joined_at: r
                .try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        })
        .collect();

    Json(FamilyResponse {
        is_parent: !members.is_empty(),
        members,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct FamilyInviteRequest {
    /// Подписка, чьи свободные device-слоты отдаются семье (валидация владения).
    pub subscription_id: Option<i64>,
    pub max_uses: Option<i32>,
    pub duration_days: Option<i32>,
}

#[derive(Serialize)]
struct FamilyInviteResponse {
    code: String,
    expires_at: String,
    max_uses: i32,
    used_count: i32,
}

/// POST /api/v2/app/family/invite — создаёт инвайт в семью текущего пользователя.
///
/// Переиспользует store_service::create_family_invite (та же таблица
/// family_invites, parent_id = текущий пользователь). Принятие инвайта
/// проставляет users.parent_id и синхронизирует семейные подписки.
pub async fn create_family_invite(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Json(payload): Json<FamilyInviteRequest>,
) -> impl IntoResponse {
    if let Some(sid) = payload.subscription_id
        && !sub_owned_by(&state, sid, auth.user_id).await
    {
        return (StatusCode::FORBIDDEN, "Not your subscription").into_response();
    }

    let max_uses = payload.max_uses.unwrap_or(1).clamp(1, 100);
    let duration = payload.duration_days.unwrap_or(7).clamp(1, 30);

    match state
        .store_service
        .create_family_invite(auth.user_id, max_uses, duration)
        .await
    {
        Ok(invite) => Json(FamilyInviteResponse {
            code: invite.code,
            expires_at: invite.expires_at.to_rfc3339(),
            max_uses: invite.max_uses,
            used_count: invite.used_count,
        })
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: create family invite failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create invite").into_response()
        }
    }
}

/// DELETE /api/v2/app/family/{member_id} — исключить участника из семьи.
///
/// Снимает parent_id у ребёнка (только если его родитель — текущий
/// пользователь) и истекает его семейные подписки через store_service.
pub async fn remove_family_member(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(member_id): Path<i64>,
) -> impl IntoResponse {
    // Ребёнок должен принадлежать именно этой семье.
    let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM users WHERE id = $1")
        .bind(member_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    if parent != Some(auth.user_id) {
        return (StatusCode::NOT_FOUND, "Member not found in your family").into_response();
    }

    // set_user_parent(None) снимает родителя; затем истекаем семейные подписки.
    if let Err(e) = state.store_service.set_user_parent(member_id, None).await {
        tracing::error!(err = %e, "app: remove family member (clear parent) failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let _ = sqlx::query(
        "UPDATE subscriptions SET status = 'expired' \
         WHERE user_id = $1 AND note = 'Family' AND status = 'active'",
    )
    .bind(member_id)
    .execute(&state.pool)
    .await;

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ============================================================
// SUBSCRIPTIONS — список с разбивкой kind/traffic/devices/pool
// ============================================================

#[derive(Serialize)]
struct AppSubscription {
    id: i64,
    subscription_uuid: String,
    plan_name: String,
    status: String,
    /// free | paid | private (private = семейная, выданная родителем).
    kind: String,
    /// Расход за период, который называет `quota_period`: на суточном
    /// плане это расход, ещё не прощённый суточным пополнением
    /// (`monitoring::daily_traffic_topup` вычитает норму с полом 0), то есть
    /// фактически «сегодня»; на остальных — за весь срок подписки.
    used_traffic_bytes: i64,
    used_traffic_gb: String,
    /// СЫРАЯ колонка плана `traffic_limit_gb`, а НЕ потолок (0/None =
    /// в колонке нуль). На бесплатном плане энфорсмент считает по суточной
    /// норме, поэтому здесь стоит 10 у того, кого душат на 200 МБ. Поле
    /// сохранено со старым значением ради уже выпущенных клиентов; новый
    /// код обязан брать `traffic_limit_bytes` вместе с `quota_period`.
    traffic_quota_gb: Option<i32>,
    /// Еженедельное пополнение бесплатного трафика в ГБ (daily_traffic_mb * 7).
    /// Это ПЕРЕСЧЁТ суточной нормы на неделю, а не квота: делить на него
    /// `used_traffic_*` нельзя — счётчик живёт на другом периоде.
    weekly_free_refill_gb: Option<f64>,
    /// Потолок, по которому реально работают истечение и троттлинг: норма
    /// плана (суточная на free, тарифная на paid) + бонусный трафик.
    /// `null` = безлимит. Единственное число, против которого можно рисовать бар.
    traffic_limit_bytes: Option<i64>,
    /// Период, за который посчитаны `traffic_limit_bytes` и `used_traffic_*`:
    /// "day" | "total". Число без периода — ровно та ошибка, из-за которой
    /// карточка подписки врала в обе стороны сразу.
    quota_period: &'static str,
    /// Признак бесплатного тарифа (`plans.is_free`) — тот же флаг, по которому
    /// энфорсмент выбирает ветку потолка. `kind` его не заменяет: семейная
    /// подписка на бесплатном плане приезжает как "private".
    is_free: bool,
    /// Суточная норма плана в МБ (0 — нет). Отдаётся отдельно от потолка:
    /// в `traffic_limit_bytes` уже подмешан бонус, а надпись «200 МБ в сутки»
    /// нужно рисовать без него.
    daily_traffic_mb: i32,
    /// Бонусный трафик пользователя в МБ, УЖЕ учтённый в `traffic_limit_bytes`.
    bonus_traffic_mb: i64,
    expires_at: String,
    days_left: i64,
    /// Устройства: использовано (lease за 15 мин) / лимит плана.
    device_used: i64,
    device_limit: i32,
    /// Имя пула узлов (node group через plan_groups), если назначен.
    pool_name: Option<String>,
    relay_country: Option<String>,
    /// Закреплённый relay-узел (`nodes.id`), если пользователь выбрал не
    /// страну целиком, а конкретный релей; `null` — страны достаточно.
    relay_node_id: Option<i64>,
    /// Состояние доступа этой подписки: пускать ли в подключение, почему нет,
    /// когда снова можно и куда идти платить. Считается тем же кодом, что
    /// объясняет отказ на `/sub/{uuid}` и наполняет `GET /subscription`, —
    /// три экрана про одну подписку не могут разойтись, потому что вывод один.
    ///
    /// `status` выше остаётся сырой колонкой базы для уже выпущенных клиентов;
    /// показывать пользователю надо отсюда.
    access: crate::subscription::access::AccessState,
}

/// GET /api/v2/app/subscriptions — список подписок пользователя для UI.
pub async fn list_subscriptions(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let rows = sqlx::query(
        r#"SELECT
                s.id,
                COALESCE(s.subscription_uuid, CONCAT('legacy-', s.id::text)) AS subscription_uuid,
                COALESCE(p.name, 'Unknown Plan') AS plan_name,
                COALESCE(s.status, 'pending') AS status,
                COALESCE(s.used_traffic, 0)::bigint AS used_traffic,
                COALESCE(p.traffic_limit_gb, 0) AS traffic_limit_gb,
                COALESCE(p.daily_traffic_mb, 0) AS daily_traffic_mb,
                COALESCE(p.is_free, FALSE) AS is_free,
                COALESCE(p.device_limit, 0) AS device_limit,
                COALESCE(s.expires_at, s.created_at, CURRENT_TIMESTAMP) AS expires_at,
                s.note,
                s.relay_country,
                s.relay_node_id,
                -- Устройств у АККАУНТА и в том же окне свежести, что у гейта
                -- лимита и экрана «Устройства». Счёт по подписке за 15 минут
                -- показывал «1 из 3» там, где привязано три устройства, и
                -- обнулялся при смене тарифа.
                (
                    SELECT COUNT(*) FROM subscription_device_leases sdl
                    WHERE sdl.user_id = s.user_id
                      AND sdl.last_seen_at > $2
                      AND sdl.last_ip <> '0.0.0.0'
                ) AS device_used,
                (
                    SELECT ng.name FROM plan_groups pg
                    JOIN node_groups ng ON ng.id = pg.group_id
                    WHERE pg.plan_id = p.id
                    ORDER BY ng.id ASC
                    LIMIT 1
                ) AS pool_name
           FROM subscriptions s
           LEFT JOIN plans p ON p.id = s.plan_id
           WHERE s.user_id = $1
           ORDER BY COALESCE(s.created_at, CURRENT_TIMESTAMP) DESC"#,
    )
    .bind(auth.user_id)
    .bind(
        chrono::Utc::now()
            - chrono::Duration::days(crate::services::subscription_service::DEVICE_LEASE_TTL_DAYS),
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    // Бонусный трафик общий на все подписки пользователя — читаем один
    // раз, а не по ходу в базу на каждую строку. При ошибке берём 0: потолок
    // окажется занижен, то есть бар в приложении заполнится раньше срока, а не
    // позже — единственная сторона ошибки, не обещающая трафик, которого нет.
    let bonus_traffic_mb = crate::services::bonus_traffic::balance_mb(&state.pool, auth.user_id)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(
                err = %e,
                user_id = auth.user_id,
                "app: bonus balance read failed, quota ceiling reported without bonus"
            );
            0
        });

    // Бан пользователя и ссылки на оплату одинаковы для всех его подписок —
    // читаем по разу на запрос, а не по разу на строку.
    let banned: bool =
        sqlx::query_scalar("SELECT COALESCE(is_banned, FALSE) FROM users WHERE id = $1")
            .bind(auth.user_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
            .unwrap_or(false);
    let bot_username = state.settings.get_or_default("bot_username", "").await;
    let mini_app_short_name = state
        .settings
        .get_or_default("mini_app_short_name", "")
        .await;
    let pay = crate::subscription::access::pay_links(&bot_username, &mini_app_short_name);

    let now = chrono::Utc::now();
    let now_ts = now.timestamp();
    let subs: Vec<AppSubscription> = rows
        .into_iter()
        .map(|r| {
            let used: i64 = r.try_get("used_traffic").unwrap_or(0);
            let limit_gb: i32 = r.try_get("traffic_limit_gb").unwrap_or(0);
            let daily_mb: i32 = r.try_get("daily_traffic_mb").unwrap_or(0);
            let is_free: bool = r.try_get("is_free").unwrap_or(false);
            let note: Option<String> = r.try_get::<Option<String>, _>("note").ok().flatten();
            let expires: chrono::DateTime<chrono::Utc> = r.try_get("expires_at").unwrap_or(now);

            // private = семейная (выдана родителем), затем free по флагу плана,
            // иначе paid.
            let kind = if note.as_deref() == Some("Family") {
                "private"
            } else if is_free {
                "free"
            } else {
                "paid"
            };

            // Клампим к нулю: отрицательный used_traffic остался в прошлом
            // вместе с онбординг-headroom (миграция 20260831120000), но
            // "израсходовано" в UI не должно уходить в минус ни при каких данных.
            let used = used.max(0);
            let used_gb = used as f64 / 1024.0 / 1024.0 / 1024.0;
            let weekly_free = if daily_mb > 0 {
                Some(daily_mb as f64 * 7.0 / 1024.0)
            } else {
                None
            };

            // Тот же потолок, по которому работает энфорсмент (один хелпер на все
            // пути отображения). Считать его здесь самостоятельно нельзя: именно
            // самодеятельность давала бесплатному юзеру потолок в 7 раз больше
            // реального.
            let traffic_limit_bytes = crate::services::bonus_traffic::plan_quota_limit_bytes(
                is_free,
                limit_gb as i64,
                daily_mb as i64,
                bonus_traffic_mb,
            );
            // "day" ровно тогда, когда потолок взят из суточной колонки, то есть
            // точно по ветке `plan_quota_limit_bytes`: бесплатный план С включённым
            // ограничением (traffic_limit_gb > 0) И с суточной нормой. Во всех
            // остальных случаях — включая безлимит — счётчик накопительный.
            let quota_period = if is_free && limit_gb > 0 && daily_mb > 0 {
                "day"
            } else {
                "total"
            };

            let device_used: i64 = r.try_get("device_used").unwrap_or(0);
            let device_limit: i32 = r.try_get("device_limit").unwrap_or(0);
            let status: String = r.try_get("status").unwrap_or_default();

            let access = crate::subscription::access::compute(
                &crate::subscription::access::AccessFacts {
                    status: status.clone(),
                    banned,
                    expires_at: expires.timestamp(),
                    used_bytes: used,
                    limit_bytes: traffic_limit_bytes,
                    is_free,
                    daily_traffic_mb: daily_mb as i64,
                    device_used,
                    device_limit: device_limit as i64,
                    now: now_ts,
                },
                pay.clone(),
            );

            AppSubscription {
                id: r.try_get("id").unwrap_or(0),
                subscription_uuid: r.try_get("subscription_uuid").unwrap_or_default(),
                plan_name: r.try_get("plan_name").unwrap_or_default(),
                status,
                kind: kind.to_string(),
                used_traffic_bytes: used,
                used_traffic_gb: format!("{:.2}", used_gb),
                traffic_quota_gb: if limit_gb > 0 { Some(limit_gb) } else { None },
                weekly_free_refill_gb: weekly_free,
                traffic_limit_bytes,
                quota_period,
                is_free,
                daily_traffic_mb: daily_mb,
                bonus_traffic_mb,
                expires_at: expires.to_rfc3339(),
                days_left: (expires - now).num_days().max(0),
                device_used,
                device_limit,
                pool_name: r.try_get::<Option<String>, _>("pool_name").ok().flatten(),
                relay_country: r
                    .try_get::<Option<String>, _>("relay_country")
                    .ok()
                    .flatten(),
                relay_node_id: r.try_get::<Option<i64>, _>("relay_node_id").ok().flatten(),
                access,
            }
        })
        .collect();

    Json(subs).into_response()
}

// ============================================================
// RELAYS — доступные relay-страны для пикера
// ============================================================

/// Один relay-узел внутри страны — строка пикера «Вход» в приложении.
#[derive(Serialize)]
struct AppRelayNode {
    /// `nodes.id` — значение для `relay_node_id` в PUT /selection и
    /// `?relay_node_id=` при запросе конфига.
    id: i64,
    /// Имя карточки узла в панели («msk-1»); оператор задаёт его сам.
    name: String,
    /// Город из карточки узла; `null` — оператор не заполнил.
    city: Option<String>,
    /// Средняя нагрузка (CPU+RAM)/2 в процентах по последнему heartbeat —
    /// та же формула, что у `/servers`.
    load_pct: f64,
    /// RTT самой машины до её цели по heartbeat, мс. Это число ПАНЕЛИ, а не
    /// замер с устройства пользователя: замер сквозь релей клиент сделать
    /// пока не может (в clash-теле у релея нет прокси), и приложение обязано
    /// подписывать число именно так.
    latency_ms: Option<i32>,
    /// Приоритет внутри страны из карточки узла (меньше — выше).
    sort_order: i32,
}

#[derive(Serialize)]
struct AppRelay {
    /// ISO-2 код страны (значение для ?relay_country=).
    country_code: String,
    /// Человекочитаемое имя страны (если известно из nodes.country).
    country_name: Option<String>,
    /// Флаг для показа, выведен из ISO-2 алгоритмически.
    flag: String,
    /// Кол-во активных relay-узлов в стране. Остаётся ради уже выпущенных
    /// клиентов, которые ничего кроме страны и счётчика не читают.
    node_count: i64,
    /// Сами узлы, в порядке приоритета (`sort_order`, затем id).
    nodes: Vec<AppRelayNode>,
}

/// Флаг из ISO-2 (региональные индикаторы); мусор — глобус.
fn relay_flag(cc: &str) -> String {
    let chars: Vec<char> = cc.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if chars.len() != 2 {
        return "🌐".to_string();
    }
    let offset = 127397u32;
    match (
        char::from_u32(chars[0].to_ascii_uppercase() as u32 + offset),
        char::from_u32(chars[1].to_ascii_uppercase() as u32 + offset),
    ) {
        (Some(a), Some(b)) => format!("{a}{b}"),
        _ => "🌐".to_string(),
    }
}

/// Группирует relay-узлы по стране. Чистая функция: тестируется без БД.
///
/// Узел без двухбуквенной страны в ответ не попадает: закрепить его нечем
/// (`?relay_country=` матчится по ISO-2), а показывать строку, которую нельзя
/// выбрать, значит обещать вход, которого нет.
fn group_relays(relays: Vec<caramba_db::models::node::Node>) -> Vec<AppRelay> {
    use std::collections::BTreeMap;
    let mut by_cc: BTreeMap<String, AppRelay> = BTreeMap::new();
    for n in relays {
        let cc = match n.country_code.as_deref().map(str::trim) {
            Some(c) if c.len() == 2 => c.to_uppercase(),
            _ => continue,
        };
        let entry = by_cc.entry(cc.clone()).or_insert_with(|| AppRelay {
            flag: relay_flag(&cc),
            country_code: cc.clone(),
            country_name: None,
            node_count: 0,
            nodes: Vec::new(),
        });
        entry.node_count += 1;
        if entry.country_name.is_none() {
            entry.country_name = n.country.clone().filter(|c| !c.trim().is_empty());
        }
        let cpu = n.last_cpu.unwrap_or(0.0);
        let ram = n.last_ram.unwrap_or(0.0);
        entry.nodes.push(AppRelayNode {
            id: n.id,
            name: n.name.clone(),
            city: n.city.clone().filter(|c| !c.trim().is_empty()),
            load_pct: (cpu + ram) / 2.0,
            latency_ms: n.last_latency.map(|l| l as i32),
            sort_order: n.sort_order,
        });
    }
    let mut out: Vec<AppRelay> = by_cc.into_values().collect();
    for country in &mut out {
        country
            .nodes
            .sort_by(|a, b| a.sort_order.cmp(&b.sort_order).then(a.id.cmp(&b.id)));
    }
    out
}

/// GET /api/v2/app/relays — relay-страны с узлами для пикера «Вход».
///
/// Группирует активные relay-узлы по country_code. Значение country_code
/// напрямую подставляется клиентом в ?relay_country= при запросе конфига
/// (см. apps/caramba-sub и panel/subscription.rs — там оно матчится по ISO-2),
/// `nodes[].id` — в `relay_node_id` PUT /selection (закрепить конкретный
/// релей). Спец-значение "none" (отключить relay) клиент добавляет сам.
pub async fn list_relays(
    State(state): State<AppState>,
    axum::Extension(_auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let relays = state
        .infrastructure_service
        .get_active_relay_nodes()
        .await
        .unwrap_or_default();
    Json(group_relays(relays)).into_response()
}

// ============================================================
// SELECTION — выбор exit-ноды и relay-страны из приложения
// ============================================================

/// Разбирает трёхзначное поле запроса: отсутствует / null / значение.
///
/// Голый `Option<T>` этих случаев не различает — serde отдаёт `None` и для
/// пропущенного ключа, и для `null`. Приём: поле объявлено как
/// `Option<Option<T>>` с `#[serde(default, deserialize_with = "de_tristate")]`.
/// `deserialize_with` вызывается ТОЛЬКО когда ключ присутствует, поэтому
/// `null` доезжает как `Some(None)`, а отсутствие ключа даёт `None` из
/// `Default`. Различие несущее: «поле не прислали» обязано сохранить текущее
/// значение, а `null` — сбросить его на выбор оператора.
fn de_tristate<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
pub struct UpdateSelectionRequest {
    /// Exit-нода: отсутствует — не трогаем, null — сброс на выбор оператора,
    /// число — закрепить (валидируется по плану подписки).
    #[serde(default, deserialize_with = "de_tristate")]
    pub node_id: Option<Option<i64>>,
    /// Relay-страна: ISO-2 из GET /relays, литерал "none" (relay выключен)
    /// либо `node:<id>` — закрепить конкретный релей (то же, что
    /// `relay_node_id`, но в одной строке: так выбор едет через клиентов, у
    /// которых в теле есть только это поле).
    #[serde(default, deserialize_with = "de_tristate")]
    pub relay_country: Option<Option<String>>,
    /// Конкретный relay-узел (`nodes.id` из GET /relays): отсутствует — не
    /// трогаем, null — снять закрепление (остаётся страна), число — закрепить;
    /// страна при этом выставляется по узлу.
    #[serde(default, deserialize_with = "de_tristate")]
    pub relay_node_id: Option<Option<i64>>,
}

#[derive(Serialize)]
struct AppSelection {
    ok: bool,
    subscription_id: i64,
    /// Значения перечитаны из БД после UPDATE, а не отражены из запроса:
    /// приложение рисует их сразу, не дожидаясь следующего поллинга.
    node_id: Option<i64>,
    relay_country: Option<String>,
    relay_node_id: Option<i64>,
}

/// Разобранный выбор входа из одного строкового поля `relay_country`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RelayChoice {
    /// Литерал `none` — без релея.
    None,
    /// ISO-2 в верхнем регистре — вся страна.
    Country(String),
    /// `node:<id>` — конкретный релей.
    Node(i64),
}

/// `node:<id>` из строки; всё остальное — не пин.
fn parse_relay_node_pin(raw: &str) -> Option<i64> {
    let v = raw.trim();
    let rest = v
        .strip_prefix("node:")
        .or_else(|| v.strip_prefix("NODE:"))?;
    rest.trim().parse::<i64>().ok().filter(|id| *id > 0)
}

/// Разбирает `relay_country` с учётом формы `node:<id>`; страны и `none`
/// проверяются прежним закрытым словарём, узел — списком активных релеев.
fn parse_relay_choice(
    raw: &str,
    allowed_cc: &[String],
    allowed_nodes: &[i64],
) -> Result<RelayChoice, String> {
    if let Some(id) = parse_relay_node_pin(raw) {
        if allowed_nodes.contains(&id) {
            return Ok(RelayChoice::Node(id));
        }
        return Err(format!(
            "relay node {} is not available. Allowed: {}",
            id,
            allowed_nodes
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    match normalize_relay_country(raw, allowed_cc)?.as_str() {
        "none" => Ok(RelayChoice::None),
        cc => Ok(RelayChoice::Country(cc.to_string())),
    }
}

/// Сводит два поля запроса (`relay_country`, `relay_node_id`) к паре обновлений
/// колонок. Чистая функция: тестируется без БД.
///
/// Правила, по одному на каждую форму намерения:
///   - узел назван (полем или `node:<id>`) — узел закрепляется, страна
///     выставляется по нему: пин без страны разъехался бы с `?relay_country=`
///     старых клиентов и с TMA, которые читают только страну;
///   - страна или `none` названы без узла — это выбор УРОВНЯ страны, и
///     прежний пин снимается: иначе «вся Россия» молча осталась бы «msk-1»;
///   - `relay_node_id: null` — снять только пин, страна остаётся;
///   - ничего не прислали — ничего не трогаем.
fn resolve_relay_update(
    country: Option<Option<RelayChoice>>,
    node: Option<Option<i64>>,
    country_of_node: impl Fn(i64) -> Option<String>,
) -> (Option<Option<String>>, Option<Option<i64>>) {
    let pinned = match (&node, &country) {
        (Some(Some(id)), _) => Some(*id),
        (None, Some(Some(RelayChoice::Node(id)))) => Some(*id),
        _ => None,
    };
    if let Some(id) = pinned {
        return (Some(country_of_node(id)), Some(Some(id)));
    }
    let country_update = match country {
        None => None,
        Some(None) => Some(None),
        Some(Some(RelayChoice::None)) => Some(Some("none".to_string())),
        Some(Some(RelayChoice::Country(cc))) => Some(Some(cc)),
        // Узел здесь невозможен: он перехвачен выше как pinned.
        Some(Some(RelayChoice::Node(_))) => None,
    };
    let node_update = match node {
        Some(None) => Some(None),
        // Явный выбор уровня страны снимает пин.
        None if country_update.is_some() => Some(None),
        _ => None,
    };
    (country_update, node_update)
}

/// Ответ на невалидный ввод: `error` — машиночитаемый код для клиента,
/// `message` — текст, который приложение может показать как есть.
fn selection_error(status: StatusCode, code: &str, message: String) -> axum::response::Response {
    (
        status,
        Json(serde_json::json!({ "error": code, "message": message })),
    )
        .into_response()
}

/// Нормализует relay-страну по закрытому словарю `allowed` (ISO-2 в верхнем
/// регистре — ровно то, что отдаёт `list_relays`).
///
/// Литерал «none» (выключить relay) разрешён всегда и приводится к нижнему
/// регистру: в таком виде его ждёт `subscription.rs`. Отвергать неизвестное
/// значение важно не из педантизма — рассогласованный выбор в БД делает
/// последующую подписанную директиву неразбираемой на клиенте.
/// Чистая функция: тестируется без БД.
fn normalize_relay_country(raw: &str, allowed: &[String]) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("relay_country must not be empty — send null to reset it".to_string());
    }
    if value.eq_ignore_ascii_case("none") {
        return Ok("none".to_string());
    }
    let upper = value.to_ascii_uppercase();
    if allowed.iter().any(|a| a.eq_ignore_ascii_case(&upper)) {
        return Ok(upper);
    }
    let mut vocabulary = vec!["none".to_string()];
    vocabulary.extend(allowed.iter().cloned());
    Err(format!(
        "relay_country '{}' is not available. Allowed: {}",
        value,
        vocabulary.join(", ")
    ))
}

/// Узлы, которые план подписки реально разрешает как exit.
///
/// Зеркалит `node_repo::get_nodes_for_plan` + фолбэк из `subscription.rs`:
/// если план не разрешает ничего (нет plan_groups / пустая группа / plan_id
/// отсутствует), конфиг всё равно отдаёт все активные узлы. Валидатор обязан
/// признавать ровно тот же набор — иначе приложение получит 400 на узел,
/// который сервер потом честно бы отдал.
async fn permitted_exit_node_ids(
    state: &AppState,
    plan_id: Option<i64>,
) -> Result<Vec<i64>, sqlx::Error> {
    if let Some(pid) = plan_id {
        let by_plan: Vec<i64> = sqlx::query_scalar(
            r#"SELECT DISTINCT n.id
                 FROM nodes n
                 JOIN node_group_members ngm ON ngm.node_id = n.id
                 JOIN plan_groups pg ON pg.group_id = ngm.group_id
                WHERE pg.plan_id = $1
                  AND n.status = 'active'
                  AND n.is_relay = FALSE"#,
        )
        .bind(pid)
        .fetch_all(&state.pool)
        .await?;
        if !by_plan.is_empty() {
            return Ok(by_plan);
        }
    }

    sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active' AND is_relay = FALSE")
        .fetch_all(&state.pool)
        .await
}

/// PUT /api/v2/app/subscriptions/{id}/selection — сохранить выбор
/// «через какую страну выхожу» и «через какой relay вхожу».
///
/// До этого эндпоинта у приложения не было легитимного писателя в
/// `subscriptions.node_id` / `relay_country`: TMA-эндпоинт сидит за другим JWT
/// (session_secret по tg_id), а запись на GET-конфига — побочный эффект, а не
/// намерение пользователя.
///
/// Семантика полей — трёхзначная (см. `UpdateSelectionRequest`). Обе колонки
/// пишутся одним UPDATE, результат перечитывается через RETURNING.
pub async fn update_subscription_selection(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(sub_id): Path<i64>,
    Json(payload): Json<UpdateSelectionRequest>,
) -> impl IntoResponse {
    // Владение и существование неразличимы снаружи: чужой id отвечает ровно
    // так же, как несуществующий, чтобы по кодам ответа нельзя было перебрать
    // диапазон id и узнать, какие подписки есть в системе.
    let owned = sqlx::query("SELECT id, plan_id FROM subscriptions WHERE id = $1 AND user_id = $2")
        .bind(sub_id)
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await;

    let row = match owned {
        Ok(Some(r)) => r,
        Ok(None) => {
            return selection_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "Subscription not found".to_string(),
            );
        }
        Err(e) => {
            tracing::error!(err = %e, "app: selection ownership lookup failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let plan_id: Option<i64> = row.try_get::<Option<i64>, _>("plan_id").ok().flatten();

    // --- Валидация node_id по закрытому словарю плана ---
    let node_update: Option<Option<i64>> = match &payload.node_id {
        None => None,
        Some(None) => Some(None),
        Some(Some(requested)) => {
            let permitted = match permitted_exit_node_ids(&state, plan_id).await {
                Ok(ids) => ids,
                Err(e) => {
                    tracing::error!(err = %e, "app: selection node vocabulary lookup failed");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };
            if !permitted.contains(requested) {
                let list = permitted
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return selection_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_node_id",
                    format!(
                        "Server {} is not available on your plan. Allowed: {}",
                        requested,
                        if list.is_empty() {
                            "none"
                        } else {
                            list.as_str()
                        }
                    ),
                );
            }
            Some(Some(*requested))
        }
    };

    // --- Валидация relay_country / relay_node_id по словарю /relays ---
    let relay_touched = payload.relay_country.is_some() || payload.relay_node_id.is_some();
    let relay_nodes = if relay_touched {
        state
            .infrastructure_service
            .get_active_relay_nodes()
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let allowed_cc: Vec<String> = relay_nodes
        .iter()
        .filter_map(|n| match n.country_code.as_deref() {
            Some(c) if c.len() == 2 => Some(c.to_uppercase()),
            _ => None,
        })
        .collect();
    let allowed_nodes: Vec<i64> = relay_nodes.iter().map(|n| n.id).collect();

    let country_choice: Option<Option<RelayChoice>> = match &payload.relay_country {
        None => None,
        Some(None) => Some(None),
        Some(Some(raw)) => match parse_relay_choice(raw, &allowed_cc, &allowed_nodes) {
            Ok(v) => Some(Some(v)),
            Err(msg) => {
                return selection_error(StatusCode::BAD_REQUEST, "invalid_relay_country", msg);
            }
        },
    };
    if let Some(Some(id)) = payload.relay_node_id
        && !allowed_nodes.contains(&id)
    {
        return selection_error(
            StatusCode::BAD_REQUEST,
            "invalid_relay_node_id",
            format!(
                "Relay node {} is not available. Allowed: {}",
                id,
                if allowed_nodes.is_empty() {
                    "none".to_string()
                } else {
                    allowed_nodes
                        .iter()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ),
        );
    }
    let (relay_update, relay_node_update) =
        resolve_relay_update(country_choice, payload.relay_node_id, |id| {
            relay_nodes
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.country_code.as_deref())
                .filter(|c| c.len() == 2)
                .map(|c| c.to_uppercase())
        });

    // Пустое тело — не ошибка: возвращаем текущее состояние, чтобы приложение
    // могло использовать этот же путь как «прочитать выбор».
    let nothing_to_write =
        node_update.is_none() && relay_update.is_none() && relay_node_update.is_none();
    let updated = if nothing_to_write {
        sqlx::query(
            "SELECT node_id, relay_country, relay_node_id FROM subscriptions \
             WHERE id = $1 AND user_id = $2",
        )
        .bind(sub_id)
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await
    } else {
        // Один UPDATE на все колонки: CASE-флаг решает «писать или оставить»,
        // поэтому «сбросить одно и задать другое» не разъезжается на два рейса.
        sqlx::query(
            r#"UPDATE subscriptions
                  SET node_id = CASE WHEN $3::boolean THEN $4::bigint ELSE node_id END,
                      relay_country = CASE WHEN $5::boolean THEN $6::text ELSE relay_country END,
                      relay_node_id = CASE WHEN $7::boolean THEN $8::bigint ELSE relay_node_id END
                WHERE id = $1 AND user_id = $2
            RETURNING node_id, relay_country, relay_node_id"#,
        )
        .bind(sub_id)
        .bind(auth.user_id)
        .bind(node_update.is_some())
        .bind(node_update.flatten())
        .bind(relay_update.is_some())
        .bind(relay_update.clone().flatten())
        .bind(relay_node_update.is_some())
        .bind(relay_node_update.flatten())
        .fetch_optional(&state.pool)
        .await
    };

    let updated = match updated {
        Ok(Some(r)) => r,
        Ok(None) => {
            return selection_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "Subscription not found".to_string(),
            );
        }
        Err(e) => {
            tracing::error!(err = %e, "app: selection update failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Маркер «выбором владеет приложение» — им `subscription.rs` глушит запись
    // на GET-конфига. Ставится и при сбросе в null: именно сброс легче всего
    // потерять, потому что авто-пин узла срабатывает как раз на NULL.
    //
    // Но только когда что-то ДЕЙСТВИТЕЛЬНО записали. Пустое тело этот эндпоинт
    // трактует как чтение текущего выбора, и чтение не должно объявлять
    // владение: маркер живёт 180 дней и глушит единственного писателя
    // relay_country в мини-аппе, так что безобидный GET-подобный вызов
    // выключил бы там выбор релэя на полгода.
    if !nothing_to_write {
        let _ = state
            .redis
            .set(
                &crate::subscription::app_selection_marker_key(sub_id),
                "1",
                crate::subscription::APP_SELECTION_MARKER_TTL_SECS,
            )
            .await;
    }

    Json(AppSelection {
        ok: true,
        subscription_id: sub_id,
        node_id: updated.try_get::<Option<i64>, _>("node_id").ok().flatten(),
        relay_country: updated
            .try_get::<Option<String>, _>("relay_country")
            .ok()
            .flatten(),
        relay_node_id: updated
            .try_get::<Option<i64>, _>("relay_node_id")
            .ok()
            .flatten(),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        AppSubscription, RelayChoice, UpdateSelectionRequest, group_relays,
        normalize_relay_country, parse_relay_choice, parse_relay_node_pin, resolve_relay_update,
    };

    // --- состояние доступа в списке подписок ---

    /// Список подписок обязан нести объяснение вместе с каждой строкой.
    ///
    /// Раньше он отдавал только сырой `status`, и приложение показывало его как
    /// есть — человек читал «throttled» и не понимал ничего. Тест держит две
    /// вещи разом: что поле называется `access` (по нему кодирует приложение) и
    /// что сырой `status` остался рядом для уже выпущенных клиентов.
    #[test]
    fn a_listed_subscription_carries_its_access_state() {
        let facts = crate::subscription::access::AccessFacts {
            status: "throttled".to_string(),
            banned: false,
            expires_at: 253_402_300_799,
            used_bytes: 263 * 1024 * 1024,
            limit_bytes: Some(200 * 1024 * 1024),
            is_free: true,
            daily_traffic_mb: 200,
            device_used: 1,
            device_limit: 1,
            now: 1_788_577_449,
        };
        let row = AppSubscription {
            id: 27,
            subscription_uuid: "feb7e480-314d-4834-8304-220db70684c2".to_string(),
            plan_name: "Free".to_string(),
            status: "throttled".to_string(),
            kind: "free".to_string(),
            used_traffic_bytes: 263 * 1024 * 1024,
            used_traffic_gb: "0.26".to_string(),
            traffic_quota_gb: Some(10),
            weekly_free_refill_gb: Some(1.37),
            traffic_limit_bytes: Some(200 * 1024 * 1024),
            quota_period: "day",
            is_free: true,
            daily_traffic_mb: 200,
            bonus_traffic_mb: 0,
            expires_at: "9999-12-31T23:59:59+00:00".to_string(),
            days_left: 2_912_195,
            device_used: 1,
            device_limit: 1,
            pool_name: None,
            relay_country: None,
            relay_node_id: None,
            access: crate::subscription::access::compute(&facts, None),
        };

        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["access"]["state"], serde_json::json!("quota_exceeded"));
        assert_eq!(v["access"]["rc"], serde_json::json!(3003));
        assert_eq!(v["access"]["may_connect"], serde_json::json!(false));
        assert_eq!(
            v["access"]["resets_at"],
            serde_json::json!("2026-09-06T00:00:00Z")
        );

        // Сырая колонка остаётся — но объяснением она больше не является.
        assert_eq!(v["status"], serde_json::json!("throttled"));
        assert!(
            !v["access"].to_string().contains("throttled"),
            "внутреннее слово уехало в объяснение: {}",
            v["access"]
        );
    }

    // --- трёхзначный разбор: отсутствует / null / значение ---

    #[test]
    fn absent_fields_are_distinct_from_null() {
        let req: UpdateSelectionRequest = serde_json::from_str("{}").unwrap();
        assert!(
            req.node_id.is_none(),
            "missing node_id must mean 'unchanged'"
        );
        assert!(req.relay_country.is_none());
    }

    #[test]
    fn null_means_reset_not_absent() {
        let req: UpdateSelectionRequest =
            serde_json::from_str(r#"{"node_id": null, "relay_country": null}"#).unwrap();
        assert_eq!(req.node_id, Some(None));
        assert_eq!(req.relay_country, Some(None));
    }

    #[test]
    fn values_survive_as_some_some() {
        let req: UpdateSelectionRequest =
            serde_json::from_str(r#"{"node_id": 5, "relay_country": "ru"}"#).unwrap();
        assert_eq!(req.node_id, Some(Some(5)));
        assert_eq!(req.relay_country, Some(Some("ru".to_string())));
    }

    #[test]
    fn one_field_absent_while_the_other_resets() {
        let req: UpdateSelectionRequest =
            serde_json::from_str(r#"{"relay_country": null}"#).unwrap();
        assert!(req.node_id.is_none());
        assert_eq!(req.relay_country, Some(None));
    }

    #[test]
    fn unknown_fields_are_tolerated_for_forward_compatibility() {
        // Клиент, который уже умеет протокол, не должен ловить 400 от панели,
        // которая его ещё не умеет.
        let req: UpdateSelectionRequest =
            serde_json::from_str(r#"{"protocol": "hysteria2", "node_id": 2}"#).unwrap();
        assert_eq!(req.node_id, Some(Some(2)));
    }

    #[test]
    fn wrong_type_is_a_parse_error_not_a_silent_reset() {
        assert!(serde_json::from_str::<UpdateSelectionRequest>(r#"{"node_id": "5"}"#).is_err());
    }

    // --- закрытый словарь relay ---

    #[test]
    fn relay_none_is_always_allowed_and_lowercased() {
        let allowed: Vec<String> = vec![];
        assert_eq!(normalize_relay_country("NONE", &allowed).unwrap(), "none");
        assert_eq!(normalize_relay_country("None", &allowed).unwrap(), "none");
        assert_eq!(normalize_relay_country(" none ", &allowed).unwrap(), "none");
    }

    #[test]
    fn known_country_is_uppercased() {
        let allowed = vec!["RU".to_string()];
        assert_eq!(normalize_relay_country("ru", &allowed).unwrap(), "RU");
        assert_eq!(normalize_relay_country(" RU ", &allowed).unwrap(), "RU");
    }

    #[test]
    fn unknown_country_is_rejected_with_the_vocabulary() {
        let allowed = vec!["RU".to_string()];
        let err = normalize_relay_country("DE", &allowed).unwrap_err();
        assert!(err.contains("DE"), "reason must name the rejected value");
        assert!(
            err.contains("none") && err.contains("RU"),
            "reason must list the vocabulary: {err}"
        );
    }

    // --- закрепление конкретного релея ---

    #[test]
    fn a_node_pin_is_read_from_the_string_form_only_as_node_colon_id() {
        assert_eq!(parse_relay_node_pin("node:12"), Some(12));
        assert_eq!(parse_relay_node_pin(" NODE:7 "), Some(7));
        for junk in ["node:", "node:0", "node:-1", "node:abc", "12", "RU", "none"] {
            assert_eq!(parse_relay_node_pin(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn relay_choice_covers_none_country_and_node() {
        let cc = vec!["RU".to_string()];
        let nodes = vec![12_i64];
        assert_eq!(
            parse_relay_choice("none", &cc, &nodes).unwrap(),
            RelayChoice::None
        );
        assert_eq!(
            parse_relay_choice("ru", &cc, &nodes).unwrap(),
            RelayChoice::Country("RU".into())
        );
        assert_eq!(
            parse_relay_choice("node:12", &cc, &nodes).unwrap(),
            RelayChoice::Node(12)
        );
        let err = parse_relay_choice("node:99", &cc, &nodes).unwrap_err();
        assert!(err.contains("99") && err.contains("12"), "{err}");
    }

    fn cc_of(id: i64) -> Option<String> {
        match id {
            12 | 13 => Some("RU".into()),
            _ => None,
        }
    }

    #[test]
    fn pinning_a_node_also_sets_the_country_it_lives_in() {
        // Полем relay_node_id.
        let (country, node) = resolve_relay_update(None, Some(Some(12)), cc_of);
        assert_eq!(country, Some(Some("RU".into())));
        assert_eq!(node, Some(Some(12)));
        // Строкой node:<id>.
        let (country, node) = resolve_relay_update(Some(Some(RelayChoice::Node(13))), None, cc_of);
        assert_eq!(country, Some(Some("RU".into())));
        assert_eq!(node, Some(Some(13)));
    }

    #[test]
    fn choosing_a_whole_country_or_none_clears_a_previous_node_pin() {
        let (country, node) =
            resolve_relay_update(Some(Some(RelayChoice::Country("RU".into()))), None, cc_of);
        assert_eq!(country, Some(Some("RU".into())));
        assert_eq!(node, Some(None), "«вся страна» снимает пин узла");
        let (country, node) = resolve_relay_update(Some(Some(RelayChoice::None)), None, cc_of);
        assert_eq!(country, Some(Some("none".into())));
        assert_eq!(node, Some(None));
        let (country, node) = resolve_relay_update(Some(None), None, cc_of);
        assert_eq!(country, Some(None));
        assert_eq!(node, Some(None));
    }

    #[test]
    fn unpinning_the_node_alone_keeps_the_country() {
        let (country, node) = resolve_relay_update(None, Some(None), cc_of);
        assert_eq!(country, None, "страну не трогаем");
        assert_eq!(node, Some(None));
    }

    #[test]
    fn an_empty_request_touches_nothing() {
        assert_eq!(resolve_relay_update(None, None, cc_of), (None, None));
    }

    #[test]
    fn selection_request_accepts_relay_node_id_in_all_three_states() {
        let req: UpdateSelectionRequest = serde_json::from_str(r#"{"relay_node_id": 12}"#).unwrap();
        assert_eq!(req.relay_node_id, Some(Some(12)));
        let req: UpdateSelectionRequest =
            serde_json::from_str(r#"{"relay_node_id": null}"#).unwrap();
        assert_eq!(req.relay_node_id, Some(None));
        let req: UpdateSelectionRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(req.relay_node_id, None);
    }

    // --- /relays: страны с узлами ---

    fn relay_node(
        id: i64,
        cc: Option<&str>,
        name: &str,
        city: Option<&str>,
        sort: i32,
    ) -> caramba_db::models::node::Node {
        let mut n: caramba_db::models::node::Node = serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "ip": format!("203.0.113.{id}"),
            "status": "active",
            "vpn_port": 443,
            "created_at": "2026-09-01T00:00:00Z",
            "auto_configure": false,
            "is_enabled": true,
            "sort_order": sort,
            "config_qos_enabled": false,
            "config_block_torrent": false,
            "config_block_ads": false,
            "config_block_porn": false,
            "max_ram": 0,
            "cpu_cores": 0,
            "speed_limit_mbps": 0,
            "max_users": 0,
            "current_speed_mbps": 0,
            "total_ingress": 0,
            "total_egress": 0,
            "uptime": 0,
            "last_session_ingress": 0,
            "last_session_egress": 0,
            "is_relay": true,
            "node_type": "relay",
            "pending_log_collection": false,
        }))
        .expect("узел-заглушка");
        n.country_code = cc.map(str::to_string);
        n.country = cc.map(|_| "Russia".to_string());
        n.city = city.map(str::to_string);
        n.last_cpu = Some(20.0);
        n.last_ram = Some(40.0);
        n.last_latency = Some(31.0);
        n
    }

    /// Два релея одной страны отдаются УЗЛАМИ с id/именем/городом, в порядке
    /// приоритета; `node_count` остаётся для старых клиентов. Узел без
    /// страны в ответ не попадает — закрепить его нечем.
    #[test]
    fn relays_are_grouped_by_country_and_list_their_nodes_in_priority_order() {
        let out = group_relays(vec![
            relay_node(11, Some("ru"), "spb-1", Some("Saint Petersburg"), 20),
            relay_node(10, Some("RU"), "msk-1", Some("Moscow"), 10),
            relay_node(30, None, "orphan", None, 0),
        ]);
        assert_eq!(out.len(), 1, "страна одна: RU; узел без страны выброшен");
        let ru = &out[0];
        assert_eq!(ru.country_code, "RU");
        assert_eq!(ru.flag, "🇷🇺");
        assert_eq!(ru.node_count, 2);
        assert_eq!(ru.country_name.as_deref(), Some("Russia"));
        let ids: Vec<i64> = ru.nodes.iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![10, 11], "по sort_order, затем по id");
        assert_eq!(ru.nodes[0].name, "msk-1");
        assert_eq!(ru.nodes[0].city.as_deref(), Some("Moscow"));
        assert_eq!(ru.nodes[0].latency_ms, Some(31));
        assert!((ru.nodes[0].load_pct - 30.0).abs() < f64::EPSILON);

        // Форма на проводе: старые поля на месте, новые рядом.
        let json = serde_json::to_value(&out).unwrap();
        assert!(json[0]["node_count"].is_number());
        assert_eq!(json[0]["nodes"][1]["id"], 11);
    }

    #[test]
    fn empty_string_is_rejected_rather_than_treated_as_reset() {
        let allowed = vec!["RU".to_string()];
        assert!(normalize_relay_country("", &allowed).is_err());
        assert!(normalize_relay_country("   ", &allowed).is_err());
    }

    #[test]
    fn free_text_never_reaches_the_database() {
        let allowed = vec!["RU".to_string(), "CA".to_string()];
        for bad in ["RUS", "russia", "R", "'; DROP TABLE nodes;--"] {
            assert!(
                normalize_relay_country(bad, &allowed).is_err(),
                "{bad} must be rejected"
            );
        }
    }
}
