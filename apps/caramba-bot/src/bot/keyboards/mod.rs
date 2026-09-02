pub mod admin;

use crate::bot::translations::t;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

pub fn main_menu(lang: Option<&str>) -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new(t(lang, "kb.buy_sub")),
            KeyboardButton::new(t(lang, "kb.my_services")),
        ],
        vec![
            KeyboardButton::new(t(lang, "kb.digital_store")),
            KeyboardButton::new(t(lang, "kb.my_profile")),
        ],
        vec![
            KeyboardButton::new(t(lang, "kb.bonuses")),
            KeyboardButton::new(t(lang, "kb.support")),
        ],
        vec![KeyboardButton::new(t(lang, "kb.guides"))],
    ])
    .resize_keyboard()
}

/// Платформы инструкций в порядке показа; ключ настройки — `guide_url_{id}`.
pub const GUIDE_PLATFORMS: [&str; 7] = [
    "ios", "android", "windows", "macos", "linux", "tv", "router",
];

/// Инлайн-кнопки со ссылками на инструкции (Telegraph). Адреса лежат в
/// настройках панели, чтобы менять их без релиза; пустые пропускаются.
/// Пусто — если не опубликована ни одна страница.
pub async fn guides_keyboard(
    settings: &crate::services::settings_service::SettingsService,
    lang: Option<&str>,
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
            t(lang, &format!("kb.guide_{id}")),
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
    settings: &crate::services::settings_service::SettingsService,
    lang: Option<&str>,
) -> Option<InlineKeyboardMarkup> {
    let url = settings.get_or_default("guide_url_index", "").await;
    let parsed = url.trim().parse::<reqwest::Url>().ok()?;
    Some(InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::url(t(lang, "kb.guide_index"), parsed),
    ]]))
}

pub fn language_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("🇺🇸 English", "set_lang_en"),
        InlineKeyboardButton::callback("🇷🇺 Русский", "set_lang_ru"),
    ]])
}

pub fn terms_keyboard(lang: Option<&str>) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(t(lang, "kb.accept"), "accept_terms"),
        InlineKeyboardButton::callback(t(lang, "kb.decline"), "decline_terms"),
    ]])
}

pub fn make_amount_keyboard(prefix: &str) -> InlineKeyboardMarkup {
    let amounts = [5.0, 10.0, 20.0, 50.0, 100.0];
    let mut grid = Vec::new();

    let mut row = Vec::new();
    for (i, amt) in amounts.iter().enumerate() {
        row.push(InlineKeyboardButton::callback(
            format!("${}", amt),
            format!("{}_{}", prefix, amt),
        ));
        if (i + 1) % 3 == 0 {
            grid.push(row);
            row = Vec::new();
        }
    }
    if !row.is_empty() {
        grid.push(row);
    }

    InlineKeyboardMarkup::new(grid)
}
