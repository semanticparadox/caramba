use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

pub struct LavaProvider {
    pub project_id: String,
    pub secret_key: String,
    pub api_domain: String,
}

#[derive(Deserialize)]
struct LavaResponse {
    data: Option<LavaData>,
    error: Option<Value>,
}

#[derive(Deserialize)]
struct LavaData {
    url: String,
}

#[async_trait]
impl PaymentProvider for LavaProvider {
    fn name(&self) -> &str {
        "lava"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        // Lava.ru Business API v2 — HMAC-SHA256 подпись тела запроса
        let amount_rub = (session.amount as f64) / 100.0;

        let json_body = serde_json::json!({
            "sum": amount_rub,
            "orderId": session.id.to_string(),
            "shopId": self.project_id,
            "comment": format!("VPN Subscription (Product: {})", session.product_id),
            "hookUrl": format!("https://{}/api/webhooks/payment/lava", self.api_domain),
            // `expire` is invoice lifetime in MINUTES (max 5 days) per
            // dev.lava.ru/api-invoice-create. 60 = 1 hour.
            "expire": 60
        });

        let body_str = serde_json::to_string(&json_body)?;

        // Подпись = HMAC-SHA256(body_str, key=secret_key)
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .context("Invalid Lava secret key")?;
        mac.update(body_str.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        let res = client
            .post("https://api.lava.ru/business/invoice/create")
            .header("Signature", &signature)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await
            .context("Failed to send request to Lava")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Lava API Error: {}", error_text);
        }

        let lava_res: LavaResponse = res.json().await.context("Failed to parse Lava response")?;
        if let Some(data) = lava_res.data {
            Ok(data.url)
        } else {
            anyhow::bail!(
                "Lava API missing invoice URL: {:?}",
                lava_res.error
            )
        }
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // Lava.ru Business API: вебхук подписывается так же, как исходящие
        // запросы — HMAC-SHA256(raw JSON body, key=secret_key), результат в
        // нижнем регистре hex. Подпись приходит в заголовке `Authorization`
        // (см. dev.lava.ru/business-webhook). Само значение заголовка
        // прокидывается сюда роутером вебхуков как `signature`.
        //
        // Сверка через `Mac::verify_slice` — это constant-time сравнение,
        // что защищает от тайминговых атак на подпись.
        let signature = signature.trim();
        if signature.is_empty() {
            return Ok(false);
        }

        // Провайдер отдаёт hex в нижнем регистре; нормализуем на случай,
        // если отправитель использует верхний регистр.
        let provided = match hex::decode(signature.to_ascii_lowercase()) {
            Ok(bytes) => bytes,
            // Невалидный hex => подпись заведомо неверна, не паникуем.
            Err(_) => return Ok(false),
        };

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .context("Invalid HMAC key length")?;
        mac.update(payload);

        Ok(mac.verify_slice(&provided).is_ok())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid Lava webhook JSON")?;

        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        // Вебхук Lava.ru Business API использует snake_case `order_id`
        // (dev.lava.ru/business-webhook). camelCase оставлен как запасной
        // вариант на случай различий в версиях API.
        let order_id = data
            .get("order_id")
            .or_else(|| data.get("orderId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "success" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "error" | "fail" | "expired" => Ok(PaymentWebhookAction::Failed {
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
