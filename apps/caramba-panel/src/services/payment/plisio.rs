// Plisio — глобальный криптоплатёжный процессор, простой API.
// API: https://api.plisio.net/api/v1/
// Аутентификация: query-параметр api_key.
// Подпись вебхука: поле verify_hash в теле (HMAC-SHA1 над отсортированными полями).

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;
use std::collections::BTreeMap;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

pub struct PlisioProvider {
    /// API Key из личного кабинета Plisio
    pub api_key: String,
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for PlisioProvider {
    fn name(&self) -> &str {
        "plisio"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount = format!("{:.2}", (session.amount as f64) / 100.0);
        let currency = session.currency.to_uppercase();
        let callback_url = format!(
            "https://{}/api/webhooks/payment/plisio",
            self.api_domain
        );
        let success_url = format!("https://t.me/{}", self.bot_username);

        // Plisio поддерживает GET с query-параметрами для создания инвойса.
        let url = reqwest::Url::parse_with_params(
            "https://api.plisio.net/api/v1/invoices/new",
            &[
                ("api_key", self.api_key.as_str()),
                ("order_number", session.id.to_string().as_str()),
                ("source_currency", currency.as_str()),
                ("source_amount", amount.as_str()),
                ("currency", "USDT"),
                ("allowed_psys_cids", "BTC,ETH,USDT,LTC,TRX"),
                ("callback_url", callback_url.as_str()),
                ("success_callback_url", success_url.as_str()),
                (
                    "description",
                    &format!("VPN Subscription (Product: {})", session.product_id),
                ),
            ],
        )
        .context("Не удалось сформировать URL запроса Plisio")?;

        let res = client
            .get(url)
            .send()
            .await
            .context("Не удалось отправить запрос в Plisio")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Plisio API Error: {}", error_text);
        }

        let resp: Value = res
            .json()
            .await
            .context("Не удалось разобрать ответ Plisio")?;

        let status = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "success" {
            anyhow::bail!("Plisio API вернул ошибку: {}", resp);
        }

        resp.pointer("/data/invoice_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Plisio не вернул invoice_url"))
    }

    async fn verify_webhook(&self, payload: &[u8], _signature: &str) -> Result<bool> {
        // Plisio встраивает verify_hash прямо в тело вебхука.
        // Алгоритм: HMAC-SHA1 над JSON-строкой из всех полей (sorted by key),
        // кроме самого verify_hash, с ключом api_key.
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Plisio")?;

        let received_hash = data
            .get("verify_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if received_hash.is_empty() {
            return Ok(false);
        }

        // Собираем отсортированный словарь без verify_hash.
        let obj = data
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Plisio webhook не является JSON-объектом"))?;

        let sorted: BTreeMap<String, Value> = obj
            .iter()
            .filter(|(k, _)| k.as_str() != "verify_hash")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let canonical = serde_json::to_string(&sorted)
            .context("Не удалось сериализовать поля для верификации Plisio")?;

        type HmacSha1 = Hmac<Sha1>;
        let mut mac = HmacSha1::new_from_slice(self.api_key.as_bytes())
            .context("Неверный HMAC-ключ Plisio")?;
        mac.update(canonical.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        Ok(received_hash == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Plisio")?;

        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_number = data
            .get("order_number")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if order_number.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "completed" | "mismatch" => {
                // mismatch = оплата поступила, но в неверной сумме — для VPN принимаем.
                Ok(PaymentWebhookAction::Completed {
                    external_id: order_number.to_string(),
                })
            }
            "expired" | "cancelled" | "error" => Ok(PaymentWebhookAction::Failed {
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
