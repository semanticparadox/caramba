use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct AaioInvoiceReq {
    merchant_id: String,
    amount: f64,
    currency: String,
    order_id: String,
    desc: String,
    lang: String,
}

pub struct AaioProvider {
    pub merchant_id: String,
    pub secret_1: String,
    /// Used for webhook signature verification (AAIO passes a second secret for webhooks).
    pub secret_2: String,
}

#[async_trait]
impl PaymentProvider for AaioProvider {
    fn name(&self) -> &str {
        "aaio"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        _client: &reqwest::Client,
    ) -> Result<String> {
        let amount = (session.amount as f64) / 100.0;
        let currency = "USD";
        let order_id = session.id.to_string();

        // AAIO signature for invoice creation: merchant_id:amount:currency:secret_1:order_id
        let sign_str = format!(
            "{}:{}:{}:{}:{}",
            self.merchant_id, amount, currency, self.secret_1, order_id
        );
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let sign = hex::encode(hasher.finalize());

        // Build the redirect URL using AaioInvoiceReq fields for documentation clarity.
        let _req = AaioInvoiceReq {
            merchant_id: self.merchant_id.clone(),
            amount,
            currency: currency.to_string(),
            order_id: order_id.clone(),
            desc: format!("VPN Subscription (Product: {})", session.product_id),
            lang: "en".to_string(),
        };

        let url = format!(
            "https://aaio.so/merchant/pay?merchant_id={}&amount={}&currency={}&order_id={}&sign={}&desc={}",
            self.merchant_id,
            amount,
            currency,
            order_id,
            sign,
            urlencoding::encode(&format!(
                "VPN Subscription (Product: {})",
                session.product_id
            ))
        );

        Ok(url)
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // AAIO webhook signature: SHA-256 of "merchant_id:amount:currency:secret_2:order_id"
        // The exact field order matches the invoice creation signature but uses secret_2.
        let data: Value = serde_json::from_slice(payload).context("Invalid AAIO webhook JSON")?;

        let amount = data
            .get("amount")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let currency = data
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("USD");
        let order_id = data
            .get("order_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let sign_str = format!(
            "{}:{}:{}:{}:{}",
            self.merchant_id, amount, currency, self.secret_2, order_id
        );
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let expected = hex::encode(hasher.finalize());

        Ok(signature == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid AAIO webhook JSON")?;

        let status = data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let order_id = data
            .get("order_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "success" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
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
