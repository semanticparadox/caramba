use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use base64::Engine;
use caramba_db::models::payment::PaymentType;
use caramba_db::models::store::{PaymentSession, User};
use chrono::Utc;

use super::provider::{PaymentProvider, PaymentWebhookAction};

#[derive(Serialize)]
struct CryptomusInvoiceReq {
    amount: String,
    currency: String,
    order_id: String,
    url_callback: String,
    url_return: String,
    additional_data: String,
}

#[derive(Deserialize)]
struct CryptomusInvoiceRes {
    result: Option<CryptomusInvoiceDetail>,
}

#[derive(Deserialize)]
struct CryptomusInvoiceDetail {
    url: String,
}

pub struct CryptomusProvider {
    pub merchant_id: String,
    pub api_key: String,
}

pub struct CryptomusAdapter {
    merchant_id: String,
    api_key: String,
}

impl CryptomusAdapter {
    pub fn new(merchant_id: String, api_key: String) -> Self {
        Self {
            merchant_id,
            api_key,
        }
    }

    fn generate_signature(&self, body: &str) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let to_hash = format!("{}{}", encoded, self.api_key);
        format!("{:x}", md5::compute(to_hash.as_bytes()))
    }
}

#[async_trait]
impl crate::services::payment::PaymentAdapter for CryptomusAdapter {
    async fn create_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
        bot_username: &str,
        api_domain: &str,
    ) -> Result<String> {
        let payload_str = payment_type.to_payload_string(user_id);
        let order_id = format!("{}_{}", user_id, Utc::now().timestamp());

        let body_json = serde_json::json!({
            "amount": amount_usd.to_string(),
            "currency": "USD",
            "order_id": order_id,
            "url_callback": format!("https://{}/caramba-api/payments/cryptomus", api_domain),
            "url_return": format!("https://t.me/{}", bot_username),
            "additional_data": payload_str
        });

        let body_str = serde_json::to_string(&body_json)?;
        let sign = self.generate_signature(&body_str);

        let client = reqwest::Client::new();
        let resp = client
            .post("https://api.cryptomus.com/v1/payment")
            .header("merchant", &self.merchant_id)
            .header("sign", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?;

        let resp_json: serde_json::Value = resp.json().await?;

        if let Some(result) = resp_json.get("result") {
            if let Some(url) = result.get("url").and_then(|u| u.as_str()) {
                return Ok(url.to_string());
            }
        }

        Err(anyhow::anyhow!("Cryptomus Error: {:?}", resp_json))
    }

    fn verify_signature(&self, payload: &str, signature: Option<&str>) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow::anyhow!("Missing sign header for Cryptomus"))?;
        let expected = self.generate_signature(payload);

        if sig == expected {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Invalid Cryptomus signature"))
        }
    }

    fn name(&self) -> &str {
        "cryptomus"
    }
}

impl CryptomusProvider {
    fn generate_signature(&self, body: &str) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let to_hash = format!("{}{}", encoded, self.api_key);
        format!("{:x}", md5::compute(to_hash.as_bytes()))
    }
}

#[async_trait]
impl PaymentProvider for CryptomusProvider {
    fn name(&self) -> &str {
        "cryptomus"
    }

    async fn create_invoice(&self, session: &PaymentSession, _user: &User, client: &reqwest::Client) -> Result<String> {
        let body_json = serde_json::json!({
            "amount": format!("{:.2}", (session.amount as f64) / 100.0),
            "currency": "USD",
            "order_id": session.id.to_string(),
            "url_callback": "https://your-api-domain.com/api/webhooks/payment/cryptomus",
            "url_return": "https://t.me/your_bot",
        });

        let body_str = serde_json::to_string(&body_json)?;
        let sign = self.generate_signature(&body_str);

        let res = client
            .post("https://api.cryptomus.com/v1/payment")
            .header("merchant", &self.merchant_id)
            .header("sign", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await
            .context("Failed to send request to Cryptomus")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Cryptomus API Error: {}", error_text);
        }

        let resp: CryptomusInvoiceRes = res.json().await.context("Failed to parse Cryptomus response")?;
        if let Some(detail) = resp.result {
            Ok(detail.url)
        } else {
            anyhow::bail!("Cryptomus API missing invoice URL");
        }
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        let payload_str = std::str::from_utf8(payload).unwrap_or("");
        let expected = self.generate_signature(payload_str);
        Ok(signature == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;
        
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_id = data.get("order_id").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "success" | "paid" => Ok(PaymentWebhookAction::Completed { external_id: order_id.to_string() }),
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(&self, _session: &PaymentSession, _client: &reqwest::Client) -> Result<String> {
        Ok("pending".to_string())
    }
}
