use crate::services::payment::PaymentAdapter;
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use caramba_db::models::payment::PaymentType;
use chrono::Utc;
use serde_json::json;

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
impl PaymentAdapter for CryptomusAdapter {
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

        let body_json = json!({
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
