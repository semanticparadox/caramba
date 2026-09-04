use crate::bot::translations::{Lang, t};
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

/// Главное меню (reply keyboard).
///
/// Подписи кнопок локализованы. Нажатие такой кнопки приходит боту обычным
/// текстовым сообщением, поэтому `command.rs::menu_action` распознаёт их на
/// обоих языках (плюс старые английские подписи как legacy-алиасы — у клиентов
/// уже отрисованные клавиатуры не обновляются сами).
pub fn main_menu(lang: Lang, app_mode: bool, always_support: bool) -> KeyboardMarkup {
    if app_mode {
        // Режим «только приложение» обязан оставлять дорогу В приложение.
        //
        // Раньше он прятал всё, кроме поддержки, — включая единственную кнопку,
        // по которой бот отдаёт ссылку caramba://connect. Выпуск ссылки при этом
        // работал и был выкачен, но нажать было негде: на боевой панели в этом
        // режиме за всё время не выдалось ни одного кода. Функция, до которой
        // нельзя дотянуться, ничем не отличается от отсутствующей.
        let mut row = vec![KeyboardButton::new(t(lang, "menu.open_app"))];
        if always_support {
            row.push(KeyboardButton::new(t(lang, "menu.support")));
        }
        return KeyboardMarkup::new(vec![row]).resize_keyboard();
    }

    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new(t(lang, "menu.buy")),
            KeyboardButton::new(t(lang, "menu.services")),
        ],
        vec![
            KeyboardButton::new(t(lang, "menu.store")),
            KeyboardButton::new(t(lang, "menu.profile")),
        ],
        vec![
            KeyboardButton::new(t(lang, "menu.referral")),
            KeyboardButton::new(t(lang, "menu.support")),
        ],
        vec![
            KeyboardButton::new(t(lang, "menu.guides")),
            KeyboardButton::new(t(lang, "menu.open_app")),
        ],
    ])
    .resize_keyboard()
}

/// Платформы инструкций в порядке показа; ключ настройки — `guide_url_{id}`.
pub const GUIDE_PLATFORMS: [&str; 7] = [
    "ios", "android", "windows", "macos", "linux", "tv", "router",
];

/// Инлайн-кнопки со ссылками на инструкции (Telegraph). Адреса лежат в
/// настройках панели, чтобы менять их без релиза; пустые пропускаются.
/// `None` — если не опубликована ни одна страница.
pub async fn guides_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();
    for id in GUIDE_PLATFORMS {
        let url = settings
            .get_or_default(&format!("guide_url_{id}"), "")
            .await;
        let Ok(parsed) = url.trim().parse::<reqwest::Url>() else {
            continue;
        };
        row.push(InlineKeyboardButton::url(
            t(lang, &format!("guides.{id}")),
            parsed,
        ));
        // Роутер — отдельной строкой, остальные по две.
        if row.len() == 2 || id == "router" {
            rows.push(std::mem::take(&mut row));
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    }
}

/// Одна кнопка «Пошаговая инструкция» — к сообщению со ссылками подписки.
pub async fn guide_index_button(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let url = settings.get_or_default("guide_url_index", "").await;
    let parsed = url.trim().parse::<reqwest::Url>().ok()?;
    Some(InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::url(t(lang, "guides.index_btn"), parsed),
    ]]))
}

/// Инлайн-клавиатура с кнопкой получения одноразового кода для входа в
/// standalone-приложение. Используется в приветствии и где удобно.
pub fn login_code_keyboard(lang: Lang) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        t(lang, "login.get_code_btn"),
        "get_login_code",
    )]])
}

/// Выбор языка. Намеренно двуязычная — показывается до того, как язык известен.
pub fn language_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("🇺🇸 English", "set_lang_en"),
        InlineKeyboardButton::callback("🇷🇺 Русский", "set_lang_ru"),
    ]])
}

pub fn terms_keyboard(lang: Lang) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(t(lang, "terms.accept"), "accept_terms"),
        InlineKeyboardButton::callback(t(lang, "terms.decline"), "decline_terms"),
    ]])
}
