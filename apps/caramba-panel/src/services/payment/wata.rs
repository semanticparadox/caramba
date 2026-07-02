// WATA — платёжный провайдер для РФ (СБП, карты физлиц).
// API: https://api.wata.pro
// Аутентификация: Bearer JWT-токен в заголовке Authorization.
//
// Подпись вебхука (подтверждено docs.wata.pro и двумя независимыми боевыми
// интеграциями, см. github.com/snoups/remnashop и Fresh-Donate/backend):
//   * WATA подписывает RAW-тело запроса асимметрично: RSA с дайджестом SHA-512
//     и padding PKCS#1 v1.5 (это НЕ HMAC).
//   * Подпись приходит в заголовке `X-Signature`, закодирована в Base64.
//   * Проверка выполняется ПУБЛИЧНЫМ ключом WATA, который отдаётся по
//     GET https://api.wata.pro/api/h2h/public-key в виде JSON {"value":"<PEM>"}.
//   * Никакого операторского webhook_secret у WATA нет — поле `webhook_secret`
//     в структуре сохранено только для совместимости с местами конструирования
//     (их править нельзя) и больше не используется.
//
// Публичный ключ кешируется на уровне модуля (WATA ротирует его редко); при
// несовпадении подписи ключ обновляется один раз и проверка повторяется — это
// штатно переживает ротацию ключа на стороне WATA.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::Engine;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::sign::Verifier;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

/// Endpoint, отдающий публичный ключ для проверки подписи вебхуков.
const WATA_PUBLIC_KEY_URL: &str = "https://api.wata.pro/api/h2h/public-key";
/// Сколько кешируем публичный ключ, прежде чем перечитать его принудительно.
const PUBLIC_KEY_TTL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone)]
struct CachedPublicKey {
    pem: String,
    fetched_at: Instant,
}

/// Процессный кеш публичного ключа WATA. Живёт на уровне модуля, т.к. структуру
/// `WataProvider` нельзя расширять полями (она конструируется в файлах, которые
/// этот провайдер не имеет права редактировать).
fn public_key_cache() -> &'static Mutex<Option<CachedPublicKey>> {
    static CACHE: OnceLock<Mutex<Option<CachedPublicKey>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Ответ эндпоинта public-key: {"value": "-----BEGIN PUBLIC KEY-----..."}.
#[derive(Deserialize)]
struct WataPublicKeyRes {
    value: String,
}

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
    /// JWT API-токен из личного кабинета WATA.
    pub jwt_token: String,
    /// Историческое поле. У WATA нет операторского секрета вебхука — подпись
    /// асимметричная (RSA-SHA512) и проверяется публичным ключом WATA. Поле
    /// сохранено только ради совместимости с местами конструирования структуры.
    pub webhook_secret: String,
    /// Домен панели — используется для формирования callback_url в будущих версиях API.
    pub api_domain: String,
    pub bot_username: String,
}

impl WataProvider {
    /// Возвращает PEM публичного ключа WATA, используя процессный кеш.
    /// При `force_refresh = true` кеш игнорируется и ключ перечитывается.
    async fn fetch_public_key(&self, force_refresh: bool) -> Result<String> {
        let cache = public_key_cache();

        if !force_refresh {
            let guard = cache.lock().await;
            if let Some(cached) = guard.as_ref()
                && cached.fetched_at.elapsed() < PUBLIC_KEY_TTL
            {
                return Ok(cached.pem.clone());
            }
        }

        // Сетевой запрос делаем вне блокировки кеша, чтобы не держать lock на await.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .context("Не удалось создать HTTP-клиент для запроса ключа WATA")?;

        let res = client
            .get(WATA_PUBLIC_KEY_URL)
            .send()
            .await
            .context("Не удалось запросить публичный ключ WATA")?;

        if !res.status().is_success() {
            anyhow::bail!("WATA public-key endpoint вернул статус {}", res.status());
        }

        let body: WataPublicKeyRes = res
            .json()
            .await
            .context("Не удалось разобрать ответ public-key WATA")?;

        let pem = body.value.trim().to_string();
        if pem.is_empty() {
            anyhow::bail!("WATA public-key endpoint вернул пустой ключ");
        }

        let mut guard = cache.lock().await;
        *guard = Some(CachedPublicKey {
            pem: pem.clone(),
            fetched_at: Instant::now(),
        });

        Ok(pem)
    }

    /// Проверяет RSA-SHA512 (PKCS#1 v1.5) подпись `signature_bytes` над `payload`
    /// публичным ключом `pem`. Возвращает Ok(true)/Ok(false); Err — только если
    /// сам ключ не удалось разобрать.
    fn verify_rsa_sha512(pem: &str, payload: &[u8], signature_bytes: &[u8]) -> Result<bool> {
        let pkey = PKey::public_key_from_pem(pem.as_bytes())
            .context("Не удалось разобрать публичный ключ WATA (PEM)")?;

        let mut verifier = Verifier::new(MessageDigest::sha512(), &pkey)
            .context("Не удалось инициализировать RSA-SHA512 верификатор")?;
        verifier
            .update(payload)
            .context("Ошибка при подаче тела вебхука в верификатор")?;

        // verify() возвращает false при несовпадении подписи без ошибки.
        Ok(verifier.verify(signature_bytes).unwrap_or(false))
    }
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

        resp.url
            .ok_or_else(|| anyhow::anyhow!("WATA API не вернул ссылку для оплаты"))
    }

    /// Проверяет подпись вебхука WATA.
    ///
    /// `signature` — это значение заголовка `X-Signature` (Base64), которое
    /// маршрут вебхука уже извлёк из заголовков и передал сюда. WATA подписывает
    /// RAW-тело запроса (`payload`) асимметрично (RSA-SHA512), поэтому проверяем
    /// его публичным ключом WATA, а не HMAC-секретом.
    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        let signature = signature.trim();
        if signature.is_empty() {
            tracing::warn!("WATA webhook missing X-Signature header — rejecting");
            return Ok(false);
        }

        let signature_bytes = match base64::engine::general_purpose::STANDARD.decode(signature) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!("WATA webhook X-Signature is not valid base64: {}", e);
                return Ok(false);
            }
        };

        // Первая попытка — с кешированным ключом.
        let pem = self.fetch_public_key(false).await?;
        if Self::verify_rsa_sha512(&pem, payload, &signature_bytes)? {
            return Ok(true);
        }

        // Несовпадение может означать ротацию ключа на стороне WATA — обновляем
        // ключ принудительно и пробуем ещё раз.
        tracing::warn!("WATA webhook signature mismatch — refreshing public key and retrying");
        let pem = self.fetch_public_key(true).await?;
        if Self::verify_rsa_sha512(&pem, payload, &signature_bytes)? {
            return Ok(true);
        }

        tracing::warn!("WATA webhook signature invalid after public key refresh — rejecting");
        Ok(false)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке WATA")?;

        let status = data
            .get("transactionStatus")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let order_id = data.get("orderId").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        // Документированные значения transactionStatus у WATA — "Paid" (успех)
        // и "Declined" (отказ). Остальные написания приняты дополнительно,
        // защитно, на случай иных нотификаций.
        match status {
            "Paid" | "paid" | "success" | "Success" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "Declined" | "declined" | "Failed" | "failed" | "error" | "Error" | "Expired"
            | "expired" => Ok(PaymentWebhookAction::Failed {
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
