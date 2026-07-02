use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::AppState;

// ============================================================================
// Лимиты для отдельных «дорогих» эндпоинтов бота
// ============================================================================
//
// Бот работает на одном процессе и всегда обращается с одного IP (127.0.0.1
// при collocated-деплое или внутреннего адреса при distributed-деплое).
// Поэтому ключ — это путь запроса, а не IP. Глобальный лимит — отдельный
// Redis-ключ "rate:bot_global".
//
// Лимиты (fixed-window):
//   Глобальный        50 req / 3 сек  (burst 50, ~16 req/sec steady)
//   /users/create-free-subscription   5 req / 5 сек
//   /admin/gift                       5 req / 5 сек
//   /users/*/purchase-plan            20 req / 4 сек
//   /admin/ban  + /admin/unban        5 req / 5 сек

struct EndpointLimit {
    /// Суффикс, который ищем в пути запроса (достаточно для уникальной идентификации)
    path_suffix: &'static str,
    /// Максимальное кол-во запросов в окне
    limit: usize,
    /// Ширина окна в секундах
    window_secs: usize,
    /// Имя ключа в Redis (без префикса "rate:")
    key_name: &'static str,
}

/// Все endpoint-специфичные лимиты. Проверяются до глобального лимита.
const ENDPOINT_LIMITS: &[EndpointLimit] = &[
    // Выдача бесплатной подписки — идемпотентна, но вызов лишний раз
    // создаёт лишние SELECT/INSERT. Burst 5 за 5 секунд.
    EndpointLimit {
        path_suffix: "/users/create-free-subscription",
        limit: 5,
        window_secs: 5,
        key_name: "bot_ep:free_sub",
    },
    // Административный подарок — реальный INSERT в subscriptions. Burst 5 за 5 секунд.
    EndpointLimit {
        path_suffix: "/admin/gift",
        limit: 5,
        window_secs: 5,
        key_name: "bot_ep:admin_gift",
    },
    // Покупка плана — SELECT + INSERT, может запускать orchestration. Burst 20 за 4 сек.
    EndpointLimit {
        path_suffix: "/purchase-plan",
        limit: 20,
        window_secs: 4,
        key_name: "bot_ep:purchase_plan",
    },
    // Бан пользователя — простой UPDATE, но административная операция. Burst 5 за 5 сек.
    EndpointLimit {
        path_suffix: "/admin/ban",
        limit: 5,
        window_secs: 5,
        key_name: "bot_ep:admin_ban",
    },
    // Анбан — аналогично бану.
    EndpointLimit {
        path_suffix: "/admin/unban",
        limit: 5,
        window_secs: 5,
        key_name: "bot_ep:admin_unban",
    },
];

const GLOBAL_LIMIT: usize = 50;
const GLOBAL_WINDOW_SECS: usize = 3;
const GLOBAL_KEY: &str = "rate:bot_global";

/// Формирует JSON-тело 429 и добавляет заголовок Retry-After.
fn rate_limited_response(retry_after_secs: u64) -> Response {
    let body = json!({
        "error": "rate_limited",
        "retry_after_secs": retry_after_secs,
    });
    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            (
                axum::http::header::RETRY_AFTER,
                retry_after_secs.to_string(),
            ),
            (
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            ),
        ],
        body.to_string(),
    )
        .into_response()
}

/// Middleware: ограничение частоты запросов для /api/v2/bot/* маршрутов.
///
/// Применяет два уровня проверки через Redis (fixed-window / Lua):
///   1. Endpoint-специфичный лимит — для дорогих операций (gift, ban, free-sub).
///   2. Глобальный лимит — для всего бот-роутера (защита от общей перегрузки).
///
/// При недоступности Redis пропускаем запрос с предупреждением в лог —
/// мы предпочитаем доступность защите при деградации инфраструктуры.
/// (Это соответствует поведению auth.rs в той же ситуации.)
pub async fn bot_rate_limit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();

    // ── Шаг 1: endpoint-специфичные лимиты ──────────────────────────────────
    for ep in ENDPOINT_LIMITS {
        if path.contains(ep.path_suffix) {
            let key = format!("rate:{}", ep.key_name);
            match state
                .redis
                .check_rate_limit(&key, ep.limit, ep.window_secs)
                .await
            {
                Ok(true) => {} // разрешено — продолжаем
                Ok(false) => {
                    tracing::warn!(
                        path = %path,
                        limit = ep.limit,
                        window_secs = ep.window_secs,
                        "Bot API endpoint rate limit exceeded"
                    );
                    return rate_limited_response(ep.window_secs as u64);
                }
                Err(e) => {
                    // Redis недоступен — логируем, пропускаем
                    tracing::error!(
                        err = %e,
                        path = %path,
                        "Redis rate-limit check failed for bot endpoint — allowing request"
                    );
                }
            }
            // Нашли совпавший паттерн — прерываем цикл (один эндпоинт — один лимит)
            break;
        }
    }

    // ── Шаг 2: глобальный лимит на весь bot-роутер ──────────────────────────
    match state
        .redis
        .check_rate_limit(GLOBAL_KEY, GLOBAL_LIMIT, GLOBAL_WINDOW_SECS)
        .await
    {
        Ok(true) => {} // разрешено
        Ok(false) => {
            tracing::warn!(
                path = %path,
                "Bot API global rate limit exceeded"
            );
            return rate_limited_response(GLOBAL_WINDOW_SECS as u64);
        }
        Err(e) => {
            tracing::error!(
                err = %e,
                "Redis global rate-limit check failed for bot API — allowing request"
            );
        }
    }

    next.run(req).await
}

// ============================================================================
// Тесты
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// Проверяем, что каждый endpoint-суффикс корректно сопоставляется с реальными путями.
    #[test]
    fn endpoint_limits_match_known_paths() {
        let known_paths = [
            "/api/v2/bot/users/create-free-subscription",
            "/api/v2/bot/admin/gift",
            "/api/v2/bot/users/42/purchase-plan",
            "/api/v2/bot/admin/ban",
            "/api/v2/bot/admin/unban",
        ];

        let expected_keys = [
            "bot_ep:free_sub",
            "bot_ep:admin_gift",
            "bot_ep:purchase_plan",
            "bot_ep:admin_ban",
            "bot_ep:admin_unban",
        ];

        for (path, expected_key) in known_paths.iter().zip(expected_keys.iter()) {
            let matched = ENDPOINT_LIMITS
                .iter()
                .find(|ep| path.contains(ep.path_suffix));
            assert!(
                matched.is_some(),
                "Path '{}' should match an endpoint limit",
                path
            );
            assert_eq!(
                matched.unwrap().key_name,
                *expected_key,
                "Path '{}' matched wrong limit key",
                path
            );
        }
    }

    /// Пути, которые НЕ должны совпадать с endpoint-лимитами.
    #[test]
    fn endpoint_limits_no_false_positives() {
        let unmatched_paths = [
            "/api/v2/bot/verify",
            "/api/v2/bot/users",
            "/api/v2/bot/plans",
            "/api/v2/bot/admin/stats",
        ];
        for path in &unmatched_paths {
            let matched = ENDPOINT_LIMITS
                .iter()
                .any(|ep| path.contains(ep.path_suffix));
            assert!(
                !matched,
                "Path '{}' should NOT match any endpoint limit",
                path
            );
        }
    }
}
