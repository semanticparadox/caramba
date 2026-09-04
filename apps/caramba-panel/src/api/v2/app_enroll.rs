//! Коды вовлечения (enrollment) для standalone-приложения Caramba Connect:
//! публичная валидация, выпуск ссылки-приглашения и её погашение.
//!
//! Здесь три вещи вокруг ОДНОЙ таблицы `enrollment_codes`:
//!   * GET  /api/v2/app/enroll/{code}   — read-only проверка пригодности кода;
//!   * POST /api/v2/app/enroll/redeem   — погашение кода из ссылки `caramba://`
//!                                        в сессию уже существующего аккаунта;
//!   * [`issue_connect_link`]           — выпуск такой ссылки (зовёт бот).
//!
//! ДВА РАЗНЫХ СМЫСЛА У ОДНОЙ ТАБЛИЦЫ, и их нельзя путать:
//!   * реферальный enroll-код — списывается при СОЗДАНИИ аккаунта
//!     (`store_service::redeem_enrollment_code`), проставляет referrer_id и
//!     начисляет реферальные бонусы;
//!   * код приглашения устройства — списывается здесь и НЕ создаёт аккаунт: он
//!     привязывает новое устройство к аккаунту, который уже есть у человека в
//!     боте. Поэтому `redeem_enrollment_code` здесь не переиспользуется: он
//!     сделан для другого события. Общими остаются таблица и предикат
//!     валидности, а не побочные эффекты.
//!
//! Различаются они пространством имён в колонке `code`: приглашение устройства
//! хранится как `lnk_<32 hex>` (см. [`crate::connect_link::DB_CODE_PREFIX`]),
//! реферальный код — как угодно ещё. Пересечься они не могут, поэтому погашение
//! одного никогда не спишет другой.
//!
//! Контракт B: GET /api/v2/app/enroll/{code} -> JSON
//!   { valid: bool, reason?: string, panel_name?: string, onboarding_traffic_mb: i64 }
//!
//! Эндпоинт ПУБЛИЧНЫЙ (без JWT): клиент вызывает его при открытии диплинка
//! carambaconnect://enroll, ДО регистрации/логина. Чисто READ-ONLY — НЕ списывает
//! использование кода (consume происходит при создании аккаунта, не здесь).
//!
//! `onboarding_traffic_mb` — лимит бесплатного плана в МБ (см. `read_onboarding_mb`).
//!
//! PII-инвариант: ответ содержит ТОЛЬКО признак валидности, обобщённую причину,
//! имя панели и число онбординг-трафика. Никогда — личность пригласившего, email,
//! used_count/max_uses или иные детали кода.

use crate::AppState;
use crate::api::v2::app_auth::{TokenPair, grant_free_plan_on_signup, issue_session};
use crate::connect_link::{self, ConnectProfile};
use anyhow::{Context, Result, anyhow};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

/// Сколько живёт ссылка-приглашение. Тридцать минут — компромисс между
/// «человек успел дойти до телефона» и «утёкшая переписка быстро протухла».
/// Больше, чем у 6-значного кода входа (5 минут), потому что ссылку открывают
/// не сразу, а когда доберутся до устройства.
const LINK_TTL_MINUTES: i64 = 30;

/// Ответ валидации кода вовлечения. `reason` присутствует только при valid=false
/// и содержит обобщённую причину без утечки деталей.
#[derive(Serialize)]
pub struct EnrollValidationResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_name: Option<String>,
    pub onboarding_traffic_mb: i64,
}

/// Сколько трафика (МБ) человек получит, зарегистрировавшись по этому коду.
///
/// Раньше это была глобальная настройка `onboarding_traffic_mb`, начислявшаяся
/// в обход подписки. Теперь трафик приходит только от плана, поэтому число берётся
/// у самого бесплатного плана — того, на который регистрация и посадит человека.
/// Имя поля в ответе сохранено: это публичный контракт, а смысл его не изменился.
///
/// Бесплатный план не настроен — `0`, как и раньше при выключенной настройке.
/// Ошибка БД тоже даёт `0`: экран приветствия не повод ронять валидацию кода.
async fn read_onboarding_mb(state: &AppState) -> i64 {
    let gb: Option<i32> = sqlx::query_scalar(
        "SELECT COALESCE(traffic_limit_gb, 0) FROM plans \
         WHERE is_free = TRUE AND is_active = TRUE LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    i64::from(gb.unwrap_or(0)).max(0).saturating_mul(1024)
}

/// Имя панели для раннего брендинга на клиенте. Берём настройку brand_name; если
/// она пуста или не задана — отдаём дефолтный бренд "Caramba Connect" (rule 1:
/// user-facing default brand, НЕ exarobot).
async fn read_panel_name(state: &AppState) -> String {
    let name = state
        .settings
        .get_or_default("brand_name", "Caramba Connect")
        .await;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Caramba Connect".to_string()
    } else {
        trimmed.to_string()
    }
}

/// GET /api/v2/app/enroll/{code} — публичная валидация кода вовлечения.
///
/// Возвращает 200 в обоих случаях (valid true/false) — это не аутентификация, а
/// проверка пригодности кода для дальнейшего флоу register/login. Причины
/// обобщённые ("invalid" — без различения expired/exhausted/unknown), чтобы не
/// раскрывать состояние кодов перебором.
pub async fn validate_enroll_code(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let code = code.trim();

    // Базовая защита от мусора: пустой код не валиден без удара по БД.
    if code.is_empty() {
        let onboarding_traffic_mb = read_onboarding_mb(&state).await;
        let panel_name = read_panel_name(&state).await;
        return Json(EnrollValidationResponse {
            valid: false,
            reason: Some("invalid".to_string()),
            panel_name: Some(panel_name),
            onboarding_traffic_mb,
        })
        .into_response();
    }

    let onboarding_traffic_mb = read_onboarding_mb(&state).await;
    let panel_name = read_panel_name(&state).await;

    match state
        .store_service
        .validate_enrollment_code(&storage_code(code))
        .await
    {
        Ok(Some(_)) => Json(EnrollValidationResponse {
            valid: true,
            reason: None,
            panel_name: Some(panel_name),
            onboarding_traffic_mb,
        })
        .into_response(),
        Ok(None) => Json(EnrollValidationResponse {
            valid: false,
            // Обобщённо: не различаем expired / exhausted / unknown во избежание
            // утечки состояния кода.
            reason: Some("invalid".to_string()),
            panel_name: Some(panel_name),
            onboarding_traffic_mb,
        })
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "enroll validation: db lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// =============================================================================
// Ссылка-приглашение caramba://connect
// =============================================================================

/// Как код выглядит в колонке `enrollment_codes.code`.
///
/// Проводной код приглашения устройства — 32 hex-символа; в базе он лежит с
/// префиксом пространства имён. Всё остальное считаем реферальным кодом и ищем
/// как есть. Одна функция на оба чтения (валидация и погашение), чтобы они не
/// разъехались: разъехавшись, они дали бы «код валиден, но не гасится».
fn storage_code(code: &str) -> String {
    if connect_link::is_wire_code(code) {
        connect_link::db_code(code)
    } else {
        code.to_string()
    }
}

/// Origin коннектора — адрес, по которому приложение будет ходить в API.
///
/// Это НЕ subscription_domain: подписку может отдавать отдельный домен, а API
/// живёт на панели. Ошибка вместо догадки намеренная — ссылка с неверным origin
/// молча приведёт приложение не туда, и человек увидит непонятный сбой сети
/// вместо понятного «оператор не настроил панель».
async fn connector_origin(state: &AppState) -> Result<String> {
    let configured = state.settings.get_or_default("panel_url", "").await;
    let raw = if configured.trim().is_empty() {
        std::env::var("PANEL_URL").unwrap_or_default()
    } else {
        configured
    };
    let raw = raw.trim().trim_end_matches('/').to_string();
    if raw.is_empty() || raw == "localhost" {
        return Err(anyhow!(
            "panel_url is not configured; a connect link would point nowhere"
        ));
    }
    Ok(if raw.starts_with("http") {
        raw
    } else {
        format!("https://{raw}")
    })
}

/// База для ссылки на подписку. Повторяет порядок, по которому её собирают бот
/// и `api::v2::app`: subscription_domain важнее panel_url, потому что подписку
/// намеренно уводят на отдельный домен.
async fn subscription_base(state: &AppState) -> Option<String> {
    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let raw = if sub_domain.trim().is_empty() {
        state.settings.get_or_default("panel_url", "").await
    } else {
        sub_domain
    };
    let raw = raw.trim().trim_end_matches('/').to_string();
    if raw.is_empty() || raw == "localhost" {
        return None;
    }
    Some(if raw.starts_with("http") {
        raw
    } else {
        format!("https://{raw}")
    })
}

/// Выпускает ссылку `caramba://connect?d=...` для существующего пользователя.
///
/// Вызывает бот, отдавая ссылку рядом с подпиской. Что происходит:
///   1. Прежние невыданные приглашения этого человека помечаются истёкшими —
///      живым остаётся ровно одно, как и у кода входа. Не DELETE: строка нужна,
///      чтобы потом можно было увидеть, что приглашение выпускалось.
///   2. Новый 128-битный секрет пишется в `enrollment_codes` с max_uses = 1.
///      ИМЕННО 1: предикат погашения — `used_count < max_uses`, поэтому строка
///      с max_uses = 0 не гасится НИКОГДА (0 < 0 ложно). Значение по умолчанию
///      в схеме тоже 1, но здесь оно проставляется явно — молчаливая нулевая
///      строка выглядела бы как рабочая и не работала бы.
///   3. `inviter_user_id` — это АККАУНТ-ЦЕЛЬ приглашения: погашение выдаст
///      сессию именно ему. Для реферального кода то же поле значит «кто привёл»;
///      пересечения нет, потому что пространства имён кодов не пересекаются.
///
/// Ошибка означает, что ссылку выдавать нельзя (не настроен panel_url, сбой
/// БД). Заглушку вместо ссылки не возвращаем: неработающая ссылка хуже, чем
/// честное сообщение оператору.
pub(crate) async fn issue_connect_link(state: &AppState, user_id: i64) -> Result<String> {
    let origin = connector_origin(state).await?;
    let operator_name = read_panel_name(state).await;

    let code = connect_link::new_code();
    let hex_code = connect_link::code_hex(&code);
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(LINK_TTL_MINUTES);

    let mut tx = state.pool.begin().await.context("enroll link: begin tx")?;

    sqlx::query(
        "UPDATE enrollment_codes SET expires_at = CURRENT_TIMESTAMP \
         WHERE inviter_user_id = $1 \
           AND left(code, $2) = $3 \
           AND used_count < max_uses \
           AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)",
    )
    .bind(user_id)
    // Сравнение по префиксу через left(), а НЕ через LIKE: в LIKE символ '_'
    // — одиночный джокер, и шаблон 'lnk_%' совпал бы ещё и с 'lnkX...'. Здесь
    // это пока безобидно, но истечение чужих кодов — не та ошибка, которую
    // хочется найти постфактум.
    .bind(connect_link::DB_CODE_PREFIX.len() as i32)
    .bind(connect_link::DB_CODE_PREFIX)
    .execute(&mut *tx)
    .await
    .context("enroll link: expiring previous invites")?;

    sqlx::query(
        "INSERT INTO enrollment_codes (code, inviter_user_id, max_uses, used_count, expires_at) \
         VALUES ($1, $2, 1, 0, $3)",
    )
    .bind(connect_link::db_code(&hex_code))
    .bind(user_id)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .context("enroll link: inserting invite")?;

    tx.commit().await.context("enroll link: commit")?;

    // Идентификатор корневого ключа кладём, только если церемония была. Пустой
    // ключ не кладём вовсе: приложение обязано отличать «протокол не включён»
    // от «включён с нулевым ключом».
    let root_key_id = match crate::csm::keys::load_identity(&state.pool).await {
        Ok(identity) => identity.map(|i| i.root_kid),
        Err(e) => {
            // Испорченная строка ключа не повод не пустить человека в приложение:
            // ссылка без ключа валидна, просто без раннего доверия к протоколу.
            tracing::warn!(err = %e, "enroll link: root key unreadable, issuing link without kid");
            None
        }
    };

    // Выпуск фаллибелен: имя оператора и адрес панели приходят из настроек, где
    // их пишет человек, и слишком длинное или непечатное значение дало бы
    // ссылку, которую приложение ОБЯЗАНО отвергнуть (см. connect_link.rs).
    // Пусть лучше оператор увидит ошибку здесь, чем пользователь — тупик.
    ConnectProfile {
        connector_origin: origin,
        code,
        operator_name,
        root_key_id,
        expires_at: expires_at.timestamp().max(0) as u64,
    }
    .to_link()
    .context("enroll link: profile outside the CSM strict profile")
}

/// Тело запроса погашения. `code` — 32 hex-символа, то есть поле 2 из ссылки,
/// записанное в hex. Сырые байты по сети не ходят.
#[derive(Deserialize)]
pub struct RedeemRequest {
    pub code: String,
}

/// Ответ погашения: пара токенов плюс то, что нужно, чтобы сразу стать клиентом
/// панели, а не «залогиненным никем».
///
/// `subscription_url` нуллабелен намеренно. Аккаунт без пригодной подписки —
/// реальное состояние (бесплатный план не настроен оператором), и приложение
/// обязано показать его как «неизвестно, почему», а не подставить выдуманный
/// адрес. Поэтому рядом лежит `subscription_reason` с машиночитаемой причиной.
#[derive(Serialize)]
pub struct RedeemResponse {
    #[serde(flatten)]
    pub session: TokenPair,
    /// Ссылка на подписку — то, что приложение скармливает ядру.
    pub subscription_url: Option<String>,
    /// UUID подписки отдельно: он же ключ подписки в API панели.
    pub subscription_uuid: Option<String>,
    /// Статус подписки как он есть в БД (active / pending / throttled / expired).
    pub subscription_status: Option<String>,
    /// Почему `subscription_url` пуст. Присутствует ТОЛЬКО когда он пуст.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_reason: Option<String>,
    /// Имя оператора — то же, что показывал экран подтверждения ссылки.
    pub panel_name: String,
}

/// POST /api/v2/app/enroll/redeem — превращает код из ссылки в сессию.
///
/// Это единственный путь, по которому человек, пришедший ТОЛЬКО со ссылкой,
/// становится подключённым к панели клиентом: у него нет ни email с паролем, ни
/// Telegram-контекста внутри приложения. До этого эндпоинта такого пути не было
/// вообще — `register`/`login/telegram` умеют списывать enroll-код лишь как
/// побочный эффект создания аккаунта, а аккаунт у человека уже есть.
///
/// Ответы намеренно единообразны (400 «Invalid or expired invite») на всех
/// промахах: различать «нет такого» и «уже погашен» значит рассказывать о
/// состоянии чужих кодов. Rate limit не ставим: пространство кода 2^128, перебор
/// не является угрозой — в отличие от 6-значного кода входа, где лимит есть.
pub async fn redeem_connect_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RedeemRequest>,
) -> impl IntoResponse {
    let code = payload.code.trim().to_ascii_lowercase();

    // Отсекаем мусор до удара по БД: приглашение устройства — всегда 32 hex.
    if !connect_link::is_wire_code(&code) {
        return (StatusCode::BAD_REQUEST, "Invalid or expired invite").into_response();
    }

    let user_id = match consume_connect_code(&state, &code).await {
        Ok(Some(id)) => id,
        Ok(None) => return (StatusCode::BAD_REQUEST, "Invalid or expired invite").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "enroll redeem: db failure");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Подписка — часть контракта ответа. Сначала ЧИТАЕМ, и только если её нет
    // совсем, зовём выдачу бесплатного плана.
    //
    // Порядок принципиален, а не косметический. `grant_free_plan_on_signup`
    // публикует план на ноды БЕЗУСЛОВНО: когда выдавать нечего, он всё равно
    // достаёт plan_id запасным запросом и вызывает notify_nodes_for_plans, то
    // есть перевыпуск конфига и перезапуск sing-box. Вызвать его на каждом
    // погашении значило бы дёргать весь флот от того, что человек подключил
    // телефон. Ровно эту ловушку описывает `command::ensure_free_plan_for_active_bot_user`:
    // идемпотентности на уровне БД здесь недостаточно.
    let mut view = subscription_view(&state, user_id).await;
    if view.reason == Some(NO_SUBSCRIPTION) {
        grant_free_plan_on_signup(&state, user_id).await;
        view = subscription_view(&state, user_id).await;
    }

    let subscription_url = match (&view.uuid, subscription_base(&state).await) {
        (Some(uuid), Some(base)) => Some(format!("{base}/sub/{uuid}")),
        _ => None,
    };
    // Домен не настроен — единственная причина, по которой uuid есть, а ссылки нет.
    let subscription_reason = view.reason.map(str::to_string).or_else(|| {
        (view.uuid.is_some() && subscription_url.is_none())
            .then(|| "subscription_domain_not_configured".to_string())
    });

    let session = match issue_session(&state, user_id, &headers).await {
        Ok(pair) => pair,
        Err(s) => return s.into_response(),
    };

    Json(RedeemResponse {
        session,
        subscription_url,
        subscription_uuid: view.uuid,
        subscription_status: view.status,
        subscription_reason,
        panel_name: read_panel_name(&state).await,
    })
    .into_response()
}

/// Машиночитаемая причина «подписки нет вообще». Отдельной константой, потому
/// что по ней принимается решение о самолечении, а не только текст ответа.
const NO_SUBSCRIPTION: &str = "no_subscription_on_account";

/// Что известно о подписке аккаунта на момент ответа.
struct SubscriptionView {
    uuid: Option<String>,
    status: Option<String>,
    /// Почему `uuid` пуст. `None` означает, что подписка есть и пригодна.
    reason: Option<&'static str>,
}

/// Читает подписку, которую приложение получит и от `GET /api/v2/app/subscription`.
///
/// Выбор «активная, иначе первая» скопирован оттуда намеренно: разойдясь, эти
/// две точки отдали бы приложению разные подписки — одну при подключении,
/// другую при первом же обновлении профиля.
async fn subscription_view(state: &AppState, user_id: i64) -> SubscriptionView {
    let subs = match state
        .subscription_service
        .get_user_subscriptions(user_id)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            // Сессию всё равно выдадим: токены заслужены погашением кода, а сбой
            // чтения подписки приложение переживёт и повторит запрос.
            tracing::error!(err = %e, user_id, "enroll redeem: subscription lookup failed");
            return SubscriptionView {
                uuid: None,
                status: None,
                reason: Some("subscription_lookup_failed"),
            };
        }
    };

    let picked = subs
        .iter()
        .find(|s| s.sub.status == "active")
        .or_else(|| subs.first());

    match picked {
        Some(s) if !s.sub.subscription_uuid.trim().is_empty() => SubscriptionView {
            uuid: Some(s.sub.subscription_uuid.clone()),
            status: Some(s.sub.status.clone()),
            reason: None,
        },
        // Строка есть, но uuid пуст — в конфиг ноды такая подписка не попадает,
        // подключиться по ней нельзя. Отдаём неизвестность с причиной, а не
        // собранную из ничего ссылку.
        Some(s) => SubscriptionView {
            uuid: None,
            status: Some(s.sub.status.clone()),
            reason: Some("subscription_has_no_uuid"),
        },
        None => SubscriptionView {
            uuid: None,
            status: None,
            reason: Some(NO_SUBSCRIPTION),
        },
    }
}

/// Списывает код приглашения устройства и возвращает аккаунт-цель.
///
/// Повторяет проверенный порядок из `store_service::redeem_enrollment_code_in_tx`
/// (SELECT FOR UPDATE по предикату валидности, затем условный инкремент), но
/// БЕЗ его побочных эффектов: ни referrer_id, ни реферальных бонусов здесь быть
/// не должно — аккаунт не создаётся, человек привязывает устройство к себе.
///
/// `Ok(None)` — код не существует, истёк, уже погашен или это не приглашение
/// устройства (нет аккаунта-цели). Вызывающий отвечает на все эти случаи
/// одинаково.
async fn consume_connect_code(state: &AppState, wire_code: &str) -> Result<Option<i64>> {
    let mut tx = state.pool.begin().await?;

    let row: Option<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, inviter_user_id FROM enrollment_codes \
         WHERE code = $1 \
           AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) \
           AND used_count < max_uses \
         FOR UPDATE",
    )
    .bind(connect_link::db_code(wire_code))
    .fetch_optional(&mut *tx)
    .await?;

    let Some((id, inviter_user_id)) = row else {
        tx.rollback().await.ok();
        return Ok(None);
    };

    // Приглашение без аккаунта-цели привязывать не к чему. Выпуск такое не
    // создаёт, но чужая строка в том же пространстве имён не должна выдавать
    // сессию непонятно кому.
    let Some(target_user_id) = inviter_user_id else {
        tx.rollback().await.ok();
        tracing::warn!(code_id = id, "enroll redeem: invite has no target account");
        return Ok(None);
    };

    // Условный инкремент под блокировкой: параллельное погашение того же кода
    // увидит rows_affected = 0 и получит отказ. Одноразовость держится здесь.
    let res = sqlx::query(
        "UPDATE enrollment_codes SET used_count = used_count + 1 \
         WHERE id = $1 AND used_count < max_uses",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        tx.rollback().await.ok();
        return Ok(None);
    }

    tx.commit().await?;
    Ok(Some(target_user_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Статический `/enroll/redeem` и параметрический `/enroll/{code}` живут
    /// рядом. Конфликт путей axum обнаруживает В МОМЕНТ РЕГИСТРАЦИИ и роняет
    /// процесс при старте панели — то есть в проде, а не в тесте эндпоинта.
    /// Поэтому проверяем именно форму путей, а не поведение хендлеров.
    #[test]
    fn static_and_param_enroll_routes_coexist() {
        let _: axum::Router<()> = axum::Router::new()
            .route("/enroll/{code}", axum::routing::get(|| async {}))
            .route("/enroll/redeem", axum::routing::post(|| async {}));
    }

    #[test]
    fn wire_codes_are_namespaced_and_others_are_not() {
        let hex = "0123456789abcdef0123456789abcdef";
        assert_eq!(storage_code(hex), format!("lnk_{hex}"));
        // Реферальный код проходит как есть — иначе старые коды перестали бы
        // находиться.
        assert_eq!(storage_code("FRIEND2026"), "FRIEND2026");
        // Уже неймспейснутая строка не получает второй префикс.
        assert_eq!(storage_code(&format!("lnk_{hex}")), format!("lnk_{hex}"));
    }
}
