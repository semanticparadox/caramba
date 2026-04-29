use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

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

        // AAIO использует GET-ссылку для оплаты — параметры передаются в query string.
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

    async fn verify_webhook(&self, payload: &[u8], _signature: &str) -> Result<bool> {
        // AAIO отправляет вебхук как application/x-www-form-urlencoded.
        // Подпись передаётся в поле `sign` внутри тела запроса (не в заголовке).
        // Алгоритм верификации: SHA-256("merchant_id:amount:currency:secret_2:order_id")
        let body_str = std::str::from_utf8(payload).unwrap_or("");

        // Разбираем тело как form-urlencoded; при неудаче — как JSON (устаревший формат)
        let params: std::collections::HashMap<String, String> =
            serde_urlencoded::from_str(body_str).unwrap_or_default();

        let data: Value = if params.is_empty() {
            // Если urldecode не дал результата — пробуем JSON
            serde_json::from_slice(payload).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::to_value(&params).unwrap_or(serde_json::json!({}))
        };

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
        let sign_from_body = data
            .get("sign")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if sign_from_body.is_empty() {
            return Ok(false);
        }

        let sign_str = format!(
            "{}:{}:{}:{}:{}",
            self.merchant_id, amount, currency, self.secret_2, order_id
        );
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let expected = hex::encode(hasher.finalize());

        Ok(sign_from_body == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        // AAIO webhooks are form-urlencoded; parse accordingly.
        let body_str = std::str::from_utf8(payload).unwrap_or("");
        let params: std::collections::HashMap<String, String> =
            serde_urlencoded::from_str(body_str).unwrap_or_default();
        let data: Value = if params.is_empty() {
            serde_json::from_slice(payload).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::to_value(&params).unwrap_or(serde_json::json!({}))
        };

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
