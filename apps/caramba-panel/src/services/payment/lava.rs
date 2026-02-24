use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use caramba_db::models::store::{PaymentSession, User};
use super::provider::{PaymentProvider, PaymentWebhookAction};

#[derive(Serialize)]
struct LavaInvoiceReq {
    account: String,
    amount: f64,
    orderId: String,
    comment: String,
    hookUrl: String,
}

#[derive(Deserialize)]
struct LavaInvoiceRes {
    status: bool,
    url: Option<String>,
}

pub struct LavaProvider {
    pub project_id: String,
    pub secret_key: String,
}

#[async_trait]
impl PaymentProvider for LavaProvider {
    fn name(&self) -> &str {
        "lava"
    }

    async fn create_invoice(&self, session: &PaymentSession, _user: &User, client: &reqwest::Client) -> Result<String> {
        let req_body = LavaInvoiceReq {
            account: self.project_id.clone(),
            amount: (session.amount as f64) / 100.0,
            orderId: session.id.to_string(),
            comment: format!("VPN Subscription (Product: {})", session.product_id),
            hookUrl: "https://your-api-domain.com/api/webhooks/payment/lava".to_string(), // TODO: Get from settings
        };

        let res = client
            .post("https://api.lava.top/business/invoice/create")
            .header("Authorization", &self.secret_key)
            .json(&req_body)
            .send()
            .await
            .context("Failed to send request to Lava")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Lava API Error: {}", error_text);
        }

        let invoice: LavaInvoiceRes = res.json().await.context("Failed to parse Lava response")?;
        if let Some(url) = invoice.url {
            Ok(url)
        } else {
            anyhow::bail!("Lava API missing invoice URL");
        }
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // Lava signature verification logic
        // TODO: Implement specific HMAC or MD5 check if needed
        Ok(true) 
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;
        
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_id = data.get("orderId").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "success" => Ok(PaymentWebhookAction::Completed { external_id: order_id.to_string() }),
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(&self, _session: &PaymentSession, _client: &reqwest::Client) -> Result<String> {
        Ok("pending".to_string())
    }
}
