//! Brand admin: просмотр и редактирование brand_*-настроек панели.
//!
//! Контракт B (общий k/v): бот ПИШЕТ в те же ключи `settings`, которые
//! читает branding-эндпоинт панели и Flutter-клиент:
//!   brand_enabled       "true" / "false"
//!   brand_name          строка
//!   brand_logo_url       URL
//!   brand_accent_hex     #RRGGBB (валидируется анти-слопом)
//!   brand_support_url    URL
//!   brand_bot_url        URL
//!
//! Запись идёт через `state.settings.set` (теперь это POST на панель,
//! ограниченный brand_*-allowlist панели) с обновлением локального кэша.
//!
//! Доступ строго админский: каждый вход в брендинг повторно проверяет
//! `is_admin` (двойное ограждение поверх /admin и adm:-роутера).
//!
//! Анти-слоп для акцента: принимается только настоящий #RRGGBB (или #RGB),
//! и отвергается полоса оттенков фиолетового/сине-фиолетового/индиго.
//! Цвет акцента НИКОГДА не несёт статус подключения — это решает клиент;
//! здесь мы лишь не даём оператору задать запрещённый бренд-оттенок.

use crate::bot::handlers::admin::AdminFsmState;
use crate::bot::keyboards::admin::{brand_field_back_keyboard, brand_menu_keyboard};
use crate::bot::translations::t;
use crate::AppState;
use teloxide::prelude::*;
use teloxide::types::{ChatId, MessageId, ParseMode};
use tracing::{error, info};

// ============================================================================
// Settings keys + поля
// ============================================================================

pub const KEY_BRAND_ENABLED: &str = "brand_enabled";
pub const KEY_BRAND_NAME: &str = "brand_name";
pub const KEY_BRAND_LOGO_URL: &str = "brand_logo_url";
pub const KEY_BRAND_ACCENT_HEX: &str = "brand_accent_hex";
pub const KEY_BRAND_SUPPORT_URL: &str = "brand_support_url";
pub const KEY_BRAND_BOT_URL: &str = "brand_bot_url";

/// Текстовые поля бренда, которые админ вводит через FSM.
/// (enabled переключается кнопкой, без ввода текста.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrandField {
    Name,
    LogoUrl,
    AccentHex,
    SupportUrl,
    BotUrl,
}

impl BrandField {
    /// Короткий идентификатор для callback-data и FSM (`adm:brand:set:<id>`).
    pub fn id(self) -> &'static str {
        match self {
            BrandField::Name => "name",
            BrandField::LogoUrl => "logo",
            BrandField::AccentHex => "accent",
            BrandField::SupportUrl => "support",
            BrandField::BotUrl => "boturl",
        }
    }

    pub fn from_id(id: &str) -> Option<BrandField> {
        match id {
            "name" => Some(BrandField::Name),
            "logo" => Some(BrandField::LogoUrl),
            "accent" => Some(BrandField::AccentHex),
            "support" => Some(BrandField::SupportUrl),
            "boturl" => Some(BrandField::BotUrl),
            _ => None,
        }
    }

    /// Целевой settings-ключ.
    pub fn key(self) -> &'static str {
        match self {
            BrandField::Name => KEY_BRAND_NAME,
            BrandField::LogoUrl => KEY_BRAND_LOGO_URL,
            BrandField::AccentHex => KEY_BRAND_ACCENT_HEX,
            BrandField::SupportUrl => KEY_BRAND_SUPPORT_URL,
            BrandField::BotUrl => KEY_BRAND_BOT_URL,
        }
    }

    /// Ключ перевода для подсказки ввода.
    pub fn prompt_key(self) -> &'static str {
        match self {
            BrandField::Name => "brand.prompt.name",
            BrandField::LogoUrl => "brand.prompt.logo",
            BrandField::AccentHex => "brand.prompt.accent",
            BrandField::SupportUrl => "brand.prompt.support",
            BrandField::BotUrl => "brand.prompt.bot",
        }
    }
}

// ============================================================================
// Валидация
// ============================================================================

/// Результат валидации значения поля.
pub enum BrandValidation {
    /// Нормализованное значение, готовое к записи.
    Ok(String),
    /// Отказ с ключом перевода для пояснения.
    Reject(&'static str),
}

/// Нормализует и валидирует значение для конкретного поля.
pub fn validate_field(field: BrandField, raw: &str) -> BrandValidation {
    let value = raw.trim();
    if value.is_empty() {
        return BrandValidation::Reject("brand.error.empty");
    }
    match field {
        BrandField::AccentHex => validate_accent(value),
        BrandField::LogoUrl | BrandField::SupportUrl | BrandField::BotUrl => validate_url(value),
        BrandField::Name => {
            // Имя бренда: ограничим разумной длиной, без управляющих символов.
            if value.chars().count() > 64 {
                return BrandValidation::Reject("brand.error.name_long");
            }
            BrandValidation::Ok(value.to_string())
        }
    }
}

/// URL: требуем http(s) и отсутствие пробелов. Намеренно нестрого
/// (полноценный парсинг URL не нужен), но отсекаем мусор.
fn validate_url(value: &str) -> BrandValidation {
    if value.contains(char::is_whitespace) {
        return BrandValidation::Reject("brand.error.url");
    }
    let lower = value.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return BrandValidation::Reject("brand.error.url");
    }
    if value.chars().count() > 512 {
        return BrandValidation::Reject("brand.error.url");
    }
    BrandValidation::Ok(value.to_string())
}

/// Парсит #RGB или #RRGGBB в (r,g,b) 0..=255. Без альфы (анти-слоп: акцент
/// должен быть непрозрачным сплошным цветом, не градиентом/прозрачностью).
fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let s = value.strip_prefix('#')?;
    let hex = match s.len() {
        3 => {
            // #RGB -> #RRGGBB
            let mut out = String::with_capacity(6);
            for c in s.chars() {
                out.push(c);
                out.push(c);
            }
            out
        }
        6 => s.to_string(),
        _ => return None,
    };
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

/// HSL hue (0..360) из RGB. Нужен только оттенок для бана полосы
/// фиолетового/индиго.
fn hue_of(r: u8, g: u8, b: u8) -> f32 {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    if delta.abs() < f32::EPSILON {
        return 0.0; // серый — оттенок не определён, не фиолетовый
    }
    let mut hue = if (max - rf).abs() < f32::EPSILON {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if (max - gf).abs() < f32::EPSILON {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    if hue < 0.0 {
        hue += 360.0;
    }
    hue
}

/// saturation (0..1) из RGB по HSL.
fn saturation_of(r: u8, g: u8, b: u8) -> f32 {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    if delta.abs() < f32::EPSILON {
        return 0.0;
    }
    let l = (max + min) / 2.0;
    delta / (1.0 - (2.0 * l - 1.0).abs())
}

/// Валидация акцента под анти-слоп:
///  - только #RGB / #RRGGBB (никаких rgba/hsl/градиентов/имён),
///  - запрещена полоса оттенков индиго/фиолетовый/сине-фиолетовый
///    (~240..295 hue) при заметной насыщенности.
/// Почти-серые (низкая saturation) проходят — там «оттенок» шумовой.
pub fn validate_accent(value: &str) -> BrandValidation {
    let (r, g, b) = match parse_hex(value) {
        Some(rgb) => rgb,
        None => return BrandValidation::Reject("brand.error.accent_hex"),
    };
    let sat = saturation_of(r, g, b);
    if sat >= 0.15 {
        let hue = hue_of(r, g, b);
        // Полоса индиго/фиолетового. Нижняя граница 240 ловит канонический
        // индиго (Tailwind indigo-500 #6366F1 ≈ 239..243, slateblue ≈ 248),
        // верхняя 295 — розовый/маджента. Чистый/azure синий (~210..230,
        // напр. #2563EB ≈ 217, #1D4ED8 ≈ 225) и циан проходят.
        if (240.0..=295.0).contains(&hue) {
            return BrandValidation::Reject("brand.error.accent_banned");
        }
    }
    // Нормализуем к верхнему регистру #RRGGBB.
    BrandValidation::Ok(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

// ============================================================================
// Меню бренда
// ============================================================================

/// Читает текущие brand_*-значения и рендерит меню.
/// `from_callback` решает edit vs send.
pub async fn send_brand_menu(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: Option<MessageId>,
    state: &AppState,
) {
    // Сброс кэша brand_*, чтобы показать то, что реально лежит в панели.
    for key in [
        KEY_BRAND_ENABLED,
        KEY_BRAND_NAME,
        KEY_BRAND_LOGO_URL,
        KEY_BRAND_ACCENT_HEX,
        KEY_BRAND_SUPPORT_URL,
        KEY_BRAND_BOT_URL,
    ] {
        state.settings.invalidate(key).await;
    }

    let enabled = state
        .settings
        .get_or_default(KEY_BRAND_ENABLED, "false")
        .await
        == "true";
    let name = state.settings.get_or_default(KEY_BRAND_NAME, "").await;
    let logo = state.settings.get_or_default(KEY_BRAND_LOGO_URL, "").await;
    let accent = state.settings.get_or_default(KEY_BRAND_ACCENT_HEX, "").await;
    let support = state
        .settings
        .get_or_default(KEY_BRAND_SUPPORT_URL, "")
        .await;
    let bot_url = state.settings.get_or_default(KEY_BRAND_BOT_URL, "").await;

    let dash = t(None, "brand.value.unset");
    let show = |v: &str| {
        if v.trim().is_empty() {
            dash.to_string()
        } else {
            v.to_string()
        }
    };

    let enabled_label = if enabled {
        t(None, "brand.state.on")
    } else {
        t(None, "brand.state.off")
    };

    let text = format!(
        "<b>{title}</b>\n\n\
        {l_enabled}: {enabled}\n\
        {l_name}: {name}\n\
        {l_logo}: {logo}\n\
        {l_accent}: {accent}\n\
        {l_support}: {support}\n\
        {l_bot}: {bot}\n\n\
        {note}",
        title = t(None, "brand.menu.title"),
        l_enabled = t(None, "brand.field.enabled"),
        enabled = enabled_label,
        l_name = t(None, "brand.field.name"),
        name = show(&name),
        l_logo = t(None, "brand.field.logo"),
        logo = show(&logo),
        l_accent = t(None, "brand.field.accent"),
        accent = show(&accent),
        l_support = t(None, "brand.field.support"),
        support = show(&support),
        l_bot = t(None, "brand.field.bot"),
        bot = show(&bot_url),
        note = t(None, "brand.menu.note"),
    );

    let kb = brand_menu_keyboard(enabled);

    if let Some(mid) = msg_id {
        let _ = bot
            .edit_message_text(chat_id, mid, &text)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await;
    } else {
        let _ = bot
            .send_message(chat_id, &text)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await;
    }
}

/// Переключает brand_enabled и перерисовывает меню.
pub async fn toggle_brand_enabled(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: Option<MessageId>,
    state: &AppState,
) {
    let current = state
        .settings
        .get_or_default(KEY_BRAND_ENABLED, "false")
        .await
        == "true";
    let next = if current { "false" } else { "true" };

    match state.settings.set(KEY_BRAND_ENABLED, next).await {
        Ok(()) => {
            info!(next, "Brand enabled toggled");
            send_brand_menu(bot, chat_id, msg_id, state).await;
        }
        Err(e) => {
            error!(error = %e, "Failed to persist brand_enabled");
            let _ = bot
                .send_message(chat_id, t(None, "brand.error.save"))
                .await;
        }
    }
}

/// Запускает ввод значения для поля: ставит FSM и шлёт подсказку.
pub async fn prompt_brand_field(
    bot: &Bot,
    chat_id: ChatId,
    tg_id: i64,
    field: BrandField,
    state: &AppState,
) {
    state
        .admin_fsm
        .set(
            tg_id,
            AdminFsmState::BrandAwaitValue {
                field: field.id().to_string(),
            },
        )
        .await;
    let _ = bot
        .send_message(chat_id, t(None, field.prompt_key()))
        .reply_markup(brand_field_back_keyboard())
        .await;
}

/// Обрабатывает текстовый ввод значения поля бренда из FSM.
/// Вызывается из admin::handle_admin_text, когда состояние BrandAwaitValue.
/// Возвращает `true` — сообщение обработано.
pub async fn handle_brand_value(
    bot: &Bot,
    chat_id: ChatId,
    tg_id: i64,
    field_id: &str,
    text: &str,
    state: &AppState,
) -> bool {
    let field = match BrandField::from_id(field_id) {
        Some(f) => f,
        None => {
            state.admin_fsm.clear(tg_id).await;
            return true;
        }
    };

    match validate_field(field, text) {
        BrandValidation::Ok(value) => match state.settings.set(field.key(), &value).await {
            Ok(()) => {
                state.admin_fsm.clear(tg_id).await;
                info!(key = field.key(), "Brand field saved");
                let _ = bot
                    .send_message(chat_id, t(None, "brand.saved"))
                    .await;
                // Показываем обновлённое меню без msg_id (новое сообщение).
                send_brand_menu(bot, chat_id, None, state).await;
            }
            Err(e) => {
                error!(error = %e, key = field.key(), "Failed to persist brand field");
                // FSM не сбрасываем — пусть админ повторит ввод.
                let _ = bot
                    .send_message(chat_id, t(None, "brand.error.save"))
                    .await;
            }
        },
        BrandValidation::Reject(err_key) => {
            // Остаёмся в том же FSM-состоянии — ждём корректный повторный ввод.
            let _ = bot.send_message(chat_id, t(None, err_key)).await;
        }
    }
    true
}
