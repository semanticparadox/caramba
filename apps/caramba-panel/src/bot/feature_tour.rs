//! «Тур по функциям»: мастер-пост со списком возможностей сервиса и по кнопке
//! на каждую — отдельное сообщение с анимацией и подписью.
//!
//! ЗАЧЕМ. Рассказать про функции приложения одним письмом нельзя: список
//! получается на три экрана, и его не читают. Тур разбивает рассказ на семь
//! коротких сообщений, а переход между ними делает сам человек — нажал
//! «Автоподбор», получил гифку автоподбора и кнопки на остальные функции.
//! Мастер-пост (`overview`) уходит рассылкой и командой `/tour`; кнопки в нём —
//! обычные callback-кнопки бота, поэтому рассылка не зависит от того, что́ мы
//! успели загрузить: подписи и анимации меняются настройкой, без релиза.
//!
//! # Хранение
//!
//! Весь контент лежит в ОДНОЙ настройке панели [`SETTING_KEY`] — JSON-массив
//! элементов. Дефолт вшит в бинарь ([`DEFAULT_JSON`]), поэтому сразу после
//! релиза тур работает без единой настройки; заданная и валидная настройка
//! замещает дефолт ЦЕЛИКОМ (не сливается по полям — иначе оператор не смог бы
//! убрать элемент).
//!
//! Формат элемента (лишние ключи, например `scene`, игнорируются):
//!
//! ```json
//! [
//!   {
//!     "id": "overview",
//!     "title": "Что такое Caramba Connect",
//!     "button": "✨ Что умеет сервис",
//!     "button_en": "✨ What the service can do",
//!     "animation_file_id": "",
//!     "caption_html": "🚢 <b>…</b>",
//!     "caption_html_en": "🚢 <b>…</b>",
//!     "buttons": [["📥 Скачать приложение", "https://t.me/exa_robot?start=apk"]]
//!   }
//! ]
//! ```
//!
//! * `id` — `[a-z_]{1,32}`, уникален, уезжает в callback-данные как
//!   `tour_<id>`; элемент с `id = "overview"` обязателен, с него начинается тур.
//! * `button` / `button_en` — подпись callback-кнопки на этот элемент
//!   в сообщениях ДРУГИХ элементов. `button_en` необязателен.
//! * `animation_file_id` — `file_id` MP4, уже загруженного этому боту.
//!   Пусто или ключа нет — уходит текстом, без анимации.
//! * `caption_html` / `caption_html_en` — подпись под анимацией, HTML,
//!   не длиннее [`MAX_CAPTION_CHARS`] символов (лимит Telegram на `caption`).
//!   Английской нет — берётся русская.
//! * `buttons` — кнопки-ссылки этого элемента, пары `[текст, url]`, http/https.
//!
//! # Как проставить `file_id`
//!
//! Анимации грузит оператор скриптом, а не админка: `file_id` привязан к
//! конкретному боту, выдаёт его сам Telegram в ответ на `sendAnimation`, и
//! перекладывать 7 файлов через веб-форму ради строки в JSON смысла нет.
//! Порядок: `sendAnimation` каждого MP4 в служебный чат бота → взять
//! `result.animation.file_id` → вписать в поле `animation_file_id` того же
//! элемента → вставить весь массив в Settings → «Feature tour».

use crate::AppState;
use crate::bot::translations::{Lang, t};
use crate::settings::SettingsService;
use serde::Deserialize;
use teloxide::prelude::*;
use teloxide::types::{
    ChatId, FileId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, ParseMode,
};
use tracing::{error, info, warn};

/// Ключ настройки панели с JSON тура.
pub const SETTING_KEY: &str = "feature_tour_json";

/// Префикс callback-данных: `tour_<id>`. Лимит Telegram на `callback_data` —
/// 64 БАЙТА; при `id` не длиннее 32 ASCII-символов запас двукратный, и это
/// проверяется тестом, а не на глаз.
pub const CALLBACK_PREFIX: &str = "tour_";

/// Элемент, с которого начинается тур: мастер-пост.
pub const OVERVIEW_ID: &str = "overview";

/// Callback-данные кнопки «открыть тур» для чужих клавиатур: одно место, где
/// префикс и `id` мастер-поста складываются, — иначе они разъедутся молча.
pub fn overview_callback_data() -> String {
    format!("{CALLBACK_PREFIX}{OVERVIEW_ID}")
}

/// Лимит Telegram на подпись к медиа. Длиннее — Telegram отклонит ОТПРАВКУ
/// целиком, поэтому длина проверяется при разборе, а не при показе.
pub const MAX_CAPTION_CHARS: usize = 1024;

/// Максимальная длина `id`: с ним `tour_<id>` заведомо влезает в 64 байта.
const MAX_ID_LEN: usize = 32;

/// Контент по умолчанию — тот же массив, что оператор видит в настройке.
/// Лежит файлом рядом, а не строкой в коде: его правит человек, и diff на
/// правку подписи должен читаться как diff текста, а не как правка Rust.
pub const DEFAULT_JSON: &str = include_str!("feature_tour_default.json");

/// Один элемент тура.
#[derive(Debug, Clone, Deserialize)]
pub struct Feature {
    pub id: String,
    /// Служебное название для оператора и логов; в чат не уходит.
    #[serde(default)]
    pub title: String,
    /// Подпись callback-кнопки на этот элемент.
    pub button: String,
    #[serde(default)]
    pub button_en: Option<String>,
    /// `file_id` анимации; пусто — элемент уходит текстом.
    #[serde(default)]
    pub animation_file_id: String,
    pub caption_html: String,
    #[serde(default)]
    pub caption_html_en: Option<String>,
    /// Кнопки-ссылки: пары `[текст, url]`.
    #[serde(default)]
    pub buttons: Vec<Vec<String>>,
}

impl Feature {
    /// Подпись сообщения на языке пользователя. Английской нет — русская:
    /// показать русский текст лучше, чем не показать ничего.
    pub fn caption(&self, lang: Lang) -> &str {
        match (lang, self.caption_html_en.as_deref()) {
            (Lang::En, Some(en)) if !en.trim().is_empty() => en,
            _ => &self.caption_html,
        }
    }

    /// Подпись кнопки на этот элемент на языке пользователя.
    pub fn button_label(&self, lang: Lang) -> &str {
        match (lang, self.button_en.as_deref()) {
            (Lang::En, Some(en)) if !en.trim().is_empty() => en,
            _ => &self.button,
        }
    }

    /// Callback-данные кнопки на этот элемент.
    pub fn callback_data(&self) -> String {
        format!("{CALLBACK_PREFIX}{}", self.id)
    }

    /// Разобранные кнопки-ссылки. Валидность проверена в [`parse`], поэтому
    /// здесь битые пары просто пропускаются — падать в момент отправки нельзя.
    fn url_buttons(&self) -> Vec<InlineKeyboardButton> {
        self.buttons
            .iter()
            .filter_map(|pair| {
                let text = pair.first()?;
                let url = pair.get(1)?.parse::<reqwest::Url>().ok()?;
                Some(InlineKeyboardButton::url(text.clone(), url))
            })
            .collect()
    }
}

/// Разобранный тур: непустой список элементов, `overview` гарантированно есть.
#[derive(Debug, Clone)]
pub struct FeatureTour {
    features: Vec<Feature>,
}

impl FeatureTour {
    /// Тур из настройки панели; настройка пуста или невалидна — дефолт.
    ///
    /// Невалидная настройка логируется как ошибка: админка не даёт сохранить
    /// мусор, значит он мог приехать только правкой БД руками, и тихо
    /// подставленный дефолт иначе выглядел бы как «настройка не применилась».
    pub async fn load(settings: &SettingsService) -> FeatureTour {
        let raw = settings.get_or_default(SETTING_KEY, "").await;
        if raw.trim().is_empty() {
            return FeatureTour::default_tour();
        }
        match parse(&raw) {
            Ok(features) => FeatureTour { features },
            Err(e) => {
                error!("feature tour: setting `{SETTING_KEY}` is invalid ({e}), using default");
                FeatureTour::default_tour()
            }
        }
    }

    /// Вшитый в бинарь контент. Его валидность — инвариант сборки
    /// (тест `default_json_parses`), поэтому здесь `expect`.
    pub fn default_tour() -> FeatureTour {
        FeatureTour {
            features: parse(DEFAULT_JSON).expect("built-in feature tour JSON must be valid"),
        }
    }

    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// Элемент по `id`; неизвестный `id` (кнопка из старого сообщения после
    /// правки настройки) сводится к мастер-посту, а не к молчанию.
    pub fn get_or_overview(&self, id: &str) -> &Feature {
        self.features
            .iter()
            .find(|f| f.id == id)
            .unwrap_or_else(|| self.overview())
    }

    /// Мастер-пост. Наличие проверено в [`parse`].
    pub fn overview(&self) -> &Feature {
        self.features
            .iter()
            .find(|f| f.id == OVERVIEW_ID)
            .expect("parse() guarantees an `overview` element")
    }
}

/// Ошибка разбора JSON тура. Текст уходит оператору в ответ на сохранение
/// настройки, поэтому он говорит, ЧТО именно не так и в каком элементе.
pub fn parse(json: &str) -> Result<Vec<Feature>, String> {
    let features: Vec<Feature> =
        serde_json::from_str(json).map_err(|e| format!("not a valid JSON array: {e}"))?;

    if features.is_empty() {
        return Err("the tour is empty: at least the `overview` element is required".to_string());
    }

    let mut seen: Vec<&str> = Vec::with_capacity(features.len());
    for f in &features {
        // `id` уезжает в callback_data, а её длину и алфавит Telegram не
        // прощает: 64 байта и никакого мусора, иначе кнопка не отрисуется.
        if f.id.is_empty()
            || f.id.len() > MAX_ID_LEN
            || !f.id.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        {
            return Err(format!(
                "id `{}`: expected 1..={MAX_ID_LEN} characters of a-z and _",
                f.id
            ));
        }
        if seen.contains(&f.id.as_str()) {
            return Err(format!("id `{}` appears twice", f.id));
        }
        seen.push(&f.id);

        if f.button.trim().is_empty() {
            return Err(format!(
                "id `{}`: `button` (the button label) is empty",
                f.id
            ));
        }

        for (field, text) in [
            ("caption_html", Some(f.caption_html.as_str())),
            ("caption_html_en", f.caption_html_en.as_deref()),
        ] {
            let Some(text) = text else { continue };
            if field == "caption_html" && text.trim().is_empty() {
                return Err(format!("id `{}`: `caption_html` is empty", f.id));
            }
            // Считаем СИМВОЛЫ, а не байты: лимит Telegram символьный, а
            // русская подпись в UTF-8 весит вдвое больше своей длины.
            let len = text.chars().count();
            if len > MAX_CAPTION_CHARS {
                return Err(format!(
                    "id `{}`: `{field}` is {len} characters, the limit is {MAX_CAPTION_CHARS}",
                    f.id
                ));
            }
        }

        for (i, pair) in f.buttons.iter().enumerate() {
            if pair.len() != 2 {
                return Err(format!(
                    "id `{}`, button #{}: expected a [text, url] pair",
                    f.id,
                    i + 1
                ));
            }
            if pair[0].trim().is_empty() {
                return Err(format!(
                    "id `{}`, button #{}: the label is empty",
                    f.id,
                    i + 1
                ));
            }
            // Telegram принимает URL-кнопку только с http/https (tg:// и
            // прочие схемы отклоняются вместе со всем сообщением).
            let url = pair[1]
                .parse::<reqwest::Url>()
                .map_err(|e| format!("id `{}`, button #{}: bad url ({e})", f.id, i + 1))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(format!(
                    "id `{}`, button #{}: url must start with http:// or https://",
                    f.id,
                    i + 1
                ));
            }
        }
    }

    if !seen.contains(&OVERVIEW_ID) {
        return Err(format!(
            "no `{OVERVIEW_ID}` element: the tour has nothing to start from"
        ));
    }

    Ok(features)
}

/// Клавиатура сообщения одного элемента тура.
///
/// ПОРЯДОК: сначала кнопки-ссылки самого элемента (по две в ряд), затем
/// callback-кнопки на ОСТАЛЬНЫЕ функции (тоже по две), последней строкой —
/// «📖 Инструкция», если её адрес задан. Текущий элемент из списка исключён:
/// кнопка, возвращающая в то же самое сообщение, выглядит как поломка.
pub fn keyboard(
    tour: &FeatureTour,
    current: &Feature,
    lang: Lang,
    guide_index_url: Option<reqwest::Url>,
) -> InlineKeyboardMarkup {
    let mut rows = two_per_row(current.url_buttons());

    let others: Vec<InlineKeyboardButton> = tour
        .features()
        .iter()
        .filter(|f| f.id != current.id)
        .map(|f| InlineKeyboardButton::callback(f.button_label(lang), f.callback_data()))
        .collect();
    rows.extend(two_per_row(others));

    if let Some(url) = guide_index_url {
        rows.push(vec![InlineKeyboardButton::url(t(lang, "menu.guides"), url)]);
    }

    InlineKeyboardMarkup::new(rows)
}

/// По две кнопки в ряд: в столбик семь подписей растягивают сообщение на весь
/// экран телефона, а по две ещё читаются.
fn two_per_row(buttons: Vec<InlineKeyboardButton>) -> Vec<Vec<InlineKeyboardButton>> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for button in buttons {
        match rows.last_mut() {
            Some(row) if row.len() < 2 => row.push(button),
            _ => rows.push(vec![button]),
        }
    }
    rows
}

/// Адрес корневой страницы инструкций для последней строки клавиатуры.
async fn guide_index_url(settings: &SettingsService) -> Option<reqwest::Url> {
    let url = settings.get_or_default("guide_url_index", "").await;
    url.trim().parse::<reqwest::Url>().ok()
}

/// Отправляет элемент тура в чат: анимация с подписью или, если `file_id` нет
/// либо он протух, тот же текст сообщением.
pub async fn send_feature(bot: &Bot, chat_id: ChatId, lang: Lang, state: &AppState, id: &str) {
    let tour = FeatureTour::load(&state.settings).await;
    let feature = tour.get_or_overview(id);
    let markup = keyboard(&tour, feature, lang, guide_index_url(&state.settings).await);
    let caption = feature.caption(lang);
    info!("feature tour: sending `{}` ({})", feature.id, feature.title);

    let file_id = feature.animation_file_id.trim();
    if !file_id.is_empty() {
        let animation = InputFile::file_id(FileId(file_id.to_string()));
        match bot
            .send_animation(chat_id, animation)
            .caption(caption)
            .parse_mode(ParseMode::Html)
            .reply_markup(markup.clone())
            .await
        {
            Ok(_) => return,
            Err(e) => {
                // Самая вероятная причина — «Bad Request: wrong file
                // identifier»: файл перезалили или удалили. Настройку не
                // трогаем (её правит человек), но рассказ не теряем: тот же
                // текст уходит без картинки.
                warn!(
                    "feature tour: send_animation by file_id failed for `{}`: {e}",
                    feature.id
                );
            }
        }
    }

    let _ = bot
        .send_message(chat_id, caption)
        .parse_mode(ParseMode::Html)
        .reply_markup(markup)
        .await
        .map_err(|e| error!("feature tour: failed to send `{}`: {e}", feature.id));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимальный валидный тур для тестов разбора.
    fn json(body: &str) -> String {
        format!(r#"[{{"id":"overview","button":"B","caption_html":"C"{body}}}]"#)
    }

    #[test]
    fn default_json_parses_and_has_every_feature() {
        let features = parse(DEFAULT_JSON).expect("built-in JSON must parse");
        let ids: Vec<&str> = features.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "overview",
                "autotune",
                "protocols",
                "rules",
                "apps",
                "entry",
                "devices"
            ]
        );
        for f in &features {
            assert!(!f.title.is_empty(), "`{}` without a title", f.id);
            assert!(
                f.caption_html_en.is_some(),
                "`{}` without an English caption",
                f.id
            );
            assert!(
                f.button_en.is_some(),
                "`{}` without an English button",
                f.id
            );
            // Анимации проставляет оператор: дефолт обязан уметь работать
            // текстом, иначе после релиза тур молчит до первой настройки.
            assert!(f.animation_file_id.is_empty());
        }
    }

    /// Клавиатура любого элемента дефолта укладывается в лимиты Telegram.
    #[test]
    fn callback_data_fits_telegram_limit() {
        let tour = FeatureTour::default_tour();
        assert_eq!(overview_callback_data(), "tour_overview");
        assert_eq!(tour.overview().callback_data(), overview_callback_data());
        for f in tour.features() {
            let data = f.callback_data();
            assert!(
                data.len() <= 64,
                "callback_data `{data}` is {} bytes",
                data.len()
            );
            assert_eq!(data, format!("tour_{}", f.id));
        }
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse("not json").is_err());
        assert!(parse("[]").is_err());
        // Нет мастер-поста.
        assert!(parse(r#"[{"id":"autotune","button":"B","caption_html":"C"}]"#).is_err());
        // Недопустимый id.
        assert!(parse(r#"[{"id":"Over View","button":"B","caption_html":"C"}]"#).is_err());
        assert!(parse(r#"[{"id":"","button":"B","caption_html":"C"}]"#).is_err());
        assert!(
            parse(&format!(
                r#"[{{"id":"{}","button":"B","caption_html":"C"}}]"#,
                "a".repeat(MAX_ID_LEN + 1)
            ))
            .is_err()
        );
        // Дубль id.
        assert!(
            parse(
                r#"[{"id":"overview","button":"B","caption_html":"C"},
                    {"id":"overview","button":"B","caption_html":"C"}]"#
            )
            .is_err()
        );
        // Пустая подпись кнопки и пустой текст.
        assert!(parse(r#"[{"id":"overview","button":" ","caption_html":"C"}]"#).is_err());
        assert!(parse(r#"[{"id":"overview","button":"B","caption_html":" "}]"#).is_err());
        // Обязательные поля отсутствуют.
        assert!(parse(r#"[{"id":"overview","button":"B"}]"#).is_err());
    }

    /// Лимит подписи считается в символах: 1024 русские буквы — это 2048 байт,
    /// и байтовая проверка отвергла бы валидный текст.
    #[test]
    fn caption_limit_counts_characters() {
        let ok = "я".repeat(MAX_CAPTION_CHARS);
        let too_long = "я".repeat(MAX_CAPTION_CHARS + 1);
        assert!(parse(&json(&format!(r#","caption_html_en":"{ok}""#))).is_ok());
        assert!(parse(&json(&format!(r#","caption_html_en":"{too_long}""#))).is_err());
        assert!(
            parse(&format!(
                r#"[{{"id":"overview","button":"B","caption_html":"{too_long}"}}]"#
            ))
            .is_err()
        );
    }

    #[test]
    fn parse_checks_link_buttons() {
        assert!(parse(&json(r#","buttons":[["Text","https://example.com"]]"#)).is_ok());
        assert!(parse(&json(r#","buttons":[["Text","http://example.com"]]"#)).is_ok());
        // Не пара.
        assert!(parse(&json(r#","buttons":[["Text"]]"#)).is_err());
        assert!(parse(&json(r#","buttons":[["a","https://e.com","b"]]"#)).is_err());
        // Пустая подпись, мусорный адрес, посторонняя схема.
        assert!(parse(&json(r#","buttons":[[" ","https://example.com"]]"#)).is_err());
        assert!(parse(&json(r#","buttons":[["Text","example.com"]]"#)).is_err());
        assert!(parse(&json(r#","buttons":[["Text","tg://resolve?domain=x"]]"#)).is_err());
    }

    /// Лишние ключи (`scene` из исходного контента) не ломают разбор.
    #[test]
    fn unknown_keys_are_ignored() {
        let parsed =
            parse(&json(r#","scene":"connect","whatever":123"#)).expect("extra keys are allowed");
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn language_falls_back_to_russian() {
        let tour = FeatureTour::default_tour();
        let f = tour.overview();
        assert_ne!(f.caption(Lang::En), f.caption(Lang::Ru));
        assert_ne!(f.button_label(Lang::En), f.button_label(Lang::Ru));

        let only_ru = parse(&json("")).unwrap();
        assert_eq!(only_ru[0].caption(Lang::En), only_ru[0].caption(Lang::Ru));
        assert_eq!(
            only_ru[0].button_label(Lang::En),
            only_ru[0].button_label(Lang::Ru)
        );
        // Пустая английская строка — тоже «перевода нет».
        let blank = parse(&json(r#","caption_html_en":"  ","button_en":"  ""#)).unwrap();
        assert_eq!(blank[0].caption(Lang::En), "C");
        assert_eq!(blank[0].button_label(Lang::En), "B");
    }

    /// Порядок строк клавиатуры и отсутствие в ней текущего элемента.
    #[test]
    fn keyboard_lists_other_features_two_per_row() {
        let tour = FeatureTour::default_tour();
        let current = tour.get_or_overview("autotune");
        let url: reqwest::Url = "https://telegra.ph/index".parse().unwrap();
        let kb = keyboard(&tour, current, Lang::Ru, Some(url));
        let rows = &kb.inline_keyboard;

        // 2 кнопки-ссылки элемента → одна строка, 6 остальных функций → 3
        // строки, плюс строка инструкции.
        assert_eq!(rows.len(), 5, "{rows:#?}");
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows.last().unwrap().len(), 1);
        assert_eq!(rows.last().unwrap()[0].text, t(Lang::Ru, "menu.guides"));

        let callbacks: Vec<String> = rows
            .iter()
            .flatten()
            .filter_map(|b| match &b.kind {
                teloxide::types::InlineKeyboardButtonKind::CallbackData(d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            callbacks,
            [
                "tour_overview",
                "tour_protocols",
                "tour_rules",
                "tour_apps",
                "tour_entry",
                "tour_devices"
            ],
            "текущий элемент не должен вести сам на себя"
        );
    }

    /// Без адреса инструкции последней строки просто нет.
    #[test]
    fn keyboard_without_guide_url_has_no_guide_row() {
        let tour = FeatureTour::default_tour();
        let kb = keyboard(&tour, tour.overview(), Lang::En, None);
        assert!(
            kb.inline_keyboard
                .iter()
                .flatten()
                .all(|b| b.text != t(Lang::En, "menu.guides"))
        );
        // Мастер-пост показывает все шесть остальных функций.
        let callbacks = kb
            .inline_keyboard
            .iter()
            .flatten()
            .filter(|b| {
                matches!(
                    b.kind,
                    teloxide::types::InlineKeyboardButtonKind::CallbackData(_)
                )
            })
            .count();
        assert_eq!(callbacks, tour.features().len() - 1);
    }

    /// Поле админки обязано называться ровно ключом настройки: разъехавшись,
    /// форма молча перестанет что-либо сохранять, а тур продолжит работать на
    /// дефолте — поломку было бы видно только по жалобе оператора.
    #[test]
    fn admin_form_field_matches_the_setting_key() {
        let template = include_str!("../../templates/settings.html");
        assert!(
            template.contains(&format!(r#"name="{SETTING_KEY}""#)),
            "в settings.html нет поля {SETTING_KEY}"
        );
        let handler = include_str!("../handlers/admin/settings.rs");
        assert!(
            handler.contains(&format!("pub {SETTING_KEY}: Option<String>")),
            "в SaveSettingsForm нет поля {SETTING_KEY}"
        );
    }

    /// Неизвестный `id` (кнопка из сообщения, отправленного до правки
    /// настройки) сводится к мастер-посту.
    #[test]
    fn unknown_id_falls_back_to_overview() {
        let tour = FeatureTour::default_tour();
        assert_eq!(tour.get_or_overview("no_such").id, OVERVIEW_ID);
        assert_eq!(tour.get_or_overview("devices").id, "devices");
    }
}
