// BTCPay Server — самохостинговый биткоин-платёжный процессор без KYC.
// API: <btcpay_url>/api/v1/stores/<store_id>/invoices  (Greenfield API v1)
// Аутентификация: заголовок Authorization: token <api_key>
// Подпись вебхука: HMAC-SHA256 hex, заголовок BTCPay-Sig: sha256=<hex>

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
struct BtcPayCheckout {
    redirect_url: String,
    redirect_automatically: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BtcPayMetadata {
    order_id: String,
}

#[derive(Serialize)]
struct BtcPayInvoiceReq {
    amount: f64,
    currency: String,
    metadata: BtcPayMetadata,
    checkout: BtcPayCheckout,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BtcPayInvoiceRes {
    checkout_link: Option<String>,
}

pub struct BtcPayProvider {
    /// URL инстанса BTCPay Server пользователя, например https://pay.example.com
    pub btcpay_url: String,
    /// Greenfield API key
    pub api_key: String,
    /// Store ID из BTCPay Server
    pub store_id: String,
    /// Shared secret из конфигурации вебхука в BTCPay
    pub webhook_secret: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for BtcPayProvider {
    fn name(&self) -> &str {
        "btcpay"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount = (session.amount as f64) / 100.0;
        let currency = session.currency.to_uppercase();
        let redirect_url = format!("https://t.me/{}", self.bot_username);

        let req_body = BtcPayInvoiceReq {
            amount,
            currency,
            metadata: BtcPayMetadata {
                order_id: session.id.to_string(),
            },
            checkout: BtcPayCheckout {
                redirect_url,
                redirect_automatically: true,
            },
        };

        let url = format!(
            "{}/api/v1/stores/{}/invoices",
            self.btcpay_url.trim_end_matches('/'),
            self.store_id
        );

        let res = client
            .post(&url)
            .header("Authorization", format!("token {}", self.api_key))
            .json(&req_body)
            .send()
            .await
            .context("Не удалось отправить запрос в BTCPay Server")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("BTCPay Server API Error: {}", error_text);
        }

        let resp: BtcPayInvoiceRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ BTCPay Server")?;

        resp.checkout_link
            .ok_or_else(|| anyhow::anyhow!("BTCPay Server не вернул checkoutLink"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        if self.webhook_secret.is_empty() {
            anyhow::bail!("btcpay_webhook_secret не задан — вебхук отклонён");
        }

        // Заголовок: BTCPay-Sig: sha256=<hex>
        // Подпись = HMAC-SHA256(webhook_secret, raw_body), значение в hex.
        let sig_hex = signature
            .strip_prefix("sha256=")
            .unwrap_or(signature)
            .trim();

        // Декодируем присланную подпись из hex. hex-разбор нечувствителен к
        // регистру, что устраняет ложные отказы из-за регистра символов.
        let provided = match hex::decode(sig_hex) {
            Ok(bytes) => bytes,
            // Невалидный hex не может совпасть с корректной подписью.
            Err(_) => return Ok(false),
        };

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .context("Неверный HMAC-ключ BTCPay")?;
        mac.update(payload);

        // verify_slice выполняет сравнение в постоянном времени (constant-time),
        // защищая от timing-атак на восстановление подписи.
        Ok(mac.verify_slice(&provided).is_ok())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке BTCPay")?;

        let event_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "InvoiceSettled" => {
                // metadata.orderId хранит UUID сессии.
                let order_id = data
                    .pointer("/metadata/orderId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if order_id.is_empty() {
                    tracing::warn!("BTCPay InvoiceSettled без metadata.orderId");
                    return Ok(PaymentWebhookAction::Ignored);
                }

                Ok(PaymentWebhookAction::Completed {
                    external_id: order_id.to_string(),
                })
            }
            "InvoiceExpired" | "InvoiceInvalid" => Ok(PaymentWebhookAction::Failed {
                reason: event_type.to_string(),
            }),
            _ => Ok(PaymentWebhookAction::Ignored),
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
