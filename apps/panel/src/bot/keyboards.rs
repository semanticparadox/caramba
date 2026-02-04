use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

pub fn main_menu() -> KeyboardMarkup {
    // Requires ADMIN_PANEL_URL env var or we hardcode for now for MVP testing
    // Better to use a clean URL.
    // For now we assume the frontend is hosting the mini app.
    let web_app_url = std::env::var("MINI_APP_URL").unwrap_or_else(|_| "https://google.com".to_string());
    
    markup_with_webapp(&web_app_url)
}

pub fn markup_with_webapp(_url: &str) -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            // WebApp method not supported in this Teloxide version for KeyboardButton
            // Users should use the persistent Menu Button set in /start
            KeyboardButton::new("🚀 Open App (Use Menu Button👇)")
        ],
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
