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
        let mut row = Vec::new();
        if always_support {
            row.push(KeyboardButton::new(t(lang, "menu.support")));
        }

        if row.is_empty() {
            // Return empty markup or hidden
            return KeyboardMarkup::new(Vec::<Vec<KeyboardButton>>::new()).resize_keyboard();
        } else {
            return KeyboardMarkup::new(vec![row]).resize_keyboard();
        }
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
        vec![KeyboardButton::new(t(lang, "menu.open_app"))],
    ])
    .resize_keyboard()
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
