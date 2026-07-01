//! JWT-защищённые эндпоинты партнёрского кабинета (/api/v2/app/partner/*).
//!
//! Партнёр — пользователь с флагом users.is_partner = TRUE. Он управляет
//! несколькими per-source реферальными кодами (partner_codes): по одному на
//! источник трафика. Каждый код привязывает нового пользователя к партнёру
//! через существующий механизм атрибуции (users.referrer_id +
//! users.signup_partner_code_id). Деньги здесь НЕ дублируются: статистика
//! (signups / conversions / balance_earned) выводится из referral_rewards,
//! которые пишет обычный реферальный путь.
//!
//! Гейтинг роли:
//!   * GET /codes у не-партнёра отдаёт { is_partner: false, codes: [] } (200),
//!     чтобы клиент мог тихо показать «вы не партнёр».
//!   * POST/DELETE у не-партнёра отдают 403 — мутации доступны только партнёрам.
//!
//! Стиль повторяет app_account.rs: AuthUser из extensions, локальные DTO с
//! Serialize, делегирование в PromoService.

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use crate::services::promo_service::PartnerCodeStats;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

/// Одна запись партнёрского кода со сводной статистикой.
/// Имена полей — часть контракта (Flutter их читает).
#[derive(Serialize)]
struct PartnerCodeEntry {
    code: String,
    source_label: String,
    /// RFC3339.
    created_at: String,
    clicks: i64,
    signups: i64,
    conversions: i64,
    /// Заработано партнёром с этого кода за всё время, минорные единицы (центы).
    balance_earned: i64,
}

impl From<PartnerCodeStats> for PartnerCodeEntry {
    fn from(s: PartnerCodeStats) -> Self {
        PartnerCodeEntry {
            code: s.code,
            source_label: s.source_label,
            created_at: s.created_at.to_rfc3339(),
            clicks: s.clicks,
            signups: s.signups,
            conversions: s.conversions,
            balance_earned: s.balance_earned,
        }
    }
}

/// Ответ GET /app/partner/codes.
#[derive(Serialize)]
struct PartnerCodesResponse {
    is_partner: bool,
    codes: Vec<PartnerCodeEntry>,
}

/// Тело POST /app/partner/codes.
#[derive(Deserialize)]
pub struct CreatePartnerCodeReq {
    #[serde(default)]
    pub source_label: String,
}

/// GET /api/v2/app/partner/codes — список партнёрских кодов + статистика.
///
/// Не-партнёр получает { is_partner: false, codes: [] } со статусом 200.
pub async fn list_codes(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let is_partner = state
        .promo_service
        .is_partner(auth.user_id)
        .await
        .unwrap_or(false);

    if !is_partner {
        return Json(PartnerCodesResponse {
            is_partner: false,
            codes: Vec::new(),
        })
        .into_response();
    }

    match state.promo_service.list_partner_codes(auth.user_id).await {
        Ok(rows) => Json(PartnerCodesResponse {
            is_partner: true,
            codes: rows.into_iter().map(PartnerCodeEntry::from).collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, user_id = auth.user_id, "partner list_codes failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// POST /api/v2/app/partner/codes — создать партнёрский код. Возвращает
/// созданный объект кода (со статистикой, изначально нулевой).
///
/// Не-партнёр получает 403.
pub async fn create_code(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Json(req): Json<CreatePartnerCodeReq>,
) -> impl IntoResponse {
    let is_partner = state
        .promo_service
        .is_partner(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_partner {
        return (StatusCode::FORBIDDEN, "Not a partner").into_response();
    }

    let code = match state
        .promo_service
        .create_partner_code(auth.user_id, &req.source_label)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(err = %e, user_id = auth.user_id, "partner create_code failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Возвращаем сразу со статистикой (нулевой) — единый shape с list.
    match state
        .promo_service
        .get_partner_code(auth.user_id, &code)
        .await
    {
        Ok(Some(stats)) => {
            (StatusCode::CREATED, Json(PartnerCodeEntry::from(stats))).into_response()
        }
        Ok(None) => {
            // Код только что создан — отсутствие строки означает гонку удаления;
            // отдаём минимальный объект, чтобы клиент не падал.
            tracing::warn!(user_id = auth.user_id, code = %code, "partner code vanished after create");
            (
                StatusCode::CREATED,
                Json(PartnerCodeEntry {
                    code,
                    source_label: req.source_label.trim().to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    clicks: 0,
                    signups: 0,
                    conversions: 0,
                    balance_earned: 0,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, user_id = auth.user_id, "partner get_code after create failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE /api/v2/app/partner/codes/{code} — удалить собственный код.
///
/// Не-партнёр получает 403; чужой/несуществующий код — 404.
pub async fn delete_code(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let is_partner = state
        .promo_service
        .is_partner(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_partner {
        return (StatusCode::FORBIDDEN, "Not a partner").into_response();
    }

    match state
        .promo_service
        .delete_partner_code(auth.user_id, &code)
        .await
    {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Code not found").into_response(),
        Err(e) => {
            tracing::error!(err = %e, user_id = auth.user_id, "partner delete_code failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
