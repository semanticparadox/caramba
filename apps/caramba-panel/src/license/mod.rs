//! Лицензионный модуль Caramba Connect (P4: активация + грейс + энфорсмент).
//!
//! P3 (seam) давал чистый дефолт по settings-ключу. P4 заменяет ИСТОЧНИК тира на
//! активированное, проверенное по ed25519-подписи состояние лицензии, сохраняя
//! ПУБЛИЧНУЮ сигнатуру [`effective_tier`] (`async fn effective_tier(&AppState)
//! -> LicenseTier`), чтобы branding-эндпоинт и прочие вызывающие не менялись.
//!
//! Контракт D:
//!   * читаем CARAMBA_LICENSE_KEY / CARAMBA_INSTANCE_ID /
//!     CARAMBA_LICENSE_SERVER_URL / CARAMBA_LICENSE_PUBKEY из env;
//!   * на старте зовём активацию, проверяем подпись pubkey'ем, кэшируем
//!     в таблицу `license_state` (одна строка);
//!   * пере-проверяем каждые 12-24 ч;
//!   * без ключа -> тир Free, без обращения к серверу;
//!   * ОФЛАЙН-ГРЕЙС: если активация не удалась (сервер недоступен), но есть
//!     валидный кэш и `now < last_verified_at + 14 дней` — держим кэшированный
//!     тир; после 14 дней ИЛИ если подпись не верифицируется ИЛИ `expires_at`
//!     прошёл — SOFT degrade к лимитам Free ТОЛЬКО для новых привилегированных
//!     действий. Существующих пользователей/трафик/активные подписки НЕ трогаем.
//!
//! Контракт E (энфорсмент) — точечные гейты на путях создания ноды/юзера,
//! биллинга и выдачи конфигов. Гейты возвращают ясную ошибку, никогда не паникуют
//! и никогда не блокируют уже выданных пользователей.
//!
//! Авторитетные типы лицензии берём из `caramba_shared::license` (контракт B):
//! `LicenseLimits` использует i64, `max_*==0` означает «без лимита» (Pro).
//! Локальные [`Limits`]/[`LicenseTier`]/[`limits_for`] сохранены ради
//! неизменного P3-вызова в branding-эндпоинте.

pub mod activation;

use crate::AppState;
use caramba_shared::license::LicenseLimits;

/// Грейс-окно офлайна по умолчанию: 14 дней с момента последней удачной
/// верификации, в течение которых недоступность сервера не понижает тир.
pub const GRACE_DAYS: i64 = 14;

/// Лицензионный тир инстанса панели. По умолчанию — [`LicenseTier::Free`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseTier {
    /// Бесплатный тир: малые лимиты, без брендинга/биллинга, с апстрим-рекламой.
    Free,
    /// Pro-тир: высокие лимиты, кастомный бренд и биллинг, без апстрим-рекламы.
    Pro,
}

/// Лимиты и фиче-флаги одного тира (локальное P3-представление с u32).
///
/// Branding-эндпоинт читает [`Limits`] через [`limits_for`]/[`effective_tier`]
/// без изменений. Для энфорсмента используем авторитетные i64-лимиты из
/// `caramba_shared` напрямую (см. [`effective_limits`]), чтобы не терять
/// «0 = без лимита» и не упираться в u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Максимум нод (`0` трактуется вызывающими как «без явного лимита»).
    pub max_nodes: u32,
    /// Максимум пользователей (`0` трактуется как «без явного лимита»).
    pub max_users: u32,
    /// Доступен ли биллинг конечных пользователей.
    pub end_user_billing: bool,
    /// Доступен ли кастомный бренд оператора (гейт для branding-эндпоинта).
    pub branding: bool,
    /// Показывается ли реклама/upsell апстрима (Caramba Connect).
    pub upstream_ads: bool,
    /// Требуется ли ручное подтверждение пользователей.
    pub manual_approval: bool,
}

/// Таблица лимитов по тиру (локальное P3-представление). Значения совпадают с
/// авторитетной матрицей `caramba_shared` и клиентской матрицей.
pub fn limits_for(tier: LicenseTier) -> Limits {
    match tier {
        LicenseTier::Free => Limits {
            max_nodes: 2,
            max_users: 100,
            end_user_billing: false,
            branding: false,
            upstream_ads: true,
            manual_approval: true,
        },
        LicenseTier::Pro => Limits {
            max_nodes: 1000,
            max_users: 0,
            end_user_billing: true,
            branding: true,
            upstream_ads: false,
            manual_approval: false,
        },
    }
}

/// Авторитетные лимиты Free-тира (i64) из `caramba_shared`. Это «дно» SOFT
/// degrade: когда лицензия не верифицируется, истекла или вышла из грейса, новые
/// привилегированные действия гейтятся по этим значениям.
pub fn free_limits() -> LicenseLimits {
    LicenseLimits {
        max_nodes: 2,
        max_users: 100,
        end_user_billing: false,
        branding: false,
        upstream_ads: true,
        manual_approval: true,
    }
}

/// Маппит локальный P3-тир в авторитетные i64-лимиты `caramba_shared`.
pub fn shared_limits_for(tier: LicenseTier) -> LicenseLimits {
    match tier {
        LicenseTier::Free => free_limits(),
        LicenseTier::Pro => LicenseLimits {
            max_nodes: 1000,
            max_users: 0,
            end_user_billing: true,
            branding: true,
            upstream_ads: false,
            manual_approval: false,
        },
    }
}

/// Локальный P3-`Limits` из авторитетных i64-лимитов. `i64::max` обрезается до
/// `u32::MAX`, «0 = без лимита» сохраняется как `0`.
fn local_limits_from(limits: &LicenseLimits) -> Limits {
    fn clamp_u32(v: i64) -> u32 {
        if v <= 0 {
            0
        } else if v >= u32::MAX as i64 {
            u32::MAX
        } else {
            v as u32
        }
    }
    Limits {
        max_nodes: clamp_u32(limits.max_nodes),
        max_users: clamp_u32(limits.max_users),
        end_user_billing: limits.end_user_billing,
        branding: limits.branding,
        upstream_ads: limits.upstream_ads,
        manual_approval: limits.manual_approval,
    }
}

/// Кэшированное, проверенное по подписи состояние лицензии. Живёт в AppState
/// за `Arc<RwLock<Option<LicenseState>>>` и зеркалит строку `license_state`.
#[derive(Debug, Clone)]
pub struct LicenseState {
    /// Верифицированный тир.
    pub tier: LicenseTier,
    /// Верифицированные авторитетные лимиты (i64).
    pub limits: LicenseLimits,
    /// Истечение лицензии из подписанного ответа сервера.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Момент последней удачной верификации — якорь грейс-окна.
    pub last_verified_at: chrono::DateTime<chrono::Utc>,
}

impl LicenseState {
    /// Истекла ли лицензия по `expires_at` относительно текущего времени.
    pub fn is_expired(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        now >= self.expires_at
    }

    /// В пределах ли грейс-окна (`now < last_verified_at + GRACE_DAYS`).
    pub fn within_grace(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        now < self.last_verified_at + chrono::Duration::days(GRACE_DAYS)
    }
}

/// Сводит кэшированное состояние к АВТОРИТЕТНЫМ лимитам для новых
/// привилегированных действий, применяя грейс/истечение (SOFT degrade).
///
/// Логика «дна»:
///   * нет ключа/нет кэша -> Free;
///   * `expires_at` прошёл -> Free (истечение НЕ маскируется грейсом);
///   * вне грейса (сервер давно недоступен) -> Free;
///   * иначе -> кэшированные лимиты тира.
///
/// Это касается ТОЛЬКО гейтов новых действий. Существующие пользователи, трафик
/// и активные подписки никогда не понижаются и не отключаются.
fn effective_limits_from(state: &Option<LicenseState>) -> LicenseLimits {
    let now = chrono::Utc::now();
    match state {
        None => free_limits(),
        Some(s) => {
            if s.is_expired(now) || !s.within_grace(now) {
                free_limits()
            } else {
                s.limits
            }
        }
    }
}

/// Эффективный тир инстанса (контракт D).
///
/// Источник — кэшированное, проверенное по ed25519-подписи состояние лицензии в
/// AppState. Без сконфигурированного ключа (нет кэша) -> [`LicenseTier::Free`].
/// SOFT degrade (истёкшая лицензия / вне грейса) тоже отдаёт `Free` — это и есть
/// тир для новых привилегированных действий.
///
/// СИГНАТУРА НЕ МЕНЯЛАСЬ с P3: branding-эндпоинт вызывает её как раньше.
/// settings-ключ `license_tier` БОЛЬШЕ НЕ читается (иначе это тривиальный обход
/// лицензии) — единственный источник тира теперь проверенное состояние.
pub async fn effective_tier(state: &AppState) -> LicenseTier {
    let guard = state.license.read().await;
    let limits = effective_limits_from(&guard);
    // Pro отличаем по биллингу/брендингу (Free их не имеет). Это устойчиво к
    // SOFT degrade: при дегрейде limits == free_limits() -> Free.
    if limits.end_user_billing || limits.branding {
        LicenseTier::Pro
    } else {
        LicenseTier::Free
    }
}

/// Авторитетные эффективные лимиты (i64) для энфорсмента новых действий.
/// Применяет грейс/истечение так же, как [`effective_tier`].
pub async fn effective_limits(state: &AppState) -> LicenseLimits {
    let guard = state.license.read().await;
    effective_limits_from(&guard)
}

/// Локальный `Limits` из эффективных авторитетных лимитов (для кода, который
/// исторически работает с u32-представлением). Не используется branding-путём,
/// но удобен будущим вызывающим, чтобы не хардкодить тир->лимиты.
pub async fn effective_local_limits(state: &AppState) -> Limits {
    local_limits_from(&effective_limits(state).await)
}

/// Эффективные авторитетные лимиты, вычисленные ПРЯМО из БД (`license_state`),
/// с применением грейса/истечения. Для слоёв (сервисов), у которых есть только
/// `PgPool`, а не `AppState`. Любой сбой чтения -> Free (никогда не паника).
///
/// Семантика идентична [`effective_limits`]: нет строки -> Free; `expires_at`
/// прошёл или вне грейса -> Free; иначе кэшированные лимиты.
pub async fn effective_limits_from_pool(pool: &sqlx::PgPool) -> LicenseLimits {
    let repo = caramba_db::repositories::license_repo::LicenseRepository::new(pool.clone());
    let row = match repo.get().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(err = %e, "license: pool-side limits read failed, treating as Free");
            return free_limits();
        }
    };
    let Some(row) = row else {
        return free_limits();
    };
    let now = chrono::Utc::now();
    let within_grace = now < row.last_verified_at + chrono::Duration::days(GRACE_DAYS);
    let expired = now >= row.expires_at;
    if expired || !within_grace {
        return free_limits();
    }
    match serde_json::from_value::<LicenseLimits>(row.limits_json) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(err = %e, "license: pool-side limits_json unparsable, treating as Free");
            free_limits()
        }
    }
}

// ============================================================
// ЭНФОРСМЕНТ (контракт E)
// ============================================================

/// Ошибка энфорсмента лицензии. Маппится вызывающими в ясный non-2xx ответ;
/// никогда не паникует и не используется для блокировки уже выданных юзеров.
#[derive(Debug, Clone)]
pub struct LicenseError {
    pub message: String,
}

impl LicenseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LicenseError {}

/// Гейт добавления ноды. `max_nodes == 0` означает «без лимита» (Pro).
/// Блокирует только когда текущее число нод достигло лимита.
pub fn check_can_add_node(limits: &LicenseLimits, current: i64) -> Result<(), LicenseError> {
    if limits.max_nodes != 0 && current >= limits.max_nodes {
        return Err(LicenseError::new(format!(
            "Node limit reached for this license tier ({} of {}). Upgrade to add more nodes.",
            current, limits.max_nodes
        )));
    }
    Ok(())
}

/// Гейт создания НОВОГО пользователя. `max_users == 0` означает «без лимита»
/// (Pro). Вызывается только для свежего пользователя; существующих не трогаем.
pub fn check_can_add_user(limits: &LicenseLimits, current: i64) -> Result<(), LicenseError> {
    if limits.max_users != 0 && current >= limits.max_users {
        return Err(LicenseError::new(format!(
            "User limit reached for this license tier ({} of {}). Upgrade to add more users.",
            current, limits.max_users
        )));
    }
    Ok(())
}

/// Гейт биллинга конечных пользователей (purchase-эндпоинты).
pub fn check_billing_enabled(limits: &LicenseLimits) -> Result<(), LicenseError> {
    if !limits.end_user_billing {
        return Err(LicenseError::new(
            "End-user billing is not available on this license tier.",
        ));
    }
    Ok(())
}

/// Требуется ли ручное подтверждение выдачи новых конфигов/подписок (Free).
pub fn requires_manual_approval(limits: &LicenseLimits) -> bool {
    limits.manual_approval
}

/// Начальный статус новой end-user подписки по флагу `manual_approval`.
/// Free -> `pending` (конфиг не выдаётся до подтверждения админом); Pro ->
/// `active` (авто-активация). Использует существующий lifecycle статусов,
/// никогда не пере-пендит уже активную подписку.
pub fn initial_subscription_status(limits: &LicenseLimits) -> &'static str {
    if requires_manual_approval(limits) {
        "pending"
    } else {
        "active"
    }
}
