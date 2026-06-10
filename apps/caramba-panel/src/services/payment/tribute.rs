// Tribute — нативная монетизация для Telegram (Shop API).
// Docs: https://wiki.tribute.tg/for-shops/api  (OpenAPI: https://tribute.tg/api/v1/openapi/shop/en)
//
// Создание заказа: POST https://tribute.tg/api/v1/shop/orders
//   Аутентификация: заголовок `Api-Key: <api-ключ>` (НЕ Bearer).
//   Тело: { title, description, amount (целое, в МИНОРНЫХ единицах — центы/копейки),
//           currency (нижний регистр: eur|rub|usd), customerId (наш ref = session.id) }.
//   Ответ: полный объект заказа; ссылка на оплату в `paymentUrl`
//   (может быть null для OnlyStars) или `webappPaymentUrl`.
//
// Вебхуки: конверт { name, created_at, sent_at, payload }.
//   `name` — тип события в snake_case (shop_order, shop_order_payment_received, ...).
//   Идентификатор нашей сессии — в `payload.customerId`.
//   Подпись: заголовок `trbt-signature`, HMAC-SHA256 над raw body, ключ = API-ключ
//   (если задан отдельный tribute_webhook_secret — используется он).

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

type HmacSha256 = Hmac<Sha256>;

#[derive(Serialize)]
struct TributeOrderReq {
    title: String,
    description: String,
    /// Сумма в МИНОРНЫХ единицах (центы для EUR/USD, копейки для RUB).
    amount: i64,
    /// Код валюты в нижнем регистре: eur | rub | usd.
    currency: String,
    /// Наш идентификатор заказа — возвращается обратно в payload.customerId вебхука.
    #[serde(rename = "customerId")]
    customer_id: String,
}

#[derive(Deserialize)]
struct TributeOrderRes {
    #[serde(rename = "paymentUrl")]
    payment_url: Option<String>,
    #[serde(rename = "webappPaymentUrl")]
    webapp_payment_url: Option<String>,
}

#[allow(dead_code)]
pub struct TributeProvider {
    /// API-ключ из панели Tribute
    pub api_key: String,
    /// Секрет для верификации подписи вебхуков (если пуст — используется api_key)
    pub webhook_secret: String,
    /// Username бота — зарезервировано для будущих redirect URL.
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
        // session.amount уже хранится в минорных единицах — Tribute ждёт ровно это
        // (целое число центов/копеек), без деления на 100.
        let req_body = TributeOrderReq {
            title: format!("VPN Subscription (Product {})", session.product_id),
            description: format!("VPN Subscription (Product {})", session.product_id),
            amount: session.amount,
            currency: session.currency.to_lowercase(),
            customer_id: session.id.to_string(),
        };

        let res = client
            .post("https://tribute.tg/api/v1/shop/orders")
            .header("Api-Key", &self.api_key)
            .json(&req_body)
            .send()
            .await
            .context("Не удалось отправить запрос в Tribute API")?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Tribute API Error ({}): {}", status, error_text);
        }

        let resp: TributeOrderRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ Tribute")?;

        // paymentUrl — основная ссылка (браузерная); webappPaymentUrl — запасная
        // (оплата внутри Telegram, единственная для OnlyStars-заказов).
        resp.payment_url
            .filter(|u| !u.is_empty())
            .or(resp.webapp_payment_url.filter(|u| !u.is_empty()))
            .ok_or_else(|| anyhow::anyhow!("Tribute API не вернул ссылку для оплаты"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // Tribute подписывает тело вебхука в ЗАГОЛОВКЕ `trbt-signature` (HMAC-SHA256
        // над raw body). marketplace_service передаёт значение этого заголовка в
        // аргументе `signature`, поэтому используем именно его.
        //
        // Ключ подписи — API-ключ аккаунта. Допускаем явный override-секрет.
        let key = if !self.webhook_secret.is_empty() {
            self.webhook_secret.as_str()
        } else if !self.api_key.is_empty() {
            self.api_key.as_str()
        } else {
            anyhow::bail!(
                "ни tribute_webhook_secret, ни tribute_api_key не заданы — вебхук отклонён"
            );
        };

        // Декодируем переданную подпись из hex. Невалидный hex => подпись неверна.
        let provided = match hex::decode(signature.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(false),
        };

        let mut mac =
            HmacSha256::new_from_slice(key.as_bytes()).context("Неверный HMAC-ключ Tribute")?;
        mac.update(payload);

        // verify_slice выполняет сравнение в постоянном времени.
        Ok(mac.verify_slice(&provided).is_ok())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Tribute")?;

        // Конверт: { name, created_at, sent_at, payload }.
        let event = data.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let body = data.get("payload");

        // Идентификатор нашей сессии — наш customerId, переданный при создании заказа.
        let customer_id = body
            .and_then(|p| p.get("customerId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match event {
            // Финальное подтверждение оплаты (status всегда "paid" для этого события).
            "shop_order" => {
                if customer_id.is_empty() {
                    return Ok(PaymentWebhookAction::Ignored);
                }
                Ok(PaymentWebhookAction::Completed {
                    external_id: customer_id.to_string(),
                })
            }
            // Промежуточный сигнал — оплата получена, но средства ещё не зачислены.
            // Документация прямо предписывает НЕ считать заказ оплаченным.
            "shop_order_payment_received" => Ok(PaymentWebhookAction::Pending),
            // Неуспешная оплата / неуспешное списание по подписке.
            "shop_order_payment_failed" | "shop_order_charge_failed" => {
                Ok(PaymentWebhookAction::Failed {
                    reason: event.to_string(),
                })
            }
            // Отмена/возврат/успешное продление подписки — пост-фулфилмент события
            // жизненного цикла, не относящиеся к этой разовой оплате. Игнорируем,
            // чтобы не логировать ложную «ошибку» по уже оплаченному заказу.
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
