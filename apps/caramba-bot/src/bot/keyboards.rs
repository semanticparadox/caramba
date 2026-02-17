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

pub fn make_amount_keyboard(prefix: &str) -> InlineKeyboardMarkup {
    let amounts = vec![5.0, 10.0, 20.0, 50.0, 100.0];
    let mut grid = Vec::new();
    
    let mut row = Vec::new();
    for (i, amt) in amounts.iter().enumerate() {
        row.push(InlineKeyboardButton::callback(format!("${}", amt), format!("{}_{}", prefix, amt)));
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
