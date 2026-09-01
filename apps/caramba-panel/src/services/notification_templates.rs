//! Редактируемые шаблоны системных уведомлений.
//!
//! Девять сообщений `notify.*` — те, на которых стоят деньги: подписка кончилась,
//! баланс на исходе, автопродление не прошло. До этого модуля они были строковыми
//! литералами в `macro_rules! translations`, то есть менялись только пересборкой,
//! и уходили голым текстом: копирайт говорил «пополните баланс», а нажать было
//! нечего.
//!
//! # Что здесь есть
//!
//!   * [`REGISTRY`] — девять событий, их подстановки и кнопка по умолчанию. Одно
//!     место правды: из него берёт подсказки редактор, по нему же валидируются
//!     индексы `{N}` и проверяется полнота переводов в тестах.
//!   * [`NotificationTemplateService`] — чтение переопределений из БД с кэшем и
//!     сборка готового [`NotificationPayload`].
//!
//! # Почему кэш обязателен
//!
//! Уведомления рассылаются свипами мониторинга сразу по всем подходящим
//! пользователям. Запрос за шаблоном на каждого лёг бы на горячий путь свипа,
//! поэтому сервис устроен как [`crate::settings::SettingsService`]: карта в
//! `RwLock`, полная перезагрузка на старте и запись сквозь кэш.
//!
//! # Фолбэк
//!
//! Нет строки в БД или поле `NULL` — берётся встроенная строка. Это не ленивая
//! реализация, а решение: оператор, который не открывал редактор, продолжает
//! получать улучшения текстов вместе с обновлением кода, а «вернуть исходный»
//! становится удалением строки, а не записью копии сегодняшнего дефолта.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::info;

use crate::bot::translations::{Lang, substitute, t};
use crate::bot_manager::{NotificationMediaType, NotificationPayload};

// ---------------------------------------------------------------------------
// Реестр событий
// ---------------------------------------------------------------------------

/// Экран мини-аппа, на который ведёт кнопка уведомления.
///
/// Ссылка собирается как `https://t.me/<bot>/<short_name>?startapp=<slug>`, а
/// мини-апп читает `start_param` и переходит на маршрут. Это обычная url-кнопка,
/// поэтому отправку (`send_rich_notification`) менять не потребовалось.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonTarget {
    /// Пополнение баланса — `/billing`.
    Billing,
    /// Витрина тарифов — `/plans`.
    Plans,
    /// Текущая подписка — `/subscription`.
    Subscription,
}

impl ButtonTarget {
    pub const fn slug(self) -> &'static str {
        match self {
            ButtonTarget::Billing => "billing",
            ButtonTarget::Plans => "plans",
            ButtonTarget::Subscription => "subscription",
        }
    }

    pub const ALL: &'static [ButtonTarget] = &[
        ButtonTarget::Billing,
        ButtonTarget::Plans,
        ButtonTarget::Subscription,
    ];
}

/// Одно системное уведомление: ключ, смысл его подстановок и кнопка,
/// которая уедет вместе с ним, если оператор ничего не настраивал.
pub struct NotifyEvent {
    /// Базовый ключ. Текст лежит под ним, карточка в приложении — под
    /// `<key>_title` и `<key>_body`. Форма соблюдается всеми девятью.
    pub key: &'static str,
    /// Человеческое название для списка в панели.
    pub label_ru: &'static str,
    /// Подписи к `{0}`, `{1}`, … — и одновременно арность для валидации.
    pub args: &'static [&'static str],
    /// Подпись кнопки и экран. `None` — уведомление информационное.
    pub default_button: Option<(&'static str, &'static str, ButtonTarget)>,
    /// Уходит ли это уведомление через [`NotificationPayload`].
    ///
    /// Восемь из девяти — да, и им доступны кнопка, медиа и parse mode.
    /// `notify.sni_rotation` отправляется отдельным путём (`notification_service`
    /// шлёт сырым `bot.send_message` в свою рассылку по узлу), поэтому у него
    /// редактируется только текст. Поля кнопки и медиа редактор для него
    /// прячет: показать их значило бы пообещать то, чего отправка не сделает.
    pub supports_payload: bool,
}

impl NotifyEvent {
    /// Отвергает `{N}`, для которых у события нет аргумента.
    ///
    /// [`substitute`] оставляет такие индексы в тексте дословно, поэтому без
    /// этой проверки опечатка админа уехала бы пользователю как `{3}`. Ловим на
    /// сохранении, где это ещё сообщение об ошибке, а не инцидент.
    pub fn validate_placeholders(&self, template: &str) -> Result<(), String> {
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            rest = &rest[open + 1..];
            let Some(close) = rest.find('}') else { break };
            let inner = &rest[..close];
            rest = &rest[close + 1..];
            if let Ok(idx) = inner.parse::<usize>()
                && idx >= self.args.len()
            {
                return Err(format!(
                    "В этом уведомлении нет подстановки {{{idx}}} — доступны {}",
                    if self.args.is_empty() {
                        "только текст без подстановок".to_string()
                    } else {
                        (0..self.args.len())
                            .map(|i| format!("{{{i}}} — {}", self.args[i]))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ));
            }
        }
        Ok(())
    }
}

/// Девять событий. Порядок — тот, в котором они показываются в панели:
/// сначала деньги, потом трафик, потом техническое.
pub const REGISTRY: &[NotifyEvent] = &[
    NotifyEvent {
        key: "notify.expired",
        label_ru: "Подписка закончилась",
        args: &["название тарифа"],
        default_button: Some(("💳 Продлить", "💳 Renew", ButtonTarget::Plans)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.expiry3",
        label_ru: "Подписка закончится через 3 дня",
        args: &[],
        default_button: Some(("💳 Продлить", "💳 Renew", ButtonTarget::Plans)),
        supports_payload: true,
    },
    // Обе кнопки ведут на витрину тарифов, а НЕ на экран баланса.
    //
    // Внутренний баланс пополнить нечем: `/billing` показывает сумму и историю,
    // формы пополнения нет, API пополнения нет, а единственные приходы — это
    // возвраты при удалении подписки и реферальные начисления. Кнопка «Пополнить»
    // вела бы на экран, где написано $0.00 и сделать нельзя ничего.
    //
    // Лекарство от «денег не хватило» в этом продукте — купить тариф напрямую,
    // где оплата и правда работает. Если пополнение баланса когда-нибудь
    // появится, вернуть сюда ButtonTarget::Billing — одна строка.
    NotifyEvent {
        key: "notify.low_balance",
        label_ru: "Баланса не хватит на автопродление",
        args: &["баланс", "название тарифа"],
        default_button: Some(("💳 Продлить", "💳 Renew", ButtonTarget::Plans)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.renew_failed",
        label_ru: "Автопродление не выполнено",
        args: &["название тарифа", "баланс", "требуется"],
        default_button: Some(("💳 Продлить", "💳 Renew", ButtonTarget::Plans)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.renewed",
        label_ru: "Подписка продлена",
        args: &["название тарифа", "дата", "сумма"],
        default_button: Some(("📱 Подписка", "📱 Subscription", ButtonTarget::Subscription)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.traffic80",
        label_ru: "Трафик: израсходовано 80%",
        args: &[],
        default_button: Some(("⬆️ Сменить тариф", "⬆️ Upgrade", ButtonTarget::Plans)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.traffic90",
        label_ru: "Трафик: израсходовано 90%",
        args: &[],
        default_button: Some(("⬆️ Сменить тариф", "⬆️ Upgrade", ButtonTarget::Plans)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.traffic_exceeded",
        label_ru: "Трафик закончился",
        args: &[],
        default_button: Some(("⬆️ Сменить тариф", "⬆️ Upgrade", ButtonTarget::Plans)),
        supports_payload: true,
    },
    NotifyEvent {
        key: "notify.sni_rotation",
        label_ru: "Ротация SNI",
        args: &["старый домен", "новый домен", "id ротации"],
        // Информационное: человеку нечего покупать, ему нужно переподключиться.
        default_button: None,
        supports_payload: false,
    },
];

pub fn event(key: &str) -> Option<&'static NotifyEvent> {
    REGISTRY.iter().find(|e| e.key == key)
}

// ---------------------------------------------------------------------------
// Переопределение из БД
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct TemplateOverride {
    pub event: String,
    pub lang: String,
    pub text: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub parse_mode: String,
    pub media_type: String,
    pub media_url: Option<String>,
    pub buttons_json: Option<serde_json::Value>,
    pub disable_link_preview: bool,
}

impl TemplateOverride {
    /// `buttons_json` → пары (текст, url). Мусор в колонке трактуется как
    /// «кнопок нет»: уведомление без кнопки хуже, чем с ней, но несравнимо
    /// лучше, чем не отправленное вовсе.
    fn buttons(&self) -> Vec<(String, String)> {
        let Some(value) = self.buttons_json.as_ref().and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        value
            .iter()
            .filter_map(|b| {
                let text = b.get("text")?.as_str()?.trim();
                let url = b.get("url")?.as_str()?.trim();
                if text.is_empty() || url.is_empty() {
                    return None;
                }
                Some((text.to_string(), url.to_string()))
            })
            .collect()
    }
}

/// Готовое уведомление: то, что уходит в Telegram, и то, что ложится карточкой
/// в приложение. Обе поверхности собираются из одного шаблона, поэтому не могут
/// разойтись по смыслу.
pub struct Rendered {
    pub payload: NotificationPayload,
    pub title: String,
    pub body: String,
}

pub struct NotificationTemplateService {
    pool: PgPool,
    cache: Arc<RwLock<HashMap<(String, String), TemplateOverride>>>,
}

impl NotificationTemplateService {
    pub async fn new(pool: PgPool) -> Result<Self> {
        let service = Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
        };
        service.reload_cache().await?;
        Ok(service)
    }

    /// Сервис с пустым кэшем — все уведомления идут на встроенных строках.
    ///
    /// Нужен ровно для одного случая: БД не ответила на старте. Ронять запуск
    /// панели из-за таблицы переопределений нельзя, а пустой кэш — это в
    /// точности штатное поведение установки, где редактор ни разу не открывали.
    pub fn empty(pool: PgPool) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn reload_cache(&self) -> Result<()> {
        let rows: Vec<TemplateOverride> =
            sqlx::query_as("SELECT * FROM notification_templates")
                .fetch_all(&self.pool)
                .await?;

        let mut cache = self.cache.write().await;
        cache.clear();
        for row in rows {
            cache.insert((row.event.clone(), row.lang.clone()), row);
        }
        info!("Notification templates cache reloaded: {} rows", cache.len());
        Ok(())
    }

    pub async fn get(&self, event: &str, lang: Lang) -> Option<TemplateOverride> {
        let cache = self.cache.read().await;
        cache
            .get(&(event.to_string(), lang.as_str().to_string()))
            .cloned()
    }

    /// Обёртка над [`render`](Self::render), которая сама достаёт из настроек
    /// имя бота и short name мини-аппа.
    ///
    /// Существует ради девяти точек отправки: без неё каждая тащила бы две
    /// строки настроек руками, и рано или поздно одна из них забыла бы.
    pub async fn render_with(
        &self,
        settings: &crate::settings::SettingsService,
        event_key: &str,
        lang: Lang,
        args: &[&str],
    ) -> Rendered {
        let bot_username = settings.get("bot_username").await;
        let short_name = settings.get_or_default("mini_app_short_name", "").await;
        self.render(event_key, lang, args, bot_username.as_deref(), &short_name)
            .await
    }

    /// Собирает уведомление: переопределение поверх встроенной строки.
    ///
    /// Кнопка берётся из переопределения; если оператор его не делал — из
    /// реестра, со ссылкой, собранной по `bot_username` и `mini_app_short_name`.
    /// Не настроен short name — ссылка ведёт на самого бота, а не собирается
    /// битой: неработающая кнопка хуже её отсутствия.
    pub async fn render(
        &self,
        event_key: &str,
        lang: Lang,
        args: &[&str],
        bot_username: Option<&str>,
        mini_app_short_name: &str,
    ) -> Rendered {
        let over = self.get(event_key, lang).await;
        let title_key = format!("{event_key}_title");
        let body_key = format!("{event_key}_body");

        let pick = |custom: Option<&String>, key: &str| -> String {
            match custom {
                Some(v) if !v.trim().is_empty() => substitute(v, args),
                _ => substitute(t(lang, key), args),
            }
        };

        let text = pick(over.as_ref().and_then(|o| o.text.as_ref()), event_key);
        let title = pick(over.as_ref().and_then(|o| o.title.as_ref()), &title_key);
        let body = pick(over.as_ref().and_then(|o| o.body.as_ref()), &body_key);

        let mut payload = match over.as_ref().map(|o| o.parse_mode.as_str()) {
            Some("plain") => NotificationPayload::plain(text),
            Some("markdown_v2") => NotificationPayload::legacy_markdown_v2(&text),
            _ => NotificationPayload::html(text),
        };

        if let Some(o) = over.as_ref() {
            payload.disable_link_preview = o.disable_link_preview;
            payload.media_type = match o.media_type.as_str() {
                "photo" => NotificationMediaType::Photo,
                "video" => NotificationMediaType::Video,
                _ => NotificationMediaType::None,
            };
            payload.media_url = o.media_url.clone();
            payload.buttons = o.buttons();
        }

        if payload.buttons.is_empty()
            && let Some(ev) = event(event_key)
            && let Some((ru, en, target)) = ev.default_button
        {
            let label = match lang {
                Lang::Ru => ru,
                Lang::En => en,
            };
            if let Some(url) = deep_link(bot_username, mini_app_short_name, target) {
                payload.buttons.push((label.to_string(), url));
            }
        }

        Rendered {
            payload,
            title,
            body,
        }
    }
}

/// Ссылка на экран мини-аппа, или `None`, если даже имени бота нет.
///
/// Без `short_name` ведём на сам чат бота: у него нет нужного экрана, но ссылка
/// живая и человек хотя бы попадает в продукт.
pub fn deep_link(
    bot_username: Option<&str>,
    mini_app_short_name: &str,
    target: ButtonTarget,
) -> Option<String> {
    let bot = bot_username?.trim().trim_start_matches('@');
    if bot.is_empty() {
        return None;
    }
    let short = mini_app_short_name.trim();
    if short.is_empty() {
        return Some(format!("https://t.me/{bot}"));
    }
    Some(format!(
        "https://t.me/{bot}/{short}?startapp={}",
        target.slug()
    ))
}

/// Текст переопределения одним запросом, без кэша.
///
/// Единственный потребитель — `notification_service::notify_sni_rotation`: он
/// живёт вне `AppState`, шлёт сообщения сырым ботом и рассылает раз в ротацию,
/// а не свипом по всем пользователям. Один запрос на всю рассылку здесь дешевле,
/// чем протаскивание сервиса с кэшем через двух вызывающих и конструктор
/// телеметрии. Ошибка чтения = `None` = встроенная строка.
pub async fn text_override(pool: &PgPool, event: &str, lang: Lang) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT text FROM notification_templates WHERE event = $1 AND lang = $2",
    )
    .bind(event)
    .bind(lang.as_str())
    .fetch_optional(pool)
    .await
    .ok()
    // Два уровня Option: внешний — «нашлась ли строка», внутренний — «text NULL
    // или нет». Оба означают одно и то же для вызывающего: переопределения нет.
    .flatten()
    .flatten()
    .filter(|t: &String| !t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::translations::{MISSING_FOR_TESTS, t, tf};

    /// Реестр не должен разъехаться с таблицей переводов. Если кто-то
    /// переименует ключ в одном месте, уведомление уедет пользователю строкой
    /// `???` — тест ловит это раньше, чем оно случится.
    #[test]
    fn every_event_has_both_languages() {
        for ev in REGISTRY {
            for key in [
                ev.key.to_string(),
                format!("{}_title", ev.key),
                format!("{}_body", ev.key),
            ] {
                for lang in [Lang::Ru, Lang::En] {
                    assert_ne!(
                        t(lang, &key),
                        MISSING_FOR_TESTS,
                        "нет перевода для {key} ({lang:?})"
                    );
                }
            }
        }
    }

    /// Индексы во ВСТРОЕННЫХ строках обязаны укладываться в арность события —
    /// та же проверка, что стоит на сохранении в панели. Иначе редактор
    /// показывал бы оператору подсказку, которой сам дефолт не соответствует.
    #[test]
    fn builtin_strings_respect_their_own_arity() {
        for ev in REGISTRY {
            for key in [
                ev.key.to_string(),
                format!("{}_title", ev.key),
                format!("{}_body", ev.key),
            ] {
                for lang in [Lang::Ru, Lang::En] {
                    assert!(
                        ev.validate_placeholders(t(lang, &key)).is_ok(),
                        "{key} ({lang:?}) использует подстановку, которой нет у события"
                    );
                }
            }
        }
    }

    #[test]
    fn validation_rejects_an_index_the_event_does_not_have() {
        let ev = event("notify.low_balance").expect("событие есть в реестре");
        assert_eq!(ev.args.len(), 2);
        assert!(ev.validate_placeholders("баланс {0} на тарифе {1}").is_ok());

        let err = ev
            .validate_placeholders("баланс {0}, тариф {1}, лишнее {2}")
            .expect_err("{2} за пределами арности");
        assert!(err.contains("{2}"), "в тексте ошибки должен быть индекс: {err}");
    }

    #[test]
    fn validation_ignores_braces_that_are_not_indices() {
        let ev = event("notify.expiry3").expect("событие есть в реестре");
        assert!(ev.args.is_empty());
        // Фигурные скобки в тексте — не подстановка, и запрещать их не за что.
        assert!(ev.validate_placeholders("смайл {ツ} и {}").is_ok());
    }

    /// Пустой кэш обязан давать ровно то, что уходило до появления шаблонов.
    /// Это главный инвариант: установка, где редактор не открывали, не должна
    /// заметить, что фича появилась.
    #[test]
    fn no_override_matches_the_builtin_string() {
        let args = ["Premium", "2.40"];
        assert_eq!(
            substitute(t(Lang::Ru, "notify.low_balance"), &args),
            tf(Lang::Ru, "notify.low_balance", &args)
        );
    }

    #[test]
    fn deep_link_needs_a_bot_and_degrades_without_a_short_name() {
        assert_eq!(deep_link(None, "app", ButtonTarget::Billing), None);
        assert_eq!(deep_link(Some("  "), "app", ButtonTarget::Billing), None);
        assert_eq!(
            deep_link(Some("@exabot"), "", ButtonTarget::Billing).as_deref(),
            Some("https://t.me/exabot"),
            "без short name ведём на бота, а не собираем битую ссылку"
        );
        assert_eq!(
            deep_link(Some("exabot"), "app", ButtonTarget::Plans).as_deref(),
            Some("https://t.me/exabot/app?startapp=plans")
        );
    }

    #[test]
    fn buttons_survive_a_malformed_json_column() {
        let mut over = TemplateOverride {
            buttons_json: Some(serde_json::json!("не массив")),
            ..Default::default()
        };
        assert!(over.buttons().is_empty());

        over.buttons_json = Some(serde_json::json!([{"text": "  ", "url": "https://x"}]));
        assert!(over.buttons().is_empty(), "пустая подпись — не кнопка");

        over.buttons_json = Some(serde_json::json!([{"text": "Оплатить", "url": "https://x"}]));
        assert_eq!(over.buttons().len(), 1);
    }
}
