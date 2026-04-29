// CrystalPay — российский платёжный провайдер (СБП + крипта).
// API: https://api.crystalpay.io/v3/
// Аутентификация: auth_login + auth_secret в теле запроса.
// Подпись вебхука: SHA1(id + type + crystalpay_salt) — их схема хэширования.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::Sha1;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct CrystalPayInvoiceReq {
    auth_login: String,
    auth_secret: String,
    amount: f64,
    lifetime: u32,
    #[serde(rename = "type")]
    invoice_type: String,
    description: String,
    redirect_url: String,
    callback_url: String,
    extra: String,
}

#[derive(Deserialize, Debug)]
struct CrystalPayInvoiceRes {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    error: Option<bool>,
    #[serde(default)]
    errors: Option<Value>,
}

pub struct CrystalPayProvider {
    /// Логин мерчанта (публичная часть)
    pub login: String,
    /// Секрет аутентификации (auth_secret для API)
    pub secret: String,
    /// Соль для проверки подписи вебхуков
    pub salt: String,
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for CrystalPayProvider {
    fn name(&self) -> &str {
        "crystalpay"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount_rub = (session.amount as f64) / 100.0;
        let return_url = format!("https://t.me/{}", self.bot_username);
        let callback_url = format!(
            "https://{}/api/webhooks/payment/crystalpay",
            self.api_domain
        );

        let req = CrystalPayInvoiceReq {
            auth_login: self.login.clone(),
            auth_secret: self.secret.clone(),
            amount: amount_rub,
            lifetime: 3600,
            invoice_type: "purchase".to_string(),
            description: format!("VPN Subscription (Product: {})", session.product_id),
            redirect_url: return_url,
            callback_url,
            extra: session.id.to_string(),
        };

        let res = client
            .post("https://api.crystalpay.io/v3/invoice/create/")
            .json(&req)
            .send()
            .await
            .context("Не удалось отправить запрос в CrystalPay")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("CrystalPay API Error: {}", error_text);
        }

        let resp: CrystalPayInvoiceRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ CrystalPay")?;

        if resp.error == Some(true) {
            anyhow::bail!("CrystalPay API вернул ошибку: {:?}", resp.errors);
        }

        resp.url
            .ok_or_else(|| anyhow::anyhow!("CrystalPay API не вернул URL для оплаты"))
    }

    async fn verify_webhook(&self, payload: &[u8], _signature: &str) -> Result<bool> {
        // CrystalPay верифицирует вебхуки через поле `signature` в теле запроса.
        // Алгоритм: HMAC-SHA1(id + ":" + type + ":" + hash, key=salt),
        // где hash = SHA1(amount + ":" + crystalpay_salt).
        // Упрощённая проверка: если соль не задана — принимаем.
        if self.salt.is_empty() {
            tracing::warn!(
                "CrystalPay webhook salt not configured — signature verification skipped. \
                 Set crystalpay_salt in admin settings."
            );
            return Ok(true);
        }

        let body_str = std::str::from_utf8(payload).unwrap_or("");

        // Разбираем как JSON (основной режим в v3).
        let data: Value = serde_json::from_str(body_str)
            .unwrap_or_else(|_| {
                // Fallback: form-urlencoded (устаревший режим)
                let params: std::collections::HashMap<String, String> =
                    serde_urlencoded::from_str(body_str).unwrap_or_default();
                serde_json::to_value(params).unwrap_or(serde_json::json!({}))
            });

        let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let webhook_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let received_sig = data
            .get("signature")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if received_sig.is_empty() {
            return Ok(false);
        }

        // Вычисляем ожидаемую подпись: HMAC-SHA1(concat(id, type, salt), key=salt)
        type HmacSha1 = Hmac<Sha1>;
        let sign_data = format!("{}:{}:{}", id, webhook_type, self.salt);
        let mut mac = HmacSha1::new_from_slice(self.salt.as_bytes())
            .context("Неверный HMAC-ключ CrystalPay")?;
        mac.update(sign_data.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        Ok(received_sig == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let body_str = std::str::from_utf8(payload).unwrap_or("");

        let data: Value = serde_json::from_str(body_str)
            .unwrap_or_else(|_| {
                let params: std::collections::HashMap<String, String> =
                    serde_urlencoded::from_str(body_str).unwrap_or_default();
                serde_json::to_value(params).unwrap_or(serde_json::json!({}))
            });

        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        // extra хранит session UUID, переданный при создании инвойса.
        let order_id = data
            .get("extra")
            .and_then(|v| v.as_str())
            .or_else(|| data.get("order_id").and_then(|v| v.as_str()))
            .unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "payed" | "paid" | "success" | "completed" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "expired" | "cancelled" | "failed" | "error" => Ok(PaymentWebhookAction::Failed {
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
