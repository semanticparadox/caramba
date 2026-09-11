//! Онбординг: три касания после регистрации.
//!
//! Владелец просил, чтобы новый пользователь сразу понимал, что происходит в
//! сервисе, что есть своё приложение и что делать дальше. Три сообщения:
//!
//!   * `onboarding.day0` — «Ваши следующие шаги», сразу после принятия
//!     соглашения (ветка `accept_terms` в `bot::handlers::callback` зовёт
//!     [`send_day0`]);
//!   * `onboarding.day1` — «Вы ещё не подключились», через 24 часа и ТОЛЬКО
//!     если у человека нет ни одной лизы устройства;
//!   * `onboarding.day3` — «Что ещё умеет Caramba Connect», через 72 часа всем.
//!
//! Тексты и кнопки живут в `notification_templates::REGISTRY` и правятся в
//! админке «Уведомления», как и девять денежных уведомлений.
//!
//! # Как не отправить дважды
//!
//! Таблица `onboarding_deliveries(user_id, step)` с первичным ключом по паре:
//! шаг сначала «забирается» вставкой (`ON CONFLICT DO NOTHING`), и только тот,
//! кто вставил строку, отправляет сообщение. Гонка между веткой `accept_terms`
//! и фоновым циклом поэтому безопасна. Обратная сторона: неудачная отправка
//! (человек заблокировал бота, Telegram недоступен) не повторяется. Это
//! осознанно: онбординг — вежливость, а не платёжное уведомление, и долбить
//! заблокировавшего бота повторами каждые полчаса две недели было бы хуже,
//! чем потерять одно касание.
//!
//! # Старые пользователи
//!
//! Миграция `20260911170000_onboarding.sql` записала все три шага как
//! доставленные каждому, кто принял соглашение до релиза. Иначе двадцати
//! существующим аккаунтам в первые полчаса прилетело бы «вы ещё не
//! подключились».
//!
//! # Решение «кому какой шаг»
//!
//! [`next_step`] — чистая функция от фактов о пользователе, момента «сейчас» и
//! расписания. Всё, что требует БД (кандидаты, флаг устройств, предпочтения
//! каналов), собирается в [`Candidate`] одним запросом, а сама логика
//! проверяется тестами без базы.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::AppState;
use crate::bot::translations::{Lang, default_language_setting, resolve_lang};

/// Категория для `notification_channel_prefs` и карточки во входящих.
pub const CATEGORY: &str = "onboarding";
/// `true`/`false`: выключатель всего онбординга (по умолчанию включён).
pub const SETTING_ENABLED: &str = "onboarding_enabled";
/// Через сколько часов после регистрации уходит `day1`.
pub const SETTING_DAY1_HOURS: &str = "onboarding_day1_hours";
/// Через сколько часов после регистрации уходит `day3`.
pub const SETTING_DAY3_HOURS: &str = "onboarding_day3_hours";
pub const DEFAULT_DAY1_HOURS: i64 = 24;
pub const DEFAULT_DAY3_HOURS: i64 = 72;
/// Кандидаты — только зарегистрированные за последние две недели: дольше
/// этого срока «следующие шаги» уже не следующие.
pub const CANDIDATE_WINDOW_DAYS: i64 = 14;
/// Период фонового цикла.
const LOOP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Шаг онбординга. `id` хранится в БД, `event_key` — ключ шаблона.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Day0,
    Day1,
    Day3,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Day0, Step::Day1, Step::Day3];

    /// Значение колонки `onboarding_deliveries.step`.
    pub const fn id(self) -> &'static str {
        match self {
            Step::Day0 => "day0",
            Step::Day1 => "day1",
            Step::Day3 => "day3",
        }
    }

    /// Ключ события в `notification_templates::REGISTRY`.
    pub const fn event_key(self) -> &'static str {
        match self {
            Step::Day0 => "onboarding.day0",
            Step::Day1 => "onboarding.day1",
            Step::Day3 => "onboarding.day3",
        }
    }

    pub fn parse(id: &str) -> Option<Step> {
        Step::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// Расписание из настроек: через сколько после регистрации уходят `day1` и
/// `day3`. Ноль и отрицательные значения не имеют смысла и заменяются часом:
/// иначе опечатка в админке разослала бы «вы ещё не подключились» всем
/// новичкам в ту же минуту, что и приветствие.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub day1_after: Duration,
    pub day3_after: Duration,
}

impl Schedule {
    pub fn from_hours(day1_hours: i64, day3_hours: i64) -> Self {
        Self {
            day1_after: Duration::hours(day1_hours.max(1)),
            day3_after: Duration::hours(day3_hours.max(1)),
        }
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::from_hours(DEFAULT_DAY1_HOURS, DEFAULT_DAY3_HOURS)
    }
}

/// Всё, что нужно знать о пользователе, чтобы выбрать шаг.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub user_id: i64,
    pub tg_id: i64,
    pub language_code: Option<String>,
    pub terms_accepted_at: DateTime<Utc>,
    /// Есть ли хоть одна лиза устройства (`subscription_device_leases.user_id`).
    pub has_device: bool,
    /// Канал `bot_dm` для категории `onboarding` не выключен пользователем.
    pub dm_enabled: bool,
    /// Шаги, уже записанные в `onboarding_deliveries`.
    pub delivered: Vec<Step>,
}

impl Candidate {
    fn delivered(&self, step: Step) -> bool {
        self.delivered.contains(&step)
    }
}

/// Какой шаг отправить этому пользователю прямо сейчас, если какой-то нужен.
///
/// За один вызов — не больше одного шага: если цикл долго не работал и у
/// человека «созрели» сразу `day1` и `day3`, второй уйдёт следующим проходом,
/// а не в ту же минуту. Порядок проверок и есть правила:
///
///   1. отключённый канал — ничего;
///   2. `day0` не отправлен — `day0` (страховка на случай, когда ветка
///      `accept_terms` не дошла до отправки);
///   3. `day1` не отправлен, срок вышел и устройств нет — `day1`; с
///      устройством шаг пропускается насовсем, а не откладывается;
///   4. `day3` не отправлен и срок вышел — `day3`.
pub fn next_step(c: &Candidate, now: DateTime<Utc>, schedule: &Schedule) -> Option<Step> {
    if !c.dm_enabled {
        return None;
    }
    if !c.delivered(Step::Day0) {
        return Some(Step::Day0);
    }
    let age = now - c.terms_accepted_at;
    if !c.delivered(Step::Day1) && age >= schedule.day1_after && !c.has_device {
        return Some(Step::Day1);
    }
    if !c.delivered(Step::Day3) && age >= schedule.day3_after {
        return Some(Step::Day3);
    }
    None
}

/// Забирает шаг за собой. `true` — строки не было и отправлять нам; `false` —
/// кто-то уже забрал (или отправил) этот шаг.
async fn claim(pool: &PgPool, user_id: i64, step: Step) -> Result<bool> {
    let affected = sqlx::query(
        "INSERT INTO onboarding_deliveries (user_id, step) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(step.id())
    .execute(pool)
    .await
    .context("onboarding: не удалось записать доставку")?
    .rows_affected();
    Ok(affected == 1)
}

/// Отправляет шаг: сначала забирает его в БД, затем шлёт сообщение в бот и
/// кладёт карточку во входящие мини-аппа. `Ok(true)` — сообщение ушло.
///
/// Ошибка Telegram не откатывает запись (см. заголовок модуля) и не
/// возвращается как `Err`: для вызывающего это «шаг закрыт», а причина
/// остаётся в логе.
pub async fn send_step(
    state: &AppState,
    user_id: i64,
    tg_id: i64,
    lang: Lang,
    step: Step,
) -> Result<bool> {
    if !claim(&state.pool, user_id, step).await? {
        return Ok(false);
    }
    let rendered = state
        .notification_templates
        .render_with(&state.settings, step.event_key(), lang, &[])
        .await;
    let sent = match state
        .bot_manager
        .send_rich_notification(tg_id, rendered.payload)
        .await
    {
        Ok(()) => true,
        Err(e) => {
            warn!(
                user_id,
                tg_id,
                step = step.id(),
                err = %e,
                "onboarding: сообщение не доставлено"
            );
            false
        }
    };
    if let Err(e) = state
        .notifications_svc
        .create_inbox_only(
            user_id,
            CATEGORY,
            "info",
            &rendered.title,
            &rendered.body,
            None,
        )
        .await
    {
        warn!(user_id, step = step.id(), err = %e, "onboarding: карточка во входящие не записана");
    }
    Ok(sent)
}

/// `day0` сразу после принятия соглашения. Best-effort: ошибка уходит в лог,
/// регистрацию она не трогает.
pub async fn send_day0(state: &AppState, user_id: i64, tg_id: i64, lang: Lang) {
    if !enabled(state).await {
        return;
    }
    if let Err(e) = send_step(state, user_id, tg_id, lang, Step::Day0).await {
        error!(user_id, err = %e, "onboarding: day0 не отправлен");
    }
}

async fn enabled(state: &AppState) -> bool {
    state
        .settings
        .get_bool_or_default(SETTING_ENABLED, true)
        .await
}

async fn schedule(state: &AppState) -> Schedule {
    let hours = |key: &'static str, default: i64| async move {
        state
            .settings
            .get_or_default(key, &default.to_string())
            .await
            .trim()
            .parse::<i64>()
            .unwrap_or(default)
    };
    Schedule::from_hours(
        hours(SETTING_DAY1_HOURS, DEFAULT_DAY1_HOURS).await,
        hours(SETTING_DAY3_HOURS, DEFAULT_DAY3_HOURS).await,
    )
}

/// Кандидаты одним запросом: только зарегистрированные за окно, с Telegram,
/// не забаненные и с хотя бы одним незакрытым шагом.
async fn candidates(pool: &PgPool) -> Result<Vec<Candidate>> {
    let rows: Vec<(i64, i64, Option<String>, DateTime<Utc>, bool, bool, Vec<String>)> =
        sqlx::query_as(
            r#"
            SELECT u.id,
                   u.tg_id,
                   u.language_code,
                   u.terms_accepted_at,
                   EXISTS (SELECT 1 FROM subscription_device_leases l WHERE l.user_id = u.id) AS has_device,
                   COALESCE(
                       (SELECT p.enabled FROM notification_channel_prefs p
                         WHERE p.user_id = u.id AND p.category = $1 AND p.channel = 'bot_dm'),
                       TRUE
                   ) AS dm_enabled,
                   ARRAY(SELECT d.step FROM onboarding_deliveries d WHERE d.user_id = u.id) AS delivered
            FROM users u
            WHERE u.tg_id IS NOT NULL
              AND u.tg_id > 0
              AND u.is_banned IS NOT TRUE
              AND u.terms_accepted_at >= NOW() - ($2::TEXT || ' days')::INTERVAL
              AND (SELECT COUNT(*) FROM onboarding_deliveries d WHERE d.user_id = u.id) < $3
            ORDER BY u.terms_accepted_at
            "#,
        )
        .bind(CATEGORY)
        .bind(CANDIDATE_WINDOW_DAYS.to_string())
        .bind(Step::ALL.len() as i64)
        .fetch_all(pool)
        .await
        .context("onboarding: не удалось прочитать кандидатов")?;

    Ok(rows
        .into_iter()
        .map(
            |(
                user_id,
                tg_id,
                language_code,
                terms_accepted_at,
                has_device,
                dm_enabled,
                delivered,
            )| {
                Candidate {
                    user_id,
                    tg_id,
                    language_code,
                    terms_accepted_at,
                    has_device,
                    dm_enabled,
                    delivered: delivered.iter().filter_map(|s| Step::parse(s)).collect(),
                }
            },
        )
        .collect())
}

/// Один проход: выбирает шаг каждому кандидату и отправляет. Возвращает число
/// ушедших сообщений.
pub async fn sweep(state: &AppState, schedule: &Schedule) -> Result<usize> {
    let list = candidates(&state.pool).await?;
    if list.is_empty() {
        return Ok(0);
    }
    let default_lang = default_language_setting(&state.pool).await;
    let now = Utc::now();
    let mut sent = 0;
    for c in &list {
        let Some(step) = next_step(c, now, schedule) else {
            continue;
        };
        let lang = resolve_lang(c.language_code.as_deref(), default_lang.as_deref());
        match send_step(state, c.user_id, c.tg_id, lang, step).await {
            Ok(true) => sent += 1,
            Ok(false) => {}
            Err(e) => {
                error!(user_id = c.user_id, step = step.id(), err = %e, "onboarding: шаг не отправлен")
            }
        }
    }
    Ok(sent)
}

/// Фоновый цикл: раз в полчаса, пока включена настройка. Запускается из
/// `main.rs` рядом с циклом напоминаний об окончании подписки.
pub async fn run_onboarding_loop(state: AppState) {
    loop {
        tokio::time::sleep(LOOP_INTERVAL).await;
        if !enabled(&state).await {
            continue;
        }
        let schedule = schedule(&state).await;
        match sweep(&state, &schedule).await {
            Ok(0) => {}
            Ok(n) => info!(sent = n, "onboarding: проход завершён"),
            Err(e) => error!(err = %e, "onboarding: проход не выполнен"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(hours_ago: i64) -> Candidate {
        Candidate {
            user_id: 1,
            tg_id: 100,
            language_code: Some("ru".to_string()),
            terms_accepted_at: Utc::now() - Duration::hours(hours_ago),
            has_device: false,
            dm_enabled: true,
            delivered: vec![],
        }
    }

    fn decide(c: &Candidate) -> Option<Step> {
        next_step(c, Utc::now(), &Schedule::default())
    }

    /// Свежий аккаунт без записей получает `day0`, и только его.
    #[test]
    fn fresh_account_gets_day0_first() {
        assert_eq!(decide(&candidate(0)), Some(Step::Day0));
        // Даже если сроки day1/day3 давно вышли: по одному шагу за проход.
        assert_eq!(decide(&candidate(100)), Some(Step::Day0));
    }

    /// До срока `day1` после `day0` ничего не уходит.
    #[test]
    fn nothing_between_day0_and_day1() {
        let mut c = candidate(23);
        c.delivered = vec![Step::Day0];
        assert_eq!(decide(&c), None);
    }

    /// `day1` — только тем, у кого нет устройств; с устройством шаг
    /// пропускается, а `day3` при этом ещё не созрел.
    #[test]
    fn day1_only_without_devices() {
        let mut c = candidate(25);
        c.delivered = vec![Step::Day0];
        assert_eq!(decide(&c), Some(Step::Day1));

        c.has_device = true;
        assert_eq!(decide(&c), None);
    }

    /// Через 72 часа `day3` уходит всем: и тем, кто получил `day1`, и тем,
    /// кому его пропустили из-за устройства.
    #[test]
    fn day3_goes_to_everyone_after_72_hours() {
        let mut connected = candidate(73);
        connected.delivered = vec![Step::Day0];
        connected.has_device = true;
        assert_eq!(decide(&connected), Some(Step::Day3));

        let mut reminded = candidate(73);
        reminded.delivered = vec![Step::Day0, Step::Day1];
        assert_eq!(decide(&reminded), Some(Step::Day3));

        // Без устройства и без day1 сначала догоняет day1, day3 — следующим проходом.
        let mut lagging = candidate(73);
        lagging.delivered = vec![Step::Day0];
        assert_eq!(decide(&lagging), Some(Step::Day1));
    }

    #[test]
    fn everything_delivered_means_silence() {
        let mut c = candidate(200);
        c.delivered = Step::ALL.to_vec();
        assert_eq!(decide(&c), None);
    }

    /// Выключенный канал молчит на любом шаге — и на `day0` тоже.
    #[test]
    fn disabled_channel_silences_every_step() {
        let mut c = candidate(73);
        c.dm_enabled = false;
        assert_eq!(decide(&c), None);
        c.delivered = vec![Step::Day0];
        assert_eq!(decide(&c), None);
    }

    /// Часы из настроек соблюдаются, а ноль не превращает day1 в мгновенный.
    #[test]
    fn schedule_hours_are_honoured_and_clamped() {
        let s = Schedule::from_hours(1, 2);
        let mut c = candidate(0);
        c.delivered = vec![Step::Day0];
        c.terms_accepted_at = Utc::now() - Duration::minutes(61);
        assert_eq!(next_step(&c, Utc::now(), &s), Some(Step::Day1));
        c.delivered = vec![Step::Day0, Step::Day1];
        assert_eq!(next_step(&c, Utc::now(), &s), None);
        c.terms_accepted_at = Utc::now() - Duration::minutes(121);
        assert_eq!(next_step(&c, Utc::now(), &s), Some(Step::Day3));

        assert_eq!(
            Schedule::from_hours(0, -5),
            Schedule::from_hours(1, 1),
            "ноль и минус заменяются часом"
        );
    }

    /// Идентификаторы шагов — ровно те строки, что записала миграция.
    #[test]
    fn step_ids_round_trip_and_match_the_migration() {
        for step in Step::ALL {
            assert_eq!(Step::parse(step.id()), Some(step));
            assert!(step.event_key().ends_with(step.id()));
        }
        assert_eq!(Step::parse("day2"), None);
        let migration =
            include_str!("../../../../libs/caramba-db/migrations/20260911170000_onboarding.sql");
        for step in Step::ALL {
            assert!(
                migration.contains(&format!("('{}')", step.id())),
                "миграция не бэкфиллит шаг {}",
                step.id()
            );
        }
    }
}
