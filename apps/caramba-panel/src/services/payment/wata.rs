// WATA — платёжный провайдер для РФ (СБП, карты физлиц).
// API: https://api.wata.pro
// Аутентификация: Bearer JWT-токен в заголовке Authorization.
// Подпись вебхука: HMAC-SHA256 над raw body, ключ = wata_webhook_secret.
// (WATA изначально использовал RSA-SHA512, но в 2024–2025 мигрировали на HMAC-SHA256
//  для упрощения интеграций. Если ключ не задан — принимаем с предупреждением.)

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WataInvoiceReq {
    amount: f64,
    currency: String,
    description: String,
    success_redirect_url: String,
    fail_redirect_url: String,
    expiration_date_time: String,
    order_id: String,
}

#[derive(Deserialize)]
struct WataInvoiceRes {
    url: Option<String>,
}

#[allow(dead_code)]
pub struct WataProvider {
    /// JWT API-токен из личного кабинета WATA
    pub jwt_token: String,
    /// Секрет для проверки подписи вебхука (HMAC-SHA256).
    /// Если пустой — подпись не верифицируется, в лог выводится предупреждение.
    pub webhook_secret: String,
    /// Домен панели — используется для формирования callback_url в будущих версиях API.
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for WataProvider {
    fn name(&self) -> &str {
        "wata"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        // WATA работает в рублях — amount в центах, конвертируем в рубли.
        // Если валюта сессии уже RUB — делим на 100; иначе передаём как есть.
        let amount_rub = (session.amount as f64) / 100.0;

        // Время истечения — 1 час от текущего момента, формат ISO 8601.
        let expiration = chrono::Utc::now() + chrono::Duration::hours(1);
        let expiration_str = expiration.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let return_url = format!("https://t.me/{}", self.bot_username);

        let req_body = WataInvoiceReq {
            amount: amount_rub,
            currency: "RUB".to_string(),
            description: format!("VPN Subscription (Product: {})", session.product_id),
            success_redirect_url: return_url.clone(),
            fail_redirect_url: return_url,
            expiration_date_time: expiration_str,
            order_id: session.id.to_string(),
        };

        let res = client
            .post("https://api.wata.pro/api/h2h/links")
            .bearer_auth(&self.jwt_token)
            .json(&req_body)
            .send()
            .await
            .context("Не удалось отправить запрос в WATA API")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("WATA API Error: {}", error_text);
        }

        let resp: WataInvoiceRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ WATA")?;

        resp.url.ok_or_else(|| anyhow::anyhow!("WATA API не вернул ссылку для оплаты"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        if self.webhook_secret.is_empty() {
            // Without a configured secret we cannot authenticate the callback, so we
            // reject it rather than trust an unsigned payload. Accepting unsigned
            // webhooks would let anyone POST a forged "Paid" event for a known session
            // and obtain free fulfillment. Configure wata_webhook_secret in admin
            // settings to enable WATA payments.
            tracing::error!(
                "WATA webhook secret not configured — rejecting unsigned webhook. \
                 Set wata_webhook_secret in admin settings to enable WATA."
            );
            return Ok(false);
        }

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .context("Неверный HMAC-ключ WATA")?;
        mac.update(payload);
        let expected = hex::encode(mac.finalize().into_bytes());
        Ok(signature == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Неверный JSON в вебхуке WATA")?;

        let status = data
            .get("transactionStatus")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let order_id = data
            .get("orderId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "Paid" | "paid" | "success" | "Success" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "Failed" | "failed" | "error" | "Error" | "Expired" | "expired" => {
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
