//! JWT-защищённые биллинг-эндпоинты standalone-приложения.
//!
//! Дополняют `app.rs`/`app_account.rs` двумя разделами, которые рисует Flutter:
//!   * история трафика для графика (`GET /traffic`) — подневные точки up/down;
//!   * создание чек-аута покупки (`POST /purchase`) — обёртка над тем же
//!     `marketplace_service.create_session`, что и mini-app, без переизобретения
//!     платежей. Возвращает `pay_url` — provider-specific строку: абсолютный URL
//!     оплаты, относительный путь панели (provider `manual` → `/manual-upload`)
//!     или сентинел `SUCCESS` для оплаты с баланса. Telegram Stars сюда НЕ
//!     попадает: `StarsProvider` исключён из `MarketplaceService` (нужны
//!     bot_token/tg_id), Stars идёт через `PayService::create_stars_invoice` из
//!     бота. Поле `pay_url_kind` классифицирует строку, чтобы клиент не пытался
//!     «запустить» не-URL во внешнем браузере.
//!
//! Стиль повторяет соседей: сырой sqlx/сервисы из AppState, локальные DTO с
//! `Serialize`, `AuthUser` из extensions (положен `app_auth::require_app_jwt`).

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================
// TRAFFIC — подневная история для fl_chart
// ============================================================

#[derive(Serialize)]
struct TrafficPoint {
    /// День в формате YYYY-MM-DD (UTC).
    date: String,
    up_bytes: i64,
    down_bytes: i64,
    /// Сумма за день (up + down) — клиент может рисовать одну линию.
    total_bytes: i64,
}

#[derive(Serialize)]
struct TrafficHistory {
    points: Vec<TrafficPoint>,
    total_up: i64,
    total_down: i64,
    /// Совокупный накопительный счётчик подписки (subscriptions.used_traffic).
    /// Подневная история начала писаться недавно, поэтому сумма по `points`
    /// может быть меньше — это значение даёт «честный» итог за всё время.
    cumulative_used_bytes: i64,
    /// Источник up/down. Узел рапортует один счётчик байт на пользователя без
    /// разделения направлений, поэтому весь объём пока в down_bytes, up = 0.
    /// Клиент может показать предупреждение/легенду по этому полю.
    direction_split: &'static str,
}

/// GET /api/v2/app/traffic — подневная история трафика (~30 дней) для графика.
///
/// Источник — таблица `app_traffic_daily` (пишется в точке приёма heartbeat'а
/// узла, см. api/v2/node.rs). Если истории ещё нет, отдаём пустой `points` плюс
/// `cumulative_used_bytes` из активной подписки как лучший доступный агрегат.
pub async fn get_traffic(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let repo = caramba_db::repositories::traffic_repo::TrafficRepository::new(state.pool.clone());

    let history = repo.get_history(auth.user_id, 30).await.unwrap_or_default();

    let mut total_up: i64 = 0;
    let mut total_down: i64 = 0;
    let points: Vec<TrafficPoint> = history
        .into_iter()
        .map(|p| {
            total_up += p.up_bytes;
            total_down += p.down_bytes;
            TrafficPoint {
                date: p.day.format("%Y-%m-%d").to_string(),
                up_bytes: p.up_bytes,
                down_bytes: p.down_bytes,
                total_bytes: p.up_bytes + p.down_bytes,
            }
        })
        .collect();

    // Накопительный счётчик активной подписки — лучший агрегат «за всё время».
    let cumulative_used_bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(used_traffic), 0)::bigint FROM subscriptions \
         WHERE user_id = $1 AND status = 'active'",
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    Json(TrafficHistory {
        points,
        total_up,
        total_down,
        cumulative_used_bytes,
        direction_split: "down_only",
    })
    .into_response()
}

// ============================================================
// PURCHASE — обёртка над marketplace_service.create_session
// ============================================================

#[derive(Deserialize)]
pub struct PurchaseRequest {
    /// Длительность плана (plan_durations.id) — основной путь покупки подписки.
    pub duration_id: Option<i64>,
    /// Альтернатива: id заказа магазина (orders.id) для оплаты корзины/товара.
    pub order_id: Option<i64>,
    /// Платёжный провайдер. Если не задан — берётся первый доступный (или balance).
    /// Имя должно совпадать с зарегистрированным в marketplace_service.
    pub provider: Option<String>,
}

#[derive(Serialize)]
struct PurchaseResponse {
    /// Provider-specific строка чек-аута. В зависимости от провайдера это:
    ///   * абсолютный URL оплаты (большинство крипто/карточных провайдеров) —
    ///     клиент открывает во внешнем браузере;
    ///   * относительный путь панели (`manual` → `/manual-upload`) — клиент
    ///     должен достроить до абсолютного через базовый URL панели;
    ///   * сентинел `SUCCESS` при оплате с баланса (`fulfilled=true`).
    /// Telegram Stars здесь не появляется (см. doc-comment модуля). Используй
    /// `pay_url_kind` для надёжной классификации вместо парсинга строки.
    pay_url: String,
    /// Тип значения `pay_url`, чтобы клиент не запускал не-URL во внешнем
    /// браузере: `absolute_url` | `relative_path` | `balance_success`.
    pay_url_kind: &'static str,
    /// UUID платёжной сессии (payment_sessions.id) — для трекинга/опроса статуса.
    session_id: String,
    /// Сумма в минорных единицах (центы/копейки) и человекочитаемая дробная.
    amount: i64,
    amount_decimal: f64,
    currency: String,
    provider: String,
    /// true только для мгновенной оплаты с баланса (подписка уже активирована).
    fulfilled: bool,
}

/// POST /api/v2/app/purchase — создаёт чек-аут покупки плана или заказа.
///
/// НЕ переизобретает платежи: целиком переиспользует
/// `marketplace_service.create_session` (тот же путь, что и mini-app в
/// api/client.rs::create_payment_invoice). Цена/валюта резолвятся через
/// catalog_service с учётом per-provider override. Для balance — мгновенное
/// списание+fulfillment, как в mini-app.
pub async fn purchase(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Json(body): Json<PurchaseRequest>,
) -> impl IntoResponse {
    // License gate (P4, contract E): end-user billing is gated on the tier flag.
    // Free / soft-degraded instances cannot open purchase checkouts. get_traffic
    // (read-only) is intentionally NOT gated.
    let limits = crate::license::effective_limits(&state).await;
    if let Err(e) = crate::license::check_billing_enabled(&limits) {
        return (StatusCode::FORBIDDEN, e.to_string()).into_response();
    }

    // Резолвим User по внутреннему id (JWT несёт user_id, не tg_id).
    let user = match state.store_service.get_user_by_id(auth.user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::NOT_FOUND, "User not found").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: purchase user lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "User lookup failed").into_response();
        }
    };

    // Провайдер: явный из тела, иначе первый доступный (balance как fallback).
    let provider = match body.provider.clone() {
        Some(p) if !p.trim().is_empty() => p,
        _ => state
            .marketplace_service
            .provider_names()
            .into_iter()
            .find(|n| n != "balance")
            .unwrap_or_else(|| "balance".to_string()),
    };
    if state.marketplace_service.get_provider(&provider).is_none() {
        return (StatusCode::BAD_REQUEST, "Unknown or disabled payment provider").into_response();
    }

    // Резолвим (product_id, duration_days?, amount, currency) + metadata — той же
    // логикой, что и mini-app. product_id для плана = plan_id; для заказа = order_id.
    let (product_id, duration_days, amount, currency): (i64, Option<i32>, i64, String) =
        if let Some(duration_id) = body.duration_id {
            match state
                .catalog_service
                .resolve_duration_price(duration_id, &provider)
                .await
            {
                Ok(Some((plan_id, days, amt, cur))) => (plan_id, Some(days), amt, cur),
                Ok(None) => return (StatusCode::BAD_REQUEST, "Invalid duration ID").into_response(),
                Err(e) => {
                    tracing::error!(err = %e, "app: resolve_duration_price failed");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Price resolution failed")
                        .into_response();
                }
            }
        } else if let Some(order_id) = body.order_id {
            // Заказ должен принадлежать пользователю.
            let owns: Option<i64> =
                sqlx::query_scalar("SELECT id FROM orders WHERE id = $1 AND user_id = $2")
                    .bind(order_id)
                    .bind(user.id)
                    .fetch_optional(&state.pool)
                    .await
                    .unwrap_or(None);
            if owns.is_none() {
                return (StatusCode::BAD_REQUEST, "Invalid order ID").into_response();
            }
            match state
                .catalog_service
                .resolve_order_price(order_id, &provider)
                .await
            {
                Ok(Some((amt, cur))) => (order_id, None, amt, cur),
                Ok(None) => return (StatusCode::BAD_REQUEST, "Invalid order ID").into_response(),
                Err(e) => {
                    tracing::error!(err = %e, "app: resolve_order_price failed");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Price resolution failed")
                        .into_response();
                }
            }
        } else {
            return (StatusCode::BAD_REQUEST, "Missing duration_id or order_id").into_response();
        };

    // Metadata определяет тип ресурса для fulfillment'а (plan/order) и срок плана.
    let mut metadata = HashMap::new();
    if body.duration_id.is_some() {
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("plan".to_string()),
        );
        if let Some(days) = duration_days {
            metadata.insert("duration_days".to_string(), serde_json::Value::from(days));
        }
    } else {
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("order".to_string()),
        );
    }

    match state
        .marketplace_service
        .create_session(
            &user,
            product_id,
            &provider,
            amount,
            &currency,
            Some(serde_json::to_value(metadata).unwrap_or_default()),
        )
        .await
    {
        Ok((session, invoice_payload)) => {
            // Оплата с баланса: списываем атомарно и сразу fulfill'им (как mini-app).
            // Списываем session.amount, а не исходный amount: create_session мог
            // применить реферальную скидку первой покупки, и кошелёк, запись сессии
            // и база для реферального вознаграждения должны сходиться на этой сумме.
            if provider == "balance" {
                let charged = sqlx::query(
                    "UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
                )
                .bind(session.amount)
                .bind(user.id)
                .execute(&state.pool)
                .await;

                match charged {
                    Ok(res) if res.rows_affected() == 1 => {}
                    Ok(_) => {
                        let _ = state
                            .marketplace_service
                            .mark_session_failed(session.id)
                            .await;
                        return (StatusCode::BAD_REQUEST, "Insufficient balance").into_response();
                    }
                    Err(e) => {
                        tracing::error!(err = %e, "app: balance charge failed");
                        let _ = state
                            .marketplace_service
                            .mark_session_failed(session.id)
                            .await;
                        return (StatusCode::INTERNAL_SERVER_ERROR, "Balance charge failed")
                            .into_response();
                    }
                }

                if let Err(e) = state.marketplace_service.fulfill_payment(session.id).await {
                    tracing::error!(err = %e, "app: balance fulfillment failed");
                    // Возврат списания, чтобы не списать «впустую».
                    let _ = sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                        .bind(session.amount)
                        .bind(user.id)
                        .execute(&state.pool)
                        .await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Fulfillment failed",
                    )
                        .into_response();
                }

                return Json(PurchaseResponse {
                    pay_url: "SUCCESS".to_string(),
                    pay_url_kind: "balance_success",
                    session_id: session.id.to_string(),
                    // Возвращаем реально списанную (со скидкой) сумму.
                    amount: session.amount,
                    amount_decimal: session.amount as f64 / 100.0,
                    currency,
                    provider,
                    fulfilled: true,
                })
                .into_response();
            }

            // Провайдеры вроде `manual` отдают относительный путь панели
            // (`/manual-upload`); остальные — абсолютный URL оплаты. Клиент
            // должен достраивать относительный путь до абсолютного сам.
            let pay_url_kind = if invoice_payload.starts_with('/') {
                "relative_path"
            } else {
                "absolute_url"
            };

            Json(PurchaseResponse {
                pay_url: invoice_payload,
                pay_url_kind,
                session_id: session.id.to_string(),
                // Сумма из сессии: учитывает возможную реферальную скидку, на
                // которую построен invoice провайдера.
                amount: session.amount,
                amount_decimal: session.amount as f64 / 100.0,
                currency,
                provider,
                fulfilled: false,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, "app: create checkout session failed");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)).into_response()
        }
    }
}
