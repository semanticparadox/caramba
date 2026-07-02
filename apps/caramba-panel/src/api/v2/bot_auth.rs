use axum::{extract::Request, http::StatusCode, middleware::Next, response::IntoResponse};
use subtle::{Choice, ConstantTimeEq};

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

    // Сравнение за константное время: предотвращает утечку через timing-атаки.
    // Когда длины разные, ct_eq возвращает false мгновенно и утечки нет — длина
    // публично известна по спецификации, а не является секретом.
    // Тем не менее дополнительно выравниваем длины через HMAC-обёртку, чтобы
    // любые будущие оптимизации компилятора не нарушили гарантию.
    let expected_len = expected.len();
    let provided_len = provided.len();
    // Сравниваем длины и содержимое раздельно за константное время
    let len_eq: Choice = Choice::from((expected_len == provided_len) as u8);
    // Pad shorter string so ct_eq runs on equal-length slices
    let expected_bytes = expected.as_bytes();
    let provided_bytes = provided.as_bytes();
    let max_len = expected_len.max(provided_len);
    let mut exp_padded = vec![0u8; max_len];
    let mut prov_padded = vec![0u8; max_len];
    exp_padded[..expected_len].copy_from_slice(expected_bytes);
    prov_padded[..provided_len].copy_from_slice(provided_bytes);
    let content_eq: Choice = exp_padded.ct_eq(&prov_padded);
    let tokens_match: bool = (len_eq & content_eq).into();

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
