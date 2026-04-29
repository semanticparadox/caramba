// OxaPay — криптоплатёжный провайдер, популярный в РФ, минимальный KYC.
// API: https://api.oxapay.com/
// Аутентификация: merchant API key передаётся в теле запроса.
// Подпись вебхука: заголовок HMAC содержит HMAC-SHA512(body, key=merchant_api_key).

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha512;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OxaPayInvoiceReq {
    merchant: String,
    amount: f64,
    currency: String,
    life_time: u32,
    callback_url: String,
    return_url: String,
    description: String,
    order_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OxaPayInvoiceRes {
    result: Option<u32>,
    pay_link: Option<String>,
    message: Option<String>,
}

pub struct OxaPayProvider {
    /// Merchant API Key из личного кабинета OxaPay
    pub merchant_key: String,
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for OxaPayProvider {
    fn name(&self) -> &str {
        "oxapay"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount = (session.amount as f64) / 100.0;
        let currency = session.currency.to_uppercase();
        let return_url = format!("https://t.me/{}", self.bot_username);
        let callback_url = format!(
            "https://{}/api/webhooks/payment/oxapay",
            self.api_domain
        );

        let req_body = OxaPayInvoiceReq {
            merchant: self.merchant_key.clone(),
            amount,
            currency,
            life_time: 60, // минут
            callback_url,
            return_url,
            description: format!("VPN Subscription (Product: {})", session.product_id),
            order_id: session.id.to_string(),
        };

        let res = client
            .post("https://api.oxapay.com/merchants/request")
            .json(&req_body)
            .send()
            .await
            .context("Не удалось отправить запрос в OxaPay")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("OxaPay API Error: {}", error_text);
        }

        let resp: OxaPayInvoiceRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ OxaPay")?;

        // result == 100 означает успех в OxaPay API
        if resp.result != Some(100) {
            anyhow::bail!(
                "OxaPay вернул ошибку: {:?}",
                resp.message.unwrap_or_else(|| "unknown".to_string())
            );
        }

        resp.pay_link
            .ok_or_else(|| anyhow::anyhow!("OxaPay не вернул payLink"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // OxaPay передаёт HMAC-SHA512(body, key=merchant_api_key) в заголовке HMAC.
        type HmacSha512 = Hmac<Sha512>;
        let mut mac = HmacSha512::new_from_slice(self.merchant_key.as_bytes())
            .context("Неверный HMAC-ключ OxaPay")?;
        mac.update(payload);
        let expected = hex::encode(mac.finalize().into_bytes());
        Ok(signature == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке OxaPay")?;

        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_id = data
            .get("orderId")
            .or_else(|| data.get("order_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "Paid" | "paid" | "Completed" | "completed" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "Expired" | "expired" | "Error" | "error" | "Refunded" | "refunded" => {
                Ok(PaymentWebhookAction::Failed {
                    reason: status.to_string(),
                })
            }
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
