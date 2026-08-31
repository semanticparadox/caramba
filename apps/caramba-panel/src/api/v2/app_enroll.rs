//! Публичная валидация кодов вовлечения (enrollment) для standalone-приложения
//! Caramba Connect.
//!
//! Контракт B: GET /api/v2/app/enroll/{code} -> JSON
//!   { valid: bool, reason?: string, panel_name?: string, onboarding_traffic_mb: i64 }
//!
//! Эндпоинт ПУБЛИЧНЫЙ (без JWT): клиент вызывает его при открытии диплинка
//! carambaconnect://enroll, ДО регистрации/логина. Чисто READ-ONLY — НЕ списывает
//! использование кода (consume происходит при создании аккаунта, не здесь).
//!
//! `onboarding_traffic_mb` — лимит бесплатного плана в МБ (см. `read_onboarding_mb`).
//!
//! PII-инвариант: ответ содержит ТОЛЬКО признак валидности, обобщённую причину,
//! имя панели и число онбординг-трафика. Никогда — личность пригласившего, email,
//! used_count/max_uses или иные детали кода.

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Serialize;

/// Ответ валидации кода вовлечения. `reason` присутствует только при valid=false
/// и содержит обобщённую причину без утечки деталей.
#[derive(Serialize)]
pub struct EnrollValidationResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_name: Option<String>,
    pub onboarding_traffic_mb: i64,
}

/// Сколько трафика (МБ) человек получит, зарегистрировавшись по этому коду.
///
/// Раньше это была глобальная настройка `onboarding_traffic_mb`, начислявшаяся
/// в обход подписки. Теперь трафик приходит только от плана, поэтому число берётся
/// у самого бесплатного плана — того, на который регистрация и посадит человека.
/// Имя поля в ответе сохранено: это публичный контракт, а смысл его не изменился.
///
/// Бесплатный план не настроен — `0`, как и раньше при выключенной настройке.
/// Ошибка БД тоже даёт `0`: экран приветствия не повод ронять валидацию кода.
async fn read_onboarding_mb(state: &AppState) -> i64 {
    let gb: Option<i32> = sqlx::query_scalar(
        "SELECT COALESCE(traffic_limit_gb, 0) FROM plans \
         WHERE is_free = TRUE AND is_active = TRUE LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    i64::from(gb.unwrap_or(0)).max(0).saturating_mul(1024)
}

/// Имя панели для раннего брендинга на клиенте. Берём настройку brand_name; если
/// она пуста или не задана — отдаём дефолтный бренд "Caramba Connect" (rule 1:
/// user-facing default brand, НЕ exarobot).
async fn read_panel_name(state: &AppState) -> String {
    let name = state
        .settings
        .get_or_default("brand_name", "Caramba Connect")
        .await;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Caramba Connect".to_string()
    } else {
        trimmed.to_string()
    }
}

/// GET /api/v2/app/enroll/{code} — публичная валидация кода вовлечения.
///
/// Возвращает 200 в обоих случаях (valid true/false) — это не аутентификация, а
/// проверка пригодности кода для дальнейшего флоу register/login. Причины
/// обобщённые ("invalid" — без различения expired/exhausted/unknown), чтобы не
/// раскрывать состояние кодов перебором.
pub async fn validate_enroll_code(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let code = code.trim();

    // Базовая защита от мусора: пустой код не валиден без удара по БД.
    if code.is_empty() {
        let onboarding_traffic_mb = read_onboarding_mb(&state).await;
        let panel_name = read_panel_name(&state).await;
        return Json(EnrollValidationResponse {
            valid: false,
            reason: Some("invalid".to_string()),
            panel_name: Some(panel_name),
            onboarding_traffic_mb,
        })
        .into_response();
    }

    let onboarding_traffic_mb = read_onboarding_mb(&state).await;
    let panel_name = read_panel_name(&state).await;

    match state.store_service.validate_enrollment_code(code).await {
        Ok(Some(_)) => Json(EnrollValidationResponse {
            valid: true,
            reason: None,
            panel_name: Some(panel_name),
            onboarding_traffic_mb,
        })
        .into_response(),
        Ok(None) => Json(EnrollValidationResponse {
            valid: false,
            // Обобщённо: не различаем expired / exhausted / unknown во избежание
            // утечки состояния кода.
            reason: Some("invalid".to_string()),
            panel_name: Some(panel_name),
            onboarding_traffic_mb,
        })
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "enroll validation: db lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
