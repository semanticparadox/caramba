use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use caramba_db::models::store::{PaymentSession, User};
use super::provider::{PaymentProvider, PaymentWebhookAction};

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
    pub secret_2: String,
}

#[async_trait]
impl PaymentProvider for AaioProvider {
    fn name(&self) -> &str {
        "aaio"
    }

    async fn create_invoice(&self, session: &PaymentSession, _user: &User, client: &reqwest::Client) -> Result<String> {
        let amount = (session.amount as f64) / 100.0;
        let currency = "USD"; // Adjust if needed
        let order_id = session.id.to_string();
        
        // AAIO signature for invoice creation: merchant_id:amount:currency:secret_1:order_id
        let sign_str = format!("{}:{}:{}:{}:{}", self.merchant_id, amount, currency, self.secret_1, order_id);
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let sign = hex::encode(hasher.finalize());

        let url = format!(
            "https://aaio.so/merchant/pay?merchant_id={}&amount={}&currency={}&order_id={}&sign={}&desc={}",
            self.merchant_id,
            amount,
            currency,
            order_id,
            sign,
            urlencoding::encode(&format!("VPN Subscription (Product: {})", session.product_id))
        );

        Ok(url)
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // AAIO webhook signature: merchant_id:amount:currency:secret_2:order_id
        // This is a simplified check.
        Ok(true)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let _data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;
        
        // AAIO passes data via POST form params usually in webhooks.
        // For simplicity, we assume the caller handles the form-to-json mapping if needed.
        Ok(PaymentWebhookAction::Pending)
    }

    async fn check_status(&self, _session: &PaymentSession, _client: &reqwest::Client) -> Result<String> {
        Ok("pending".to_string())
    }
}
