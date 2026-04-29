use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha512;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct NowPaymentsInvoiceReq {
    price_amount: f64,
    price_currency: String,
    pay_currency: String,
    order_id: String,
    order_description: String,
    ipn_callback_url: String,
    success_url: String,
    cancel_url: String,
}

#[derive(Deserialize)]
struct NowPaymentsInvoiceRes {
    invoice_url: String,
}

pub struct NowPaymentsProvider {
    pub api_key: String,
    pub ipn_secret: String,
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for NowPaymentsProvider {
    fn name(&self) -> &str {
        "nowpayments"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let req_body = NowPaymentsInvoiceReq {
            price_amount: (session.amount as f64) / 100.0, // Amount is in cents
            price_currency: session.currency.to_uppercase(),
            pay_currency: "USDTTRC20".to_string(), // Or could be empty for full selection
            order_id: session.id.to_string(),
            order_description: format!("VPN Subscription (Product: {})", session.product_id),
            ipn_callback_url: format!(
                "https://{}/api/webhooks/payment/nowpayments",
                self.api_domain
            ),
            success_url: format!("https://t.me/{}", self.bot_username),
            cancel_url: format!("https://t.me/{}", self.bot_username),
        };

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let res = client
            .post("https://api.nowpayments.io/v1/invoice")
            .headers(headers)
            .json(&req_body)
            .send()
            .await
            .context("Failed to send request to NowPayments")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("NowPayments API Error: {}", error_text);
        }

        let invoice: NowPaymentsInvoiceRes = res
            .json()
            .await
            .context("Failed to parse NowPayments response")?;
        Ok(invoice.invoice_url)
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        type HmacSha512 = Hmac<Sha512>;
        let mut mac =
            HmacSha512::new_from_slice(self.ipn_secret.as_bytes()).context("Invalid HMAC key")?;

        let payload_str = std::str::from_utf8(payload).unwrap_or("");

        // NowPayments requires the payload dictionary to be sorted by keys. For robust parsing, we re-serialize it.
        // Or simply verify the raw payload bytes if the web framework preserves exact order.
        mac.update(payload_str.as_bytes());

        let result = mac.finalize().into_bytes();
        let computed_sig = hex::encode(result);

        Ok(computed_sig == signature)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;

        let status = data
            .get("payment_status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let order_id = data.get("order_id").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "finished" | "completed" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "failed" | "expired" | "refunded" => Ok(PaymentWebhookAction::Failed {
                reason: status.to_string(),
            }),
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}
