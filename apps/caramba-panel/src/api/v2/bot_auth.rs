use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use subtle::ConstantTimeEq;

/// Middleware: проверяет заголовок X-Bot-Token у всех /api/v2/bot/* маршрутов.
/// Токен сравнивается с переменной окружения PANEL_TOKEN за константное время
/// (защита от timing-атак).
pub async fn require_bot_token(req: Request, next: Next) -> impl IntoResponse {
    // Читаем ожидаемый токен из окружения. Если переменная не задана или пуста —
    // отклоняем все запросы, чтобы не допустить случайного открытого доступа.
    let expected = match std::env::var("PANEL_TOKEN") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            tracing::warn!("PANEL_TOKEN is not configured — bot API requests will be rejected");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    };

    let provided = req
        .headers()
        .get("X-Bot-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Сравнение за константное время: предотвращает утечку длины через timing
    let tokens_match = expected.as_bytes().ct_eq(provided.as_bytes()).into();

    if tokens_match {
        next.run(req).await.into_response()
    } else {
        tracing::warn!(
            path = %req.uri().path(),
            "Bot API request rejected: invalid or missing X-Bot-Token"
        );
        (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
    }
}
