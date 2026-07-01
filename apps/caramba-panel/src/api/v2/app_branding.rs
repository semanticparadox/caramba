//! Публичный branding-эндпоинт standalone-приложения Caramba Connect.
//!
//! Контракт A: GET /api/v2/app/branding (ПУБЛИЧНЫЙ, без JWT — нужен ДО логина,
//! чтобы клиент показал правильный бренд на экране входа). Ответ:
//!   { enabled, brand_name, logo_url, accent_hex, support_url, bot_url,
//!     upstream_ads }
//!
//! Источник истины — settings(key,value) (контракт B). Бренд ПИШЕТ бот
//! (admin-команды), а ЭТОТ эндпоинт только ЧИТАЕТ ключи brand_* и накладывает
//! тир-гейт из модуля `license` (контракт C):
//!   • enabled       = limits.branding && brand_enabled=="true" && brand_name≠""
//!   • upstream_ads  = limits.upstream_ads || !enabled
//! На Free-тире (дефолт) или без сконфигурированного бренда: enabled=false,
//! upstream_ads=true — клиент рендерит дефолтный вид Caramba Connect + powered-by.
//!
//! Инвариант утечки: отдаём ТОЛЬКО шесть brand_* полей + два флага. Никаких
//! секретов; эндпоинт строго READ-ONLY (пишет бот через /api/v2/bot/*).

use crate::license;
use crate::AppState;
use axum::{extract::State, response::Json};
use serde::Serialize;

/// Ответ branding-эндпоинта. Поля brand_* заполняются только при enabled=true;
/// иначе отдаём пустые строки — клиент тогда использует дефолтный kBrandName
/// и общий вид Caramba Connect.
#[derive(Serialize)]
pub struct BrandingResponse {
    pub enabled: bool,
    pub brand_name: String,
    pub logo_url: String,
    pub accent_hex: String,
    pub support_url: String,
    pub bot_url: String,
    pub upstream_ads: bool,
}

/// GET /api/v2/app/branding — публичный, READ-ONLY.
///
/// Всегда 200: это не аутентификация, а раннее получение бренда. Гейт тира
/// вычисляется через `effective_tier` (P3: settings-ключ `license_tier`; P4
/// заменит источник на проверенную лицензию — сигнатура неизменна).
pub async fn get_branding(State(state): State<AppState>) -> Json<BrandingResponse> {
    let tier = license::effective_tier(&state).await;
    let lim = license::limits_for(tier);

    // Читаем шесть brand_* ключей (дефолты пустые/false; их сеет миграция).
    let brand_enabled = state
        .settings
        .get_bool_or_default("brand_enabled", false)
        .await;
    let brand_name = state.settings.get_or_default("brand_name", "").await;
    let logo_url = state.settings.get_or_default("brand_logo_url", "").await;
    let accent_hex = state.settings.get_or_default("brand_accent_hex", "").await;
    let support_url = state
        .settings
        .get_or_default("brand_support_url", "")
        .await;
    let bot_url = state.settings.get_or_default("brand_bot_url", "").await;

    // Гейт: бренд активен только если тир разрешает И оператор включил И задал имя.
    let enabled = lim.branding && brand_enabled && !brand_name.trim().is_empty();

    // upstream_ads: на тире с рекламой ИЛИ когда бренд не активен (дефолтный вид).
    let upstream_ads = lim.upstream_ads || !enabled;

    if enabled {
        Json(BrandingResponse {
            enabled: true,
            brand_name: brand_name.trim().to_string(),
            logo_url,
            accent_hex,
            support_url,
            bot_url,
            upstream_ads,
        })
    } else {
        // Дефолтный вид Caramba Connect: пустые brand_* -> клиент берёт kBrandName.
        Json(BrandingResponse {
            enabled: false,
            brand_name: String::new(),
            logo_url: String::new(),
            accent_hex: String::new(),
            support_url: String::new(),
            bot_url: String::new(),
            upstream_ads,
        })
    }
}
