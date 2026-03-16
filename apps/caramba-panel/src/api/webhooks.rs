use axum::body::Bytes;
use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::post,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/payment/{provider}", post(handle_payment_webhook))
}

async fn handle_payment_webhook(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Determine signature based on provider. NowPayments uses x-nowpayments-sig, CryptoBot uses crypto-pay-api-signature.
    let signature = match provider.as_str() {
        "nowpayments" => headers
            .get("x-nowpayments-sig")
            .and_then(|v| v.to_str().ok()),
        "cryptobot" => headers
            .get("crypto-pay-api-signature")
            .and_then(|v| v.to_str().ok()),
        _ => None, // Some providers don't use webhooks or headers
    };

    let sig_str = signature.unwrap_or("");

    match state
        .marketplace_service
        .handle_webhook(&provider, &body, sig_str)
        .await
    {
        Ok(_) => {
            tracing::info!("Successfully processed webhook for provider: {}", provider);
            (axum::http::StatusCode::OK, "OK").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to process webhook for {}: {}", provider, e);
            // We return 200 OK even on error to prevent webhook retries if the signature was invalid,
            // but return 400 if it's genuinely a bad request we want them to retry.
            // For safety, let's return 400 so we can see it in logs, but some providers prefer 200 to stop retry loops.
            (axum::http::StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}
