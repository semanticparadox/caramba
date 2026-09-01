use crate::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;

/// Public health-check endpoint — `GET /api/health`.
///
/// Designed for external uptime monitors, k8s readiness probes, load balancers.
/// Returns:
/// - **200 OK** + JSON when DB is reachable. (DB is the only hard requirement
///   for the panel to do anything useful; Redis/bot-token are optional and
///   reported but don't fail the check.)
/// - **503 Service Unavailable** + JSON when DB is unreachable.
///
/// No auth, no PII. Safe to expose publicly. Response is always a parseable
/// JSON object so probes can grep specific subsystems.
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    db: SubsystemStatus,
    redis: SubsystemStatus,
    bot_token_configured: bool,
    active_nodes: i64,
    active_frontends: i64,
}

#[derive(Serialize)]
struct SubsystemStatus {
    ok: bool,
    detail: Option<String>,
}

pub async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    // 1. DB — hard requirement
    // Проба живости НЕ декодирует результат.
    //
    // Раньше результат `SELECT 1` разбирался как `i64`, и это не работало
    // никогда: Postgres типизирует голый целочисленный литерал как INT4, а
    // восьмибайтный тип требует INT8. Декодирование падало с «mismatched
    // types», health объявлял живую базу мёртвой и отдавал 503 — с апреля
    // 2026, то есть монитор, нацеленный на этот адрес, всё это время горел
    // красным по несуществующей причине.
    //
    // Проверять здесь нужно ровно одно: что запрос доходит до базы и
    // возвращается. Значение не нужно никому, поэтому не декодируем его вовсе —
    // так этот класс ошибки тут не воспроизводится в принципе. Повторное
    // появление ловит `tests/sql_dialect_guard.rs`; там же объяснено, почему
    // формулировка выше избегает самого запрещённого выражения.
    let (db_ok, db_detail, active_nodes, active_frontends) =
        match sqlx::query("SELECT 1").fetch_one(&state.pool).await {
            Ok(_) => {
                let nodes: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM nodes WHERE status = 'active' AND is_enabled = TRUE",
                )
                .fetch_one(&state.pool)
                .await
                .unwrap_or(0);
                let frontends: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM frontend_servers WHERE status != 'offline'",
                )
                .fetch_one(&state.pool)
                .await
                .unwrap_or(0);
                (true, None, nodes, frontends)
            }
            Err(e) => (false, Some(e.to_string()), 0, 0),
        };

    // 2. Redis — soft. Best-effort PING. Not failing health on this since panel
    //    can run degraded without Redis (rate limits, sub-config cache disabled).
    let (redis_ok, redis_detail) = match state.redis.ping().await {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };

    // 3. Bot token — informational. Empty token = bot disabled, panel still works.
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    let bot_token_configured = !bot_token.trim().is_empty();

    let body = HealthResponse {
        status: if db_ok { "ok" } else { "degraded" },
        version: env!("CARGO_PKG_VERSION"),
        db: SubsystemStatus {
            ok: db_ok,
            detail: db_detail,
        },
        redis: SubsystemStatus {
            ok: redis_ok,
            detail: redis_detail,
        },
        bot_token_configured,
        active_nodes,
        active_frontends,
    };

    let code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body)).into_response()
}
