use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct CryptoBotInvoiceReq {
    asset: String,
    amount: String,
    description: String,
    hidden_message: String,
    paid_btn_name: String,
    paid_btn_url: String,
    payload: String,
}

#[derive(Deserialize)]
struct CryptoBotInvoiceRes {
    ok: bool,
    result: Option<CryptoBotInvoiceDetail>,
}

#[derive(Deserialize)]
struct CryptoBotInvoiceDetail {
    pay_url: String,
}

pub struct CryptoBotProvider {
    pub token: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for CryptoBotProvider {
    fn name(&self) -> &str {
        "cryptobot"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        // Amount must be formatted as a string for CryptoBot
        let amount_str = format!("{:.2}", (session.amount as f64) / 100.0);

        // CryptoBot settles in crypto assets. Honor a per-method currency override when it
        // names a supported asset; otherwise default to USDT.
        let asset = {
            let c = session.currency.trim().to_uppercase();
            const SUPPORTED: [&str; 9] =
                ["USDT", "TON", "BTC", "ETH", "LTC", "BNB", "TRX", "USDC", "JET"];
            if SUPPORTED.contains(&c.as_str()) {
                c
            } else {
                "USDT".to_string()
            }
        };

        let req_body = CryptoBotInvoiceReq {
            asset,
            amount: amount_str,
            description: format!("VPN Subscription (Product: {})", session.product_id),
            hidden_message: "Thank you for your purchase!".to_string(),
            paid_btn_name: "callback".to_string(),
            paid_btn_url: format!("https://t.me/{}", self.bot_username),
            payload: session.id.to_string(), // Internal reference
        };

        let mut headers = HeaderMap::new();
        headers.insert("Crypto-Pay-API-Token", HeaderValue::from_str(&self.token)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let res = client
            .post("https://pay.crypt.bot/api/createInvoice")
            .headers(headers)
            .json(&req_body)
            .send()
            .await
            .context("Failed to send request to CryptoBot")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("CryptoBot API Error: {}", error_text);
        }

        let invoice: CryptoBotInvoiceRes = res
            .json()
            .await
            .context("Failed to parse CryptoBot response")?;

        if !invoice.ok {
            anyhow::bail!("CryptoBot API returned not ok");
        }

        if let Some(detail) = invoice.result {
            Ok(detail.pay_url)
        } else {
            anyhow::bail!("CryptoBot API missing result detail");
        }
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // CryptoBot webhook signature uses HMAC-SHA256
        // The secret key is the SHA256 hash of the API Token.
        let mut hasher = Sha256::new();
        hasher.update(self.token.as_bytes());
        let secret_hash = hasher.finalize();

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(&secret_hash).context("Invalid HMAC key")?;

        mac.update(payload);
        let result = mac.finalize().into_bytes();
        let computed_sig = hex::encode(result);

        Ok(computed_sig == signature)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;

        let update_type = data
            .get("update_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if update_type != "invoice_paid" {
            return Ok(PaymentWebhookAction::Ignored);
        }

        let payload_id = data
            .get("payload")
            .and_then(|v| v.get("payload"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if payload_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        Ok(PaymentWebhookAction::Completed {
            external_id: payload_id.to_string(),
        })
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}
