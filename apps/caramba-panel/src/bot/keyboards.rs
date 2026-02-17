use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

pub fn main_menu() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![KeyboardButton::new("🛍 Buy Subscription"), KeyboardButton::new("🔐 My Services")],
        vec![KeyboardButton::new("📦 Digital Store"), KeyboardButton::new("👤 My Profile")],
        vec![KeyboardButton::new("🎁 Bonuses / Referral"), KeyboardButton::new("❓ Support")],
    ])
    .resize_keyboard()
}

pub fn language_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("🇺🇸 English", "set_lang_en"),
            InlineKeyboardButton::callback("🇷🇺 Русский", "set_lang_ru"),
        ]
    ])
}

pub fn terms_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("✅ Accept", "accept_terms"),
            InlineKeyboardButton::callback("❌ Decline", "decline_terms"),
        ]
    ])
}
