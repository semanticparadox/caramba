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
    ])
    .resize_keyboard()
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
