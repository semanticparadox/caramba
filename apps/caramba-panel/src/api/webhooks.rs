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
    // Каждый провайдер передаёт подпись по-своему:
    //   CryptoBot    — crypto-pay-api-signature  (заголовок, HMAC-SHA256 hex)
    //   NowPayments  — x-nowpayments-sig         (заголовок, HMAC-SHA512 hex)
    //   Lava.ru biz  — Authorization             (заголовок, см. dev.lava.ru/business-webhook)
    //   Cryptomus    — поле `sign` в ТЕЛЕ JSON    (MD5 hex; НЕ заголовок —
    //                  doc.cryptomus.com/merchant-api/payments/webhook).
    //                  Здесь передаём пустую строку, подпись читается из тела.
    //   AAIO         — подпись передаётся в теле формы (поле sign),
    //                  поэтому здесь мы передаём пустую строку, а
    //                  AaioProvider::verify_webhook читает тело самостоятельно.
    // Helper: case-insensitive header read.
    let header = |names: &[&str]| -> String {
        for name in names {
            if let Some(v) = headers.get(*name).and_then(|v| v.to_str().ok()) {
                return v.to_string();
            }
        }
        String::new()
    };

    let sig_str: String = match provider.as_str() {
        "cryptobot" => header(&["crypto-pay-api-signature"]),
        "nowpayments" => header(&["x-nowpayments-sig"]),
        // Lava.ru Business API delivers the signature in the `Authorization` header,
        // NOT `Signature` (confirmed: dev.lava.ru/business-webhook —
        // "В заголовке Authorization содержится сигнатура подтверждающая валидность").
        "lava" => header(&["Authorization", "authorization"]),
        // Cryptomus puts the signature in the JSON body field `sign`, not a header.
        // Pass an empty string; CryptomusProvider::verify_webhook reads it from the body.
        "cryptomus" => String::new(),
        // Stripe-Signature: "t=<ts>,v1=<hex>"
        "stripe" => header(&["stripe-signature", "Stripe-Signature"]),
        // New providers (7 added in this audit pass):
        "wata" => header(&["X-Signature", "x-signature"]),
        "tribute" => header(&[
            "trbt-signature",
            "X-Tribute-Signature",
            "x-tribute-signature",
        ]),
        "btcpay" => header(&["BTCPay-Sig", "btcpay-sig"]),
        "oxapay" => header(&["HMAC", "hmac"]),
        "coinbase_commerce" => header(&["X-CC-Webhook-Signature", "x-cc-webhook-signature"]),
        // CrystalPay and Plisio embed the signature in the body — provider reads it directly.
        // AAIO does the same. manual/balance/stars don't use webhooks.
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
            let err_msg = e.to_string();
            tracing::error!(provider = %provider, error = %err_msg, "Webhook processing failed");

            // Page admins on security/operational events that need attention:
            //   - Invalid signature: either misconfigured webhook secret OR an attempted
            //     spoof. Either way, payments aren't being processed and admin must act.
            //   - Provider not found: someone is hitting a webhook URL for a provider
            //     we never registered — could be cleanup needed or recon attempt.
            // We DON'T page on every 400 (e.g. "no matching session") — those are noisy
            // and not actionable in real-time.
            if err_msg.contains("Invalid webhook signature")
                || err_msg.contains("Payment provider not found")
            {
                let pool = state.pool.clone();
                let bot_manager = state.bot_manager.clone();
                let provider_name = provider.clone();
                let alert_msg = if err_msg.contains("Invalid webhook signature") {
                    format!(
                        "🚨 *Payment webhook signature INVALID*\n\nProvider: `{}`\n\nReason: webhook secret in panel does not match what the provider used to sign, OR an unauthenticated party tried to spoof a payment. Verify the provider's webhook secret in admin settings.",
                        provider_name
                    )
                } else {
                    format!(
                        "⚠️ *Webhook for unknown provider*\n\nProvider: `{}`\n\nReceived a webhook but no provider with this name is registered. Check the URL the provider is calling.",
                        provider_name
                    )
                };
                tokio::spawn(async move {
                    bot_manager.notify_admins(&pool, &alert_msg).await;
                });
            }

            // 400 so the provider logs a delivery failure and retries (except for duplicate-safe ones).
            (axum::http::StatusCode::BAD_REQUEST, err_msg).into_response()
        }
    }
}
