// Tribute — нативная монетизация для Telegram-каналов.
// API: https://tribute.tg/api/v1/
// Аутентификация: Bearer token (API-ключ) из личного кабинета Tribute.
// Подпись вебхука: HMAC-SHA256 над raw body, заголовок `trbt-signature`.
// Ключ подписи = API-ключ (по докам Tribute). Если задан отдельный
// tribute_webhook_secret — используется он, иначе откатываемся на API-ключ.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct TributeInvoiceReq {
    amount: f64,
    currency: String,
    description: String,
    external_id: String,
}

#[derive(Deserialize)]
struct TributeInvoiceRes {
    url: Option<String>,
}

#[allow(dead_code)]
pub struct TributeProvider {
    /// API-ключ из панели Tribute
    pub api_key: String,
    /// Секрет для верификации подписи вебхуков
    pub webhook_secret: String,
    /// Username бота — для redirect URL при создании инвойса через платёжные ссылки.
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for TributeProvider {
    fn name(&self) -> &str {
        "tribute"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount = (session.amount as f64) / 100.0;
        let currency = session.currency.to_uppercase();

        let req_body = TributeInvoiceReq {
            amount,
            currency,
            description: format!("VPN Subscription (Product: {})", session.product_id),
            external_id: session.id.to_string(),
        };

        let res = client
            .post("https://tribute.tg/api/v1/payment_links")
            .bearer_auth(&self.api_key)
            .json(&req_body)
            .send()
            .await
            .context("Не удалось отправить запрос в Tribute API")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Tribute API Error: {}", error_text);
        }

        let resp: TributeInvoiceRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ Tribute")?;

        resp.url
            .ok_or_else(|| anyhow::anyhow!("Tribute API не вернул URL для оплаты"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // Tribute signs the webhook body with the account API key. Allow an explicit
        // override secret, but fall back to the API key (documented behavior).
        let key = if !self.webhook_secret.is_empty() {
            self.webhook_secret.as_str()
        } else if !self.api_key.is_empty() {
            self.api_key.as_str()
        } else {
            anyhow::bail!("ни tribute_webhook_secret, ни tribute_api_key не заданы — вебхук отклонён");
        };

        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(key.as_bytes()).context("Неверный HMAC-ключ Tribute")?;
        mac.update(payload);
        let expected = hex::encode(mac.finalize().into_bytes());
        Ok(signature.eq_ignore_ascii_case(&expected))
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Tribute")?;

        let status = data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // external_id — UUID сессии, переданный при создании инвойса.
        let external_id = data
            .get("external_id")
            .and_then(|v| v.as_str())
            .or_else(|| data.get("order_id").and_then(|v| v.as_str()))
            .unwrap_or("");

        if external_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "paid" | "success" | "completed" | "active" => Ok(PaymentWebhookAction::Completed {
                external_id: external_id.to_string(),
            }),
            "failed" | "cancelled" | "expired" | "error" => Ok(PaymentWebhookAction::Failed {
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
