//! Витрина тарифов, сроков и способов оплаты для standalone-приложения.
//!
//! Три чтения, ни одной записи:
//!   * `GET /plans`            — каталог планов со сроками (то, что видит
//!     мини-апп, тем же `catalog_service`);
//!   * `GET /payment-methods`  — способы оплаты для выбранного срока/заказа;
//!   * `GET /purchase/{id}`    — статус ранее созданной платёжной сессии.
//!
//! Почему это отдельный модуль, а не продолжение `app_billing.rs`: там живёт
//! ЗАПИСЬ (создание чек-аута), она за лицензионным гейтом и её нельзя вызвать
//! на Free-тире. Витрина — чтение, и гейтить её нельзя: человек, которому
//! нельзя купить из приложения, всё равно должен видеть, что ему предлагают и
//! куда идти платить. Гейт остаётся ровно на `POST /purchase`, а сюда приезжает
//! флагом `in_app_purchase`, чтобы приложение сказало правду ДО нажатия, а не
//! получило 403 после.
//!
//! Каталог НЕ придумывает данные. План без строк в `plan_durations` — это
//! осмысленное решение оператора «не продаётся» (см. длинный комментарий в
//! `catalog_service::get_active_plans`), и он приезжает с `durations: []`.
//! Ровно так его рисует мини-апп: карточка без кнопки. Подставить сюда
//! выдуманный срок «30 дней по plans.price» значило бы вернуть то
//! самовосстановление, которое из панели уже убрали, только на другом конце.

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use caramba_db::models::store::Plan;
use serde::{Deserialize, Serialize};

/// Базовая валюта витрины. Тот же литерал, что возвращает
/// `catalog_service::resolve_duration_price` при отсутствии per-provider
/// override, и тот же, что подставляет `price_for` в мини-аппе. Второго
/// источника валюты в панели нет: `plan_durations` колонки валюты не имеет,
/// валюта появляется только в строках `plan_duration_provider_prices`.
const BASE_CURRENCY: &str = "USD";

/// Ссылки на оплату этой установки.
///
/// Читает те же две настройки и зовёт ту же
/// `subscription::access::pay_links`, что и `app.rs`. Формат ссылок собран в
/// одном месте (в `access::pay_links`), поэтому здесь дублируется только
/// чтение настроек — разъехаться нечему. Отдельная копия существует потому,
/// что хелпер в `app.rs` приватный, а `app.rs` в этой работе не редактируется.
async fn pay_links(state: &AppState) -> Option<crate::subscription::access::PayLinks> {
    let bot = state.settings.get_or_default("bot_username", "").await;
    let short = state
        .settings
        .get_or_default("mini_app_short_name", "")
        .await;
    crate::subscription::access::pay_links(&bot, &short)
}

// ============================================================
// PLANS — каталог
// ============================================================

#[derive(Debug, Serialize, PartialEq)]
struct DurationOut {
    /// `plan_durations.id` — именно он уезжает в `POST /purchase`.
    id: i64,
    duration_days: i32,
    /// Минорные единицы (центы). Основное поле: округления нет.
    price_cents: i64,
    /// Та же цена дробью — чтобы клиенту не делить самому.
    price: f64,
}

#[derive(Debug, Serialize, PartialEq)]
struct PlanOut {
    id: i64,
    name: String,
    description: Option<String>,
    /// 0 — трафик не ограничен.
    traffic_limit_gb: i32,
    device_limit: i32,
    is_free: bool,
    is_trial: bool,
    /// МБ суточного пополнения; 0 — суточной нормы нет.
    daily_traffic_mb: i32,
    /// Сколько живых узлов даёт план и в каких странах — выведено из
    /// инфраструктуры в `catalog_service`, не из текста описания.
    server_count: i64,
    countries: Vec<String>,
    /// Пусто = план сейчас не продаётся. Клиент рисует карточку без кнопки.
    durations: Vec<DurationOut>,
}

#[derive(Debug, Serialize)]
struct PlansResponse {
    currency: String,
    /// Работает ли `POST /purchase` на этой установке (лицензионный флаг
    /// `end_user_billing`). `false` — приложение обязано увести человека в
    /// Telegram, а не показывать кнопку, которая вернёт 403.
    in_app_purchase: bool,
    /// Куда идти платить, если в приложении нельзя. `null` — оператор не
    /// настроил бота; тогда честнее промолчать, чем нарисовать мёртвую ссылку.
    pay: Option<crate::subscription::access::PayLinks>,
    plans: Vec<PlanOut>,
}

fn plan_out(p: &Plan) -> PlanOut {
    PlanOut {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
        traffic_limit_gb: p.traffic_limit_gb,
        device_limit: p.device_limit,
        is_free: p.is_free,
        is_trial: p.is_trial.unwrap_or(false),
        daily_traffic_mb: p.daily_traffic_mb,
        server_count: p.server_count,
        countries: p.countries.clone(),
        durations: p
            .durations
            .iter()
            .map(|d| DurationOut {
                id: d.id,
                duration_days: d.duration_days,
                price_cents: d.price,
                price: d.price as f64 / 100.0,
            })
            .collect(),
    }
}

/// Порядок карточек на витрине: сначала то, что можно купить, потом платное без
/// цены, в самом конце бесплатное. Внутри группы — по id, чтобы порядок не
/// плясал между запросами (`get_active_plans` не сортирует).
///
/// Бесплатный план уезжает в конец не из вкусовщины: экран покупки открывают,
/// чтобы купить, а `is_free` кнопки покупки не получает никогда.
fn plan_sort_key(p: &PlanOut) -> (bool, bool, i64) {
    (p.durations.is_empty(), p.is_free, p.id)
}

fn build_plans_response(
    plans: &[Plan],
    in_app_purchase: bool,
    pay: Option<crate::subscription::access::PayLinks>,
) -> PlansResponse {
    let mut out: Vec<PlanOut> = plans.iter().map(plan_out).collect();
    out.sort_by_key(plan_sort_key);
    PlansResponse {
        currency: BASE_CURRENCY.to_string(),
        in_app_purchase,
        pay,
        plans: out,
    }
}

/// GET /api/v2/app/plans — каталог тарифов и покупаемых сроков.
///
/// За `require_app_jwt`, но БЕЗ лицензионного гейта (см. doc-comment модуля).
pub async fn get_plans(
    State(state): State<AppState>,
    axum::Extension(_auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let plans = match state.catalog_service.get_active_plans().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(err = %e, "app: failed to fetch plans");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch plans").into_response();
        }
    };

    let limits = crate::license::effective_limits(&state).await;
    let pay = pay_links(&state).await;

    Json(build_plans_response(&plans, limits.end_user_billing, pay)).into_response()
}

// ============================================================
// PAYMENT METHODS — способы оплаты выбранного срока
// ============================================================

/// Где происходит оплата.
///
/// Различие не косметическое: `in_app` уходит в `POST /purchase` и получает
/// `pay_url`, а `telegram` в приложении не оплачивается вовсе — Stars требует
/// bot_token и tg_id, которых у приложения нет, и `MarketplaceService` его
/// чек-аут не создаёт (см. doc-comment `app_billing.rs`). Без этого поля
/// клиент отправлял бы Stars в `/purchase` и получал ошибку провайдера.
const CHECKOUT_IN_APP: &str = "in_app";
const CHECKOUT_TELEGRAM: &str = "telegram";

fn checkout_kind(provider: &str) -> &'static str {
    match provider {
        "stars" => CHECKOUT_TELEGRAM,
        _ => CHECKOUT_IN_APP,
    }
}

/// Можно ли этим способом реально заплатить прямо сейчас.
///
/// Telegram-путь лицензии не требует: он идёт через бота, мимо `POST /purchase`.
/// Всё остальное упирается в `end_user_billing`, включая оплату с баланса —
/// её тоже проводит `/purchase`.
fn method_available(checkout: &str, in_app_purchase: bool) -> bool {
    checkout == CHECKOUT_TELEGRAM || in_app_purchase
}

#[derive(Debug, Deserialize)]
pub struct MethodsQuery {
    pub duration_id: Option<i64>,
    pub order_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PaymentMethodOut {
    /// Имя провайдера — то самое, что уходит в `POST /purchase` полем `provider`.
    id: String,
    /// Запасная подпись (та же, что рисует мини-апп). Приложение вправе
    /// показывать свою: `id` — контракт, `label` — вежливость.
    label: String,
    /// Эффективная сумма с учётом per-provider override. `null` — цель запроса
    /// не задана (общий список способов).
    amount: Option<i64>,
    amount_decimal: Option<f64>,
    currency: Option<String>,
    /// `in_app` | `telegram`.
    checkout: &'static str,
    /// Куда вести, если `checkout == "telegram"`. Native-форму приложение
    /// берёт из `pay.miniapp_native` конверта и пробует первой.
    url: Option<String>,
    /// `false` — способ существует, но сейчас не сработает.
    available: bool,
    /// Машиночитаемая причина недоступности; `null`, когда доступен.
    unavailable_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct PaymentMethodsResponse {
    currency: String,
    in_app_purchase: bool,
    pay: Option<crate::subscription::access::PayLinks>,
    methods: Vec<PaymentMethodOut>,
}

/// GET /api/v2/app/payment-methods?duration_id=X|order_id=Y
///
/// Порт логики мини-аппа (`api/client.rs::get_payment_providers`) на JWT
/// приложения: тот же реестр `provider_names()`, тот же per-provider тумблер
/// `provider_enable_setting`, те же цены через `resolve_duration_price` /
/// `list_duration_overrides`. Отличий ровно три и все — из-за приложения:
/// баланс ищется по внутреннему `user_id` (tg_id у приложения может не быть),
/// заказ проверяется на принадлежность (иначе чужая сумма утекала бы по
/// перебору id), и каждый способ несёт `checkout`.
pub async fn get_payment_methods(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Query(q): Query<MethodsQuery>,
) -> impl IntoResponse {
    // Базовая цель запроса. Несуществующий срок — это 404, а не список способов
    // с пустыми ценами: пустая цена в листе оплаты выглядит как «бесплатно».
    let base_amount: Option<i64> = if let Some(did) = q.duration_id {
        let found: Option<i64> =
            sqlx::query_scalar("SELECT price FROM plan_durations WHERE id = $1")
                .bind(did)
                .fetch_optional(&state.pool)
                .await
                .unwrap_or(None);
        match found {
            Some(v) => Some(v),
            None => return (StatusCode::NOT_FOUND, "Unknown duration ID").into_response(),
        }
    } else if let Some(oid) = q.order_id {
        // Принадлежность заказа проверяем ДО цены — чужую сумму показывать нельзя.
        let owned: Option<i64> =
            sqlx::query_scalar("SELECT total_amount FROM orders WHERE id = $1 AND user_id = $2")
                .bind(oid)
                .bind(auth.user_id)
                .fetch_optional(&state.pool)
                .await
                .unwrap_or(None);
        match owned {
            Some(v) => Some(v),
            None => return (StatusCode::NOT_FOUND, "Unknown order ID").into_response(),
        }
    } else {
        None
    };

    let overrides: std::collections::HashMap<String, (i64, String)> = match q.duration_id {
        Some(did) => state.catalog_service.list_duration_overrides(did).await,
        None => std::collections::HashMap::new(),
    };

    let limits = crate::license::effective_limits(&state).await;
    let in_app_purchase = limits.end_user_billing;
    let pay = pay_links(&state).await;
    // Для Telegram-способа отдаём https-форму: nativeform (`tg://`) клиент берёт
    // из конверта сам и пробует первой, но положить её сюда нельзя — способ
    // должен открываться и там, где Telegram не установлен.
    let tg_url = pay
        .as_ref()
        .map(|p| p.miniapp_url.clone().unwrap_or_else(|| p.bot_url.clone()));

    let price_for = |name: &str| -> (Option<i64>, Option<String>) {
        if let Some((a, c)) = overrides.get(name) {
            (Some(*a), Some(c.clone()))
        } else if let Some(b) = base_amount {
            (Some(b), Some(BASE_CURRENCY.to_string()))
        } else {
            (None, None)
        }
    };

    let mut methods: Vec<PaymentMethodOut> = Vec::new();

    // Баланс предлагаем только когда он положительный: пункт «оплатить с
    // баланса $0.00» — это кнопка, ведущая в «Insufficient balance».
    let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if balance > 0 {
        let checkout = checkout_kind("balance");
        let available = method_available(checkout, in_app_purchase);
        methods.push(PaymentMethodOut {
            id: "balance".to_string(),
            label: format!("Balance (${:.2})", balance as f64 / 100.0),
            amount: base_amount,
            amount_decimal: base_amount.map(|a| a as f64 / 100.0),
            currency: base_amount.map(|_| BASE_CURRENCY.to_string()),
            checkout,
            url: None,
            available,
            unavailable_reason: (!available).then_some("in_app_purchase_disabled"),
        });
    }

    for name in state.marketplace_service.provider_names() {
        // Баланс уже добавлен выше со своей динамической подписью.
        if name == "balance" {
            continue;
        }
        let (enable_key, default_on) =
            crate::services::marketplace_service::provider_enable_setting(&name);
        let default = if default_on { "true" } else { "false" };
        if state.settings.get_or_default(&enable_key, default).await != "true" {
            continue;
        }

        let (amount, currency) = if let Some(oid) = q.order_id {
            match state.catalog_service.resolve_order_price(oid, &name).await {
                Ok(Some((a, c))) => (Some(a), Some(c)),
                _ => (base_amount, base_amount.map(|_| BASE_CURRENCY.to_string())),
            }
        } else {
            price_for(&name)
        };

        let checkout = checkout_kind(&name);
        let available = method_available(checkout, in_app_purchase);
        methods.push(PaymentMethodOut {
            id: name.clone(),
            label: crate::api::client::provider_label(&name),
            amount,
            amount_decimal: amount.map(|a| a as f64 / 100.0),
            currency,
            checkout,
            url: if checkout == CHECKOUT_TELEGRAM {
                tg_url.clone()
            } else {
                None
            },
            available,
            unavailable_reason: (!available).then_some("in_app_purchase_disabled"),
        });
    }

    Json(PaymentMethodsResponse {
        currency: BASE_CURRENCY.to_string(),
        in_app_purchase,
        pay,
        methods,
    })
    .into_response()
}

// ============================================================
// PURCHASE STATUS — чем кончился чек-аут
// ============================================================

#[derive(Debug, Serialize)]
struct PurchaseStatusResponse {
    session_id: String,
    /// `pending` | `completed` | `failed` | `expired` — как их пишет
    /// `payment_session_repo`. Клиент ОБЯЗАН пережить незнакомое значение.
    status: String,
    provider: String,
    amount: i64,
    amount_decimal: f64,
    currency: String,
    /// Оплачено и выдано. Единственное значение, по которому приложение вправе
    /// сказать «готово» — остальные статусы означают «ещё нет» или «не будет».
    paid: bool,
    created_at: String,
    updated_at: String,
}

/// GET /api/v2/app/purchase/{session_id} — статус платёжной сессии.
///
/// Нужен ровно для одного момента: человек ушёл платить во внешний браузер и
/// вернулся в приложение. Без этого запроса приложению остаётся угадывать по
/// подписке, а деньги и продление разъезжаются во времени (вебхук провайдера
/// приходит своим темпом).
///
/// Чужая сессия отвечает 404, а не 403: существование чужого id — тоже факт,
/// и подтверждать его перебором незачем.
pub async fn get_purchase_status(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(session_id.trim()) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, "Malformed session ID").into_response(),
    };

    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            i64,
            String,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        ),
    >(
        "SELECT status, provider, amount, currency, created_at, updated_at \
         FROM payment_sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(uuid)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await;

    let row = match row {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "app: purchase status lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Status lookup failed").into_response();
        }
    };

    let (status, provider, amount, currency, created_at, updated_at) = row;
    Json(PurchaseStatusResponse {
        session_id: uuid.to_string(),
        paid: status == "completed",
        status,
        provider,
        amount,
        amount_decimal: amount as f64 / 100.0,
        currency,
        created_at: created_at.to_rfc3339(),
        updated_at: updated_at.to_rfc3339(),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use caramba_db::models::store::PlanDuration;
    use chrono::{TimeZone, Utc};

    fn duration(id: i64, plan_id: i64, days: i32, price: i64) -> PlanDuration {
        PlanDuration {
            id,
            plan_id,
            duration_days: days,
            price,
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
        }
    }

    fn plan(id: i64, name: &str, is_free: bool, durations: Vec<PlanDuration>) -> Plan {
        Plan {
            id,
            name: name.to_string(),
            description: None,
            is_active: true,
            traffic_limit_gb: 0,
            device_limit: 1,
            is_trial: Some(false),
            daily_traffic_mb: 0,
            is_free,
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
            durations,
            server_count: 0,
            countries: vec![],
        }
    }

    /// Слепок каталога живой установки на 2026-09-05: Gold с двумя сроками
    /// (180 дней за $15, 360 за $25), Starter без единой строки в
    /// `plan_durations`, бесплатный Free.
    fn live_catalog() -> Vec<Plan> {
        vec![
            plan(
                1,
                "Gold",
                false,
                vec![duration(7, 1, 180, 1500), duration(8, 1, 360, 2500)],
            ),
            plan(2, "Starter", false, vec![]),
            plan(3, "Free", true, vec![]),
        ]
    }

    /// Главная защита этого модуля. У Starter нет ни одной строки цены, и это
    /// решение оператора, а не пробел в данных. Стоит кому-нибудь «починить»
    /// витрину, подставив срок из `plans.price`, — и приложение начнёт продавать
    /// тариф по цене, которой оператор не назначал. Такое уже вырезали из
    /// `catalog_service`; тест держит дверь закрытой с этой стороны.
    #[test]
    fn a_plan_without_durations_ships_empty_not_invented() {
        let r = build_plans_response(&live_catalog(), false, None);
        let starter = r.plans.iter().find(|p| p.name == "Starter").unwrap();
        assert!(
            starter.durations.is_empty(),
            "Starter получил выдуманный срок: {:?}",
            starter.durations
        );
        let free = r.plans.iter().find(|p| p.name == "Free").unwrap();
        assert!(free.durations.is_empty(), "бесплатному плану дали цену");
    }

    /// Цена уезжает в центах ровно той, что лежит в базе: ни округления, ни
    /// пересчёта в «удобные» единицы. `price` — производная, а не второй источник.
    #[test]
    fn duration_prices_travel_in_minor_units() {
        let r = build_plans_response(&live_catalog(), true, None);
        let gold = r.plans.iter().find(|p| p.name == "Gold").unwrap();
        assert_eq!(
            gold.durations,
            vec![
                DurationOut {
                    id: 7,
                    duration_days: 180,
                    price_cents: 1500,
                    price: 15.0
                },
                DurationOut {
                    id: 8,
                    duration_days: 360,
                    price_cents: 2500,
                    price: 25.0
                },
            ]
        );
        assert_eq!(r.currency, "USD");
    }

    /// Покупаемое — первым, бесплатное — последним. Иначе экран покупки
    /// открывается на карточке, которую купить нельзя.
    #[test]
    fn purchasable_plans_come_first_and_free_last() {
        let r = build_plans_response(&live_catalog(), true, None);
        let order: Vec<&str> = r.plans.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, vec!["Gold", "Starter", "Free"]);
    }

    /// Витрина не гейтится лицензией — она отдаёт `in_app_purchase: false` и
    /// список планов целиком. Пустой каталог на Free-тире означал бы, что
    /// человеку не показывают даже того, что ему продают в Telegram.
    #[test]
    fn the_catalog_survives_a_disabled_in_app_billing() {
        let r = build_plans_response(&live_catalog(), false, None);
        assert!(!r.in_app_purchase);
        assert_eq!(r.plans.len(), 3);
    }

    /// Stars оплачивается только в Telegram: `MarketplaceService` его чек-аут
    /// не создаёт. Классифицируй его как `in_app` — и клиент отправит покупку в
    /// `POST /purchase`, где она умрёт ошибкой провайдера.
    #[test]
    fn stars_is_a_telegram_checkout_everything_else_is_in_app() {
        assert_eq!(checkout_kind("stars"), CHECKOUT_TELEGRAM);
        for name in ["balance", "stripe", "cryptobot", "manual", "nowpayments"] {
            assert_eq!(checkout_kind(name), CHECKOUT_IN_APP, "провайдер {name}");
        }
    }

    /// Telegram-путь идёт мимо `POST /purchase`, поэтому лицензия его не
    /// закрывает. Всё остальное — включая оплату с баланса — закрывает.
    #[test]
    fn the_license_gate_closes_in_app_methods_only() {
        assert!(method_available(CHECKOUT_TELEGRAM, false));
        assert!(!method_available(CHECKOUT_IN_APP, false));
        assert!(method_available(CHECKOUT_IN_APP, true));
    }

    /// Каталог тарифов — не публичные данные: он рассказывает, что у оператора
    /// продаётся и почём, любому, кто знает адрес панели. Роутер собирается из
    /// двух половин, и попасть в `public` вместо `protected` — правка на один
    /// отступ. Тест читает исходник роутера и держит все три чтения за JWT.
    #[test]
    fn the_storefront_routes_live_behind_the_jwt_layer() {
        let router = include_str!("mod.rs");
        let protected_at = router
            .find("let protected = axum::Router::new()")
            .expect("роутер приложения переименовали — проверь, где теперь JWT");
        let jwt_at = router
            .find("app_auth::require_app_jwt")
            .expect("слой JWT исчез из роутера приложения");

        // Ищем ПОСЛЕ начала защищённой половины: путь `/plans` есть и в роутере
        // бота, и наивный поиск с начала файла нашёл бы чужой маршрут.
        // Путь и хендлер проверяются порознь: rustfmt разносит длинный вызов
        // `.route()` на несколько строк, и склеенная подстрока «путь, хендлер»
        // ломалась бы от переформатирования, а не от ошибки.
        let tail = &router[protected_at..];
        for (path, handler) in [
            ("\"/plans\"", "app_plans::get_plans"),
            ("\"/payment-methods\"", "app_plans::get_payment_methods"),
            (
                "\"/purchase/{session_id}\"",
                "app_plans::get_purchase_status",
            ),
        ] {
            for needle in [path, handler] {
                let at = protected_at
                    + tail.find(needle).unwrap_or_else(|| {
                        panic!("не зарегистрировано в защищённой половине: {needle}")
                    });
                assert!(
                    at < jwt_at,
                    "{needle} оказалось вне защищённой половины роутера"
                );
            }
        }

        // Статус сессии не должен перебить создание чек-аута: это разные методы
        // на разных путях, и оба обязаны остаться на месте.
        assert!(
            router.contains("\"/purchase\", post(app_billing::purchase)"),
            "POST /purchase пропал — статус сессии съел создание чек-аута"
        );
    }

    /// Каталог не ездит в ссылке `caramba://`.
    ///
    /// Ссылка — одноразовое offline-приглашение: её печатают в QR, пересылают в
    /// мессенджере и открывают когда угодно, а цены и сроки оператор меняет в
    /// админке в любой момент. Каталог внутри ссылки протух бы молча — человек
    /// увидел бы старую цену и был бы прав, что увидел её от нас.
    ///
    /// Работает это иначе и уже работает: `redeem_connect_code` отдаёт
    /// JWT-сессию, и экран подтверждения первым делом спрашивает `/plans` этой
    /// сессией — свежий каталог за один лишний запрос.
    ///
    /// Тест смотрит в исходник соседнего модуля, потому что защищает решение, а
    /// не код: `RedeemResponse` обязан нести сессию и не обязан нести витрину.
    #[test]
    fn the_connect_link_carries_a_session_not_a_catalog() {
        let enroll = include_str!("app_enroll.rs");
        let at = enroll
            .find("pub struct RedeemResponse {")
            .expect("ответ погашения переименовали");
        let body = &enroll[at..at + enroll[at..].find("\n}\n").expect("ответ не закрыт")];

        assert!(
            body.contains("pub session: TokenPair"),
            "из ответа погашения пропала сессия — приложению нечем спросить /plans"
        );
        for smell in ["plans", "durations", "price", "currency"] {
            assert!(
                !body.contains(smell),
                "в одноразовую ссылку заехала витрина (поле с «{smell}») — она протухнет молча"
            );
        }
    }

    /// Приложение отличает `null` («бот не настроен») от ссылки. Проверяем, что
    /// конверт переносит оба состояния, а не подставляет пустую строку.
    #[test]
    fn pay_links_are_absent_rather_than_empty() {
        let none = build_plans_response(&[], false, None);
        let json = serde_json::to_value(&none).unwrap();
        assert!(json["pay"].is_null(), "пустой бот приехал не как null");

        let links = crate::subscription::access::pay_links("exa_robot", "exaconnect")
            .expect("бот настроен — ссылки обязаны быть");
        let some = build_plans_response(&[], true, Some(links));
        let json = serde_json::to_value(&some).unwrap();
        assert_eq!(json["pay"]["bot_url"], "https://t.me/exa_robot");
        assert!(
            json["pay"]["miniapp_native"]
                .as_str()
                .unwrap()
                .starts_with("tg://resolve?domain=exa_robot"),
            "нативная ссылка потерялась: {}",
            json["pay"]["miniapp_native"]
        );
    }
}
