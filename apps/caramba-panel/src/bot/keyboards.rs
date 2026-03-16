use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

pub fn main_menu(app_mode: bool, always_support: bool) -> KeyboardMarkup {
    if app_mode {
        let mut row = Vec::new();
        if always_support {
            row.push(KeyboardButton::new("❓ Support"));
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
            KeyboardButton::new("🛍 Buy Subscription"),
            KeyboardButton::new("🔐 My Services"),
        ],
        vec![
            KeyboardButton::new("📦 Digital Store"),
            KeyboardButton::new("👤 My Profile"),
        ],
        vec![
            KeyboardButton::new("🎁 Bonuses / Referral"),
            KeyboardButton::new("❓ Support"),
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

pub fn terms_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("✅ Accept", "accept_terms"),
        InlineKeyboardButton::callback("❌ Decline", "decline_terms"),
    ]])
}
