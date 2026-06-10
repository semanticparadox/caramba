// Coinbase Commerce — глобальный криптоплатёжный процессор.
// API: https://api.commerce.coinbase.com/
// Аутентификация: заголовок X-CC-Api-Key.
// Подпись вебхука: HMAC-SHA256 hex, заголовок X-CC-Webhook-Signature.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct CoinbaseLocalPrice {
    amount: String,
    currency: String,
}

#[derive(Serialize)]
struct CoinbaseMetadata {
    order_id: String,
}

#[derive(Serialize)]
struct CoinbaseInvoiceReq {
    name: String,
    description: String,
    pricing_type: String,
    local_price: CoinbaseLocalPrice,
    metadata: CoinbaseMetadata,
    redirect_url: String,
}

#[derive(Deserialize)]
struct CoinbaseData {
    hosted_url: Option<String>,
}

#[derive(Deserialize)]
struct CoinbaseInvoiceRes {
    data: Option<CoinbaseData>,
}

pub struct CoinbaseCommerceProvider {
    /// API Key из Coinbase Commerce Dashboard
    pub api_key: String,
    /// Shared Webhook Secret из Coinbase Commerce Dashboard
    pub webhook_secret: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for CoinbaseCommerceProvider {
    fn name(&self) -> &str {
        "coinbase_commerce"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount = format!("{:.2}", (session.amount as f64) / 100.0);
        let currency = session.currency.to_uppercase();
        let redirect_url = format!("https://t.me/{}", self.bot_username);

        let req_body = CoinbaseInvoiceReq {
            name: "VPN Subscription".to_string(),
            description: format!("VPN Subscription (Product: {})", session.product_id),
            pricing_type: "fixed_price".to_string(),
            local_price: CoinbaseLocalPrice { amount, currency },
            metadata: CoinbaseMetadata {
                order_id: session.id.to_string(),
            },
            redirect_url,
        };

        let res = client
            .post("https://api.commerce.coinbase.com/charges/")
            .header("X-CC-Api-Key", &self.api_key)
            .header("X-CC-Version", "2018-03-22")
            .json(&req_body)
            .send()
            .await
            .context("Не удалось отправить запрос в Coinbase Commerce")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Coinbase Commerce API Error: {}", error_text);
        }

        let resp: CoinbaseInvoiceRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ Coinbase Commerce")?;

        resp.data
            .and_then(|d| d.hosted_url)
            .ok_or_else(|| anyhow::anyhow!("Coinbase Commerce не вернул hosted_url"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        if self.webhook_secret.is_empty() {
            anyhow::bail!("coinbase_webhook_secret не задан — вебхук отклонён");
        }

        // Coinbase Commerce: заголовок X-CC-Webhook-Signature содержит
        // HMAC-SHA256(shared_secret, raw_body) в hex (подтверждено исходниками
        // официального SDK coinbase-commerce-node: crypto.createHmac('sha256', secret)
        //  .update(payload, 'utf8').digest('hex'), сравнение — constant-time secure-compare).
        // Декодируем присланную подпись из hex (разбор нечувствителен к регистру),
        // чтобы выполнить сравнение в постоянном времени и избежать timing-атак.
        let provided = match hex::decode(signature.trim()) {
            Ok(bytes) => bytes,
            // Невалидный hex не может совпасть с корректной подписью.
            Err(_) => return Ok(false),
        };

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .context("Неверный HMAC-ключ Coinbase Commerce")?;
        mac.update(payload);

        // verify_slice выполняет constant-time сравнение, как и официальный SDK.
        Ok(mac.verify_slice(&provided).is_ok())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Coinbase Commerce")?;

        let event_type = data
            .pointer("/event/type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match event_type {
            "charge:confirmed" | "charge:resolved" => {
                // metadata.order_id хранит UUID сессии.
                let order_id = data
                    .pointer("/event/data/metadata/order_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if order_id.is_empty() {
                    tracing::warn!("Coinbase Commerce: charge:confirmed без metadata.order_id");
                    return Ok(PaymentWebhookAction::Ignored);
                }

                Ok(PaymentWebhookAction::Completed {
                    external_id: order_id.to_string(),
                })
            }
            // charge:failed — терминальный отказ (charge истёк/отменён без оплаты).
            "charge:failed" => Ok(PaymentWebhookAction::Failed {
                reason: event_type.to_string(),
            }),
            // charge:delayed — НЕ отказ: оплата поступила с задержкой / после
            // истечения charge (UNRESOLVED, reason=delayed). Согласно официальной
            // документации Coinbase Commerce такой charge может быть позже
            // подтверждён мерчантом и придёт как charge:resolved (обрабатывается
            // выше как Completed). Поэтому здесь — Ignored, без ложного отказа
            // и без преждевременного исполнения заказа.
            "charge:delayed" => Ok(PaymentWebhookAction::Ignored),
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
