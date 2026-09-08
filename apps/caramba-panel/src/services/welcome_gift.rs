//! Подарочная подписка при регистрации («акция до 1 октября»).
//!
//! Смысл акции: человек, впервые пришедший в бота, сразу получает платный план
//! на N дней, а когда подарок истекает, `monitoring` сажает его на бесплатный
//! тариф ровно тем же путём, что и любую другую истёкшую подписку. Отдельного
//! «перехода на Free» здесь нет и не должно быть — он уже написан один раз.
//!
//! # Почему подарок выдаётся ДО бесплатного плана
//!
//! `store_service::ensure_free_plan_subscription_tx` первым делом проверяет, нет
//! ли у пользователя активной подписки на не-бесплатном плане, и при её наличии
//! ничего не создаёт. Поэтому порядок «сначала подарок, потом прежняя выдача
//! Free» не требует никаких флагов: выдача Free сама себя пропустит. Обратный
//! порядок дал бы пользователю две подписки сразу.
//!
//! # Кому подарок НЕ достаётся
//!
//! * Аккаунтам без `users.tg_id` — то есть заведённым через
//!   `POST /api/v2/app/register` по email. У этого эндпоинта нет ни капчи, ни
//!   подтверждения почты, так что безлимит с него фармится скриптом за минуту.
//!   Осознанное ограничение: акция живёт только там, где вход стоит Telegram-аккаунта.
//! * Всем, у кого уже есть ХОТЯ БЫ ОДНА строка в `subscriptions` — любого
//!   статуса, включая истёкшую бесплатную. Подарок ровно один раз в жизни
//!   аккаунта, и это же условие делает повторное нажатие «принять условия»
//!   безобидным.
//!
//! # Настройки (таблица `settings`, миграций не требуют)
//!
//! * [`SETTING_PLAN_ID`] — id дарёного плана; пусто/мусор = акция выключена;
//! * [`SETTING_DAYS`] — срок в днях, по умолчанию [`DEFAULT_DAYS`];
//! * [`SETTING_UNTIL`] — дата окончания акции `YYYY-MM-DD` (UTC); пусто =
//!   бессрочно.
//!
//! Дефолт всех трёх — пусто, то есть выключено: акция включается только явной
//! настройкой оператора, чтобы деплой кода сам по себе никому ничего не раздал.

use crate::AppState;
use chrono::{DateTime, NaiveDate, Utc};

/// Id плана, который дарится при регистрации. Пусто = акция выключена.
pub const SETTING_PLAN_ID: &str = "welcome_gift_plan_id";

/// Срок подарка в днях.
pub const SETTING_DAYS: &str = "welcome_gift_days";

/// Дата окончания акции, `YYYY-MM-DD` в UTC. Пусто = бессрочно.
pub const SETTING_UNTIL: &str = "welcome_gift_until";

/// Срок подарка, если [`SETTING_DAYS`] не задан или задан мусором.
pub const DEFAULT_DAYS: i32 = 30;

/// Метка в `subscriptions.note` — по ней подарочные подписки отличимы от
/// покупок и от ручных подарков админа в отчётах и в поддержке.
pub const NOTE: &str = "welcome_gift";

/// Что регистрация выдала аккаунту сверх обычного бесплатного плана.
///
/// Возвращается наружу ради ОДНОГО потребителя — бота, который показывает
/// человеку сообщение о подарке. Приложение (`/api/v2/app/register`) результат
/// игнорирует: у него нет экрана, где это сообщение было бы уместно.
#[derive(Debug, Clone, Default)]
pub struct SignupGrant {
    pub gift: Option<WelcomeGift>,
}

/// Выданный подарок — ровно те данные, которые нужны тексту сообщения.
#[derive(Debug, Clone)]
pub struct WelcomeGift {
    /// Название плана из `plans.name` (в сообщении подставляется как есть).
    pub plan_name: String,
    /// Срок подарка в днях.
    pub days: i32,
    /// `daily_traffic_mb` активного бесплатного плана — сколько останется, когда
    /// подарок истечёт. `None`, если бесплатный план не настроен или его суточная
    /// квота нулевая: тогда сообщение обходится без числа, а не врёт про «0 МБ».
    pub free_daily_mb: Option<i32>,
}

/// Разобранные настройки акции.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoSettings {
    pub plan_id: i64,
    pub days: i32,
    /// Дата окончания; `None` — акция бессрочная.
    pub until: Option<NaiveDate>,
}

/// Решение о подарке: какой план и на сколько дней.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GiftPlan {
    pub plan_id: i64,
    pub days: i32,
}

/// Разбирает три настройки. `None` — акция выключена.
///
/// Fail-closed по каждому непонятному значению: неразобранный `plan_id` и
/// неразобранная дата выключают акцию целиком. Опечатка в дате («2026-13-01»)
/// иначе означала бы бессрочную раздачу безлимита — самый дорогой из возможных
/// вариантов «на всякий случай продолжим».
///
/// Срок — единственное поле с мягким разбором: мусор в нём откатывается к
/// [`DEFAULT_DAYS`], потому что «сколько дней» не влияет на то, кому дарить.
pub fn parse_settings(plan_id_raw: &str, days_raw: &str, until_raw: &str) -> Option<PromoSettings> {
    let plan_id: i64 = plan_id_raw.trim().parse().ok()?;
    if plan_id <= 0 {
        return None;
    }

    let days = days_raw
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|d| *d >= 1)
        .unwrap_or(DEFAULT_DAYS);

    let until_raw = until_raw.trim();
    let until = if until_raw.is_empty() {
        None
    } else {
        Some(NaiveDate::parse_from_str(until_raw, "%Y-%m-%d").ok()?)
    };

    Some(PromoSettings {
        plan_id,
        days,
        until,
    })
}

/// Идёт ли акция в момент `now`.
///
/// Граница — полночь UTC указанной даты: `welcome_gift_until = 2026-10-01`
/// значит «последний день акции 30 сентября», как и читается фраза «дата
/// окончания акции 1 октября» в приказе.
pub fn is_open(settings: &PromoSettings, now: DateTime<Utc>) -> bool {
    match settings.until {
        None => true,
        Some(until) => now.date_naive() < until,
    }
}

/// Чистое решение: положен ли подарок. Единственное место, где сходятся все
/// условия акции — тесты внизу файла покрывают каждую ветку.
pub fn decide(
    settings: &PromoSettings,
    now: DateTime<Utc>,
    has_tg: bool,
    has_any_subscription: bool,
) -> Option<GiftPlan> {
    if !is_open(settings, now) || !has_tg || has_any_subscription {
        return None;
    }
    Some(GiftPlan {
        plan_id: settings.plan_id,
        days: settings.days,
    })
}

/// Выдаёт подарок, если он положен. Возвращает данные для сообщения бота.
///
/// Best-effort целиком, как и вся выдача при регистрации: любая ошибка гасится в
/// `warn!` и превращается в `None`. Аккаунт уже создан, и ни один сбой раздачи
/// подарков не имеет права сломать регистрацию — человек в худшем случае
/// останется на бесплатном плане, который выдаётся следующим шагом.
pub(crate) async fn grant_on_signup(state: &AppState, user_id: i64) -> Option<WelcomeGift> {
    let settings = parse_settings(
        &state.settings.get_or_default(SETTING_PLAN_ID, "").await,
        &state.settings.get_or_default(SETTING_DAYS, "").await,
        &state.settings.get_or_default(SETTING_UNTIL, "").await,
    )?;

    // Одним запросом оба условия по пользователю: акция для Telegram-аккаунтов и
    // ровно один подарок в жизни аккаунта.
    let eligibility = sqlx::query_as::<_, (bool, bool)>(
        "SELECT u.tg_id IS NOT NULL, \
                EXISTS(SELECT 1 FROM subscriptions s WHERE s.user_id = u.id) \
         FROM users u WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(user_id, error = %e, "welcome gift: eligibility check failed (non-fatal)");
        None
    });
    let (has_tg, has_any_subscription) = eligibility?;

    let gift = decide(&settings, Utc::now(), has_tg, has_any_subscription)?;

    // Имя плана нужно и само по себе (в сообщение), и как проверка, что
    // настройка указывает на живой план: с мусорным id INSERT упал бы по FK.
    let plan_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM plans WHERE id = $1 AND is_active = TRUE")
            .bind(gift.plan_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);
    let Some(plan_name) = plan_name else {
        tracing::warn!(
            user_id,
            plan_id = gift.plan_id,
            "welcome gift: configured plan is missing or inactive, gift skipped"
        );
        return None;
    };

    if let Err(e) = state
        .store_service
        .admin_gift_subscription_with_note(user_id, gift.plan_id, gift.days, Some(NOTE))
        .await
    {
        tracing::warn!(
            user_id,
            plan_id = gift.plan_id,
            error = %e,
            "welcome gift: grant failed (non-fatal), user falls back to the free plan"
        );
        return None;
    }

    // Подписка есть в базе, но нод она достигнет только с перевыпуском конфига —
    // как и при выдаче бесплатного плана. Сбой публикации не отменяет подарок:
    // строка создана, и следующий проход оркестрации её подхватит.
    if let Err(e) = state
        .orchestration_service
        .notify_nodes_for_plans(&[gift.plan_id])
        .await
    {
        tracing::warn!(
            user_id,
            plan_id = gift.plan_id,
            error = %e,
            "welcome gift: granted but node publish failed (non-fatal)"
        );
    }

    tracing::info!(
        user_id,
        plan_id = gift.plan_id,
        days = gift.days,
        "welcome gift: granted"
    );

    Some(WelcomeGift {
        plan_name,
        days: gift.days,
        free_daily_mb: free_plan_daily_mb(state).await,
    })
}

/// Суточная квота бесплатного плана — то, на что человек перейдёт после
/// подарка. `None`, если бесплатного плана нет или квота нулевая: сообщение
/// тогда обходится без числа.
async fn free_plan_daily_mb(state: &AppState) -> Option<i32> {
    sqlx::query_scalar::<_, i32>(
        "SELECT daily_traffic_mb FROM plans \
         WHERE is_free = TRUE AND is_active = TRUE AND daily_traffic_mb > 0 LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(date: &str, time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("{date}T{time}Z"))
            .unwrap()
            .with_timezone(&Utc)
    }

    fn promo() -> PromoSettings {
        parse_settings("1", "30", "2026-10-01").unwrap()
    }

    // ---- разбор настроек ----------------------------------------------------

    #[test]
    fn an_empty_plan_id_turns_the_promo_off() {
        assert_eq!(parse_settings("", "30", "2026-10-01"), None);
        assert_eq!(parse_settings("   ", "30", ""), None);
        assert_eq!(parse_settings("gold", "30", ""), None);
        assert_eq!(parse_settings("0", "30", ""), None);
    }

    #[test]
    fn a_broken_deadline_turns_the_promo_off_rather_than_making_it_endless() {
        assert_eq!(parse_settings("1", "30", "01.10.2026"), None);
        assert_eq!(parse_settings("1", "30", "2026-13-01"), None);
    }

    #[test]
    fn an_empty_deadline_means_no_deadline() {
        let parsed = parse_settings("1", "30", "  ").unwrap();
        assert_eq!(parsed.until, None);
        assert!(is_open(&parsed, at("2099-01-01", "00:00:00")));
    }

    #[test]
    fn a_broken_duration_falls_back_to_the_default() {
        assert_eq!(parse_settings("1", "", "").unwrap().days, DEFAULT_DAYS);
        assert_eq!(parse_settings("1", "0", "").unwrap().days, DEFAULT_DAYS);
        assert_eq!(parse_settings("1", "-5", "").unwrap().days, DEFAULT_DAYS);
        assert_eq!(parse_settings("1", "60", "").unwrap().days, 60);
    }

    // ---- решение ------------------------------------------------------------

    #[test]
    fn a_telegram_account_without_subscriptions_gets_the_gift() {
        assert_eq!(
            decide(&promo(), at("2026-09-07", "12:00:00"), true, false),
            Some(GiftPlan {
                plan_id: 1,
                days: 30
            })
        );
    }

    #[test]
    fn the_gift_stops_on_the_deadline_date_itself() {
        // Последний день акции — канун даты окончания.
        assert!(decide(&promo(), at("2026-09-30", "23:59:59"), true, false).is_some());
        assert_eq!(
            decide(&promo(), at("2026-10-01", "00:00:00"), true, false),
            None
        );
        assert_eq!(
            decide(&promo(), at("2026-10-02", "12:00:00"), true, false),
            None
        );
    }

    #[test]
    fn an_email_account_gets_no_gift() {
        assert_eq!(
            decide(&promo(), at("2026-09-07", "12:00:00"), false, false),
            None
        );
    }

    #[test]
    fn an_account_with_any_subscription_gets_no_second_gift() {
        assert_eq!(
            decide(&promo(), at("2026-09-07", "12:00:00"), true, true),
            None
        );
    }
}
