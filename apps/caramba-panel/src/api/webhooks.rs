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
    // Каждый провайдер передаёт подпись в своём заголовке:
    //   CryptoBot    — crypto-pay-api-signature  (HMAC-SHA256 hex)
    //   NowPayments  — x-nowpayments-sig         (HMAC-SHA512 hex)
    //   Lava.top     — Signature                 (HMAC-SHA256 hex)
    //   Cryptomus    — sign                      (MD5 hex, в заголовке)
    //   AAIO         — подпись передаётся в теле формы (поле sign),
    //                  поэтому здесь мы передаём пустую строку, а
    //                  AaioProvider::verify_webhook читает тело самостоятельно.
    let sig_str: String = match provider.as_str() {
        "cryptobot" => headers
            .get("crypto-pay-api-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        "nowpayments" => headers
            .get("x-nowpayments-sig")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        "lava" => headers
            .get("Signature")
            .or_else(|| headers.get("signature"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        "cryptomus" => headers
            .get("sign")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        // Stripe-Signature: "t=<ts>,v1=<hex>"
        "stripe" => headers
            .get("stripe-signature")
            .or_else(|| headers.get("Stripe-Signature"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        // AAIO embeds the signature in the form body; AaioProvider reads it from payload bytes.
        // manual/balance/stars don't use webhooks — signature is irrelevant.
        _ => String::new(),
    };

    match state
        .marketplace_service
        .handle_webhook(&provider, &body, &sig_str)
        .await
    {
        Ok(_) => {
            tracing::info!(provider = %provider, "Webhook processed successfully");
            (axum::http::StatusCode::OK, "OK").into_response()
        }
        Err(e) => {
            tracing::error!(provider = %provider, error = %e, "Webhook processing failed");
            // 400 so the provider logs a delivery failure and retries (except for duplicate-safe ones).
            (axum::http::StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}
