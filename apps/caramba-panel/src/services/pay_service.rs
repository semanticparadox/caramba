use crate::bot_manager::BotManager;
use crate::services::activity_service::ActivityService;
use crate::services::payment::{PaymentAdapter, cryptomus::CryptomusAdapter};
use crate::services::store_service::StoreService;
use anyhow::Result;
use anyhow::anyhow;
use caramba_db::models::payment::PaymentType;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct CryptoBotInvoice {
    pub asset: String,
    pub amount: String,
    pub description: Option<String>,
    pub payload: Option<String>,
    pub paid_btn_name: Option<String>,
    pub paid_btn_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct CryptoBotResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct CreateInvoiceResult {
    pub invoice_id: i64,
    pub bot_invoice_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct NowPaymentInvoice {
    pub price_amount: f64,
    pub price_currency: String,
    pub pay_currency: String,
    pub ipn_callback_url: String,
    pub order_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct NowPaymentResponse {
    pub payment_id: String,
    pub invoice_url: String,
}

pub struct PayService {
    pool: PgPool,
    #[allow(dead_code)]
    store_service: Arc<StoreService>,
    catalog_service: Arc<crate::services::catalog_service::CatalogService>,
    bot_manager: Arc<BotManager>,
    // Единственный HTTP-клиент на весь сервис — переиспользует connection pool
    http_client: reqwest::Client,
    bot_token: String,
    cryptobot_token: String,
    nowpayments_key: String,
    crystalpay_login: String,
    crystalpay_secret: String,
    stripe_secret_key: String,
    aaio_merchant_id: String,
    aaio_secret_1: String,
    aaio_secret_2: String,
    lava_project_id: String,
    lava_secret_key: String,
    is_testnet: bool,
    api_domain: String,
    adapters: HashMap<String, Box<dyn PaymentAdapter>>,
}

impl PayService {
    pub fn new(
        pool: PgPool,
        store_service: Arc<StoreService>,
        catalog_service: Arc<crate::services::catalog_service::CatalogService>,
        bot_manager: Arc<BotManager>,
        bot_token: String,
        cryptobot_token: String,
        nowpayments_key: String,
        crystalpay_login: String,
        crystalpay_secret: String,
        stripe_secret_key: String,
        cryptomus_merchant_id: String,
        cryptomus_payment_api_key: String,
        aaio_merchant_id: String,
        aaio_secret_1: String,
        aaio_secret_2: String,
        lava_project_id: String,
        lava_secret_key: String,
        is_testnet: bool,
        api_domain: String,
    ) -> Self {
        let mut adapters: HashMap<String, Box<dyn PaymentAdapter>> = HashMap::new();

        if !cryptomus_merchant_id.is_empty() && !cryptomus_payment_api_key.is_empty() {
            adapters.insert(
                "cryptomus".to_string(),
                Box::new(CryptomusAdapter::new(
                    cryptomus_merchant_id.clone(),
                    cryptomus_payment_api_key.clone(),
                )),
            );
        }

        // Создаём один клиент для всего сервиса с разумными таймаутами
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client for PayService");

        Self {
            pool,
            store_service,
            catalog_service,
            bot_manager,
            http_client,
            bot_token,
            cryptobot_token,
            nowpayments_key,
            crystalpay_login,
            crystalpay_secret,
            stripe_secret_key,
            aaio_merchant_id,
            aaio_secret_1,
            aaio_secret_2,
            lava_project_id,
            lava_secret_key,
            is_testnet,
            api_domain,
            adapters,
        }
    }

    fn verify_cryptobot_signature(&self, payload: &str, signature: Option<&str>) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow!("Missing signature header"))?;

        // Правильная схема: HMAC-SHA256(payload, key=SHA256(token))
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(self.cryptobot_token.as_bytes());
        let secret_hash = hasher.finalize();

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(&secret_hash)
            .map_err(|e| anyhow!("Invalid HMAC key: {}", e))?;
        mac.update(payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        if sig == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid CryptoBot signature"))
        }
    }

    fn verify_nowpayments_signature(&self, payload: &str, signature: Option<&str>) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow!("Missing x-nowpayments-sig header"))?;

        use hmac::{Hmac, Mac};
        type HmacSha512 = Hmac<sha2::Sha512>;

        let mut mac = HmacSha512::new_from_slice(self.nowpayments_key.as_bytes())
            .map_err(|e| anyhow!("Invalid HMAC key: {}", e))?;
        mac.update(payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        if sig == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid NOWPayments signature"))
        }
    }

    fn verify_crystalpay_signature(&self, payload: &serde_json::Value) -> Result<()> {
        let sign_from_callback = payload
            .get("signature")
            .and_then(|s| s.as_str())
            .ok_or_else(|| anyhow!("Missing signature in payload"))?;

        let id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let state = payload.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let amount = payload
            .get("amount")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let data = format!("{}{}{}{}", id, amount, state, self.crystalpay_secret);
        let expected = format!("{:x}", md5::compute(data.as_bytes()));

        if sign_from_callback == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid CrystalPay signature"))
        }
    }

    fn verify_stripe_signature(
        &self,
        payload: &str,
        signature: Option<&str>,
        webhook_secret: &str,
    ) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow!("Missing Stripe-Signature header"))?;

        let parts: Vec<&str> = sig.split(',').collect();
        let mut timestamp = "";
        let mut sig_v1 = "";

        for p in parts {
            if let Some(val) = p.strip_prefix("t=") {
                timestamp = val;
            } else if let Some(val) = p.strip_prefix("v1=") {
                sig_v1 = val;
            }
        }

        if timestamp.is_empty() {
            return Err(anyhow!("Missing timestamp in signature"));
        }
        if sig_v1.is_empty() {
            return Err(anyhow!("Missing v1 signature"));
        }

        // Защита от replay-атак: отклоняем вебхуки старше 5 минут
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts: u64 = timestamp
            .parse()
            .map_err(|_| anyhow!("Invalid timestamp in Stripe signature"))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow!("System clock error: {}", e))?
            .as_secs();
        if now.saturating_sub(ts) > 300 {
            return Err(anyhow!("Stripe webhook timestamp too old"));
        }

        use hmac::{Hmac, Mac};
        type HmacSha256 = Hmac<sha2::Sha256>;

        let signed_payload = format!("{}.{}", timestamp, payload);
        let mut mac = HmacSha256::new_from_slice(webhook_secret.as_bytes())
            .map_err(|e| anyhow!("Invalid HMAC key: {}", e))?;
        mac.update(signed_payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        if sig_v1 == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid Stripe signature"))
        }
    }

    fn verify_aaio_signature(
        &self,
        merchant_id: &str,
        amount: &str,
        currency: &str,
        order_id: &str,
        sign: &str,
    ) -> Result<()> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let data = format!(
            "{}:{}:{}:{}:{}",
            merchant_id, amount, currency, self.aaio_secret_2, order_id
        );
        hasher.update(data.as_bytes());
        let expected = hex::encode(hasher.finalize());

        if sign == expected {
            Ok(())
        } else {
            Err(anyhow!(
                "Invalid Aaio signature. Expected: {}, Got: {}",
                expected,
                sign
            ))
        }
    }

    fn get_cryptobot_url(&self) -> &str {
        if self.is_testnet {
            "https://testnet-pay.crypt.bot/api"
        } else {
            "https://pay.crypt.bot/api"
        }
    }

    pub async fn get_bot_username(&self) -> String {
        self.bot_manager
            .get_username()
            .await
            .unwrap_or_else(|| "YOUR_BOT_USERNAME".to_string())
    }

    pub async fn create_cryptobot_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating CryptoPay invoice for user {}: ${} ({:?})",
            user_id, amount_usd, payment_type
        );

        let payload = payment_type.to_payload_string(user_id);

        let invoice = serde_json::json!({
             "asset": "USDT",
             "amount": format!("{:.2}", amount_usd),
             "description": "CARAMBA Top-up",
             "payload": payload,
             "allow_anonymous": false,
             "allow_comments": false
        });

        let client = self.http_client.clone();
        let resp = client
            .post(format!("{}/createInvoice", self.get_cryptobot_url()))
            .header("Crypto-Pay-API-Token", &self.cryptobot_token)
            .json(&invoice)
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        if body["ok"].as_bool().unwrap_or(false) {
            Ok(body["result"]["bot_invoice_url"]
                .as_str()
                .unwrap_or("")
                .to_string())
        } else {
            Err(anyhow::anyhow!("CryptoBot error: {:?}", body))
        }
    }

    pub async fn create_nowpayments_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating NOWPayments invoice for user {}: ${}",
            user_id, amount_usd
        );

        let payload_base = payment_type.to_payload_string(user_id);
        let unique_order_id = format!("{}_{}", payload_base, Utc::now().timestamp());

        let invoice = serde_json::json!({
            "price_amount": amount_usd,
            "price_currency": "usd",
            "pay_currency": "usdttrc20",
            "order_id": unique_order_id,
            "ipn_callback_url": format!(
                "https://{}/caramba-api/payments/nowpayments",
                self.api_domain
            ),
            "success_url": format!("https://t.me/{}", self.get_bot_username().await),
            "cancel_url": format!("https://t.me/{}", self.get_bot_username().await)
        });

        let client = self.http_client.clone();
        let resp = client
            .post("https://api.nowpayments.io/v1/invoice")
            .header("x-api-key", &self.nowpayments_key)
            .json(&invoice)
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        if let Some(url) = body["invoice_url"].as_str() {
            Ok(url.to_string())
        } else {
            Err(anyhow::anyhow!("NOWPayments error: {:?}", body))
        }
    }

    pub async fn create_crystalpay_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating CrystalPay invoice for user {}: ${}",
            user_id, amount_usd
        );

        let payload = payment_type.to_payload_string(user_id);

        let body_json = serde_json::json!({
            "auth_login": self.crystalpay_login,
            "auth_secret": self.crystalpay_secret,
            "amount": amount_usd,
            "amount_currency": "USD",
            "type": "purchase",
            "description": format!("CARAMBA User {}", user_id),
            "redirect_url": format!("https://t.me/{}", self.get_bot_username().await),
            "callback_url": format!(
                "https://{}/caramba-api/payments/crystalpay",
                self.api_domain
            ),
            "extra": payload
        });

        let client = self.http_client.clone();
        let resp = client
            .post("https://api.crystalpay.io/v2/invoice/create/")
            .json(&body_json)
            .send()
            .await?;

        let resp_json: serde_json::Value = resp.json().await?;

        if resp_json["error"].as_bool().unwrap_or(true) {
            Err(anyhow::anyhow!("CrystalPay Error: {:?}", resp_json))
        } else {
            Ok(resp_json["data"]["url"].as_str().unwrap_or("").to_string())
        }
    }

    pub async fn create_stripe_session(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating Stripe Session for user {}: ${}",
            user_id, amount_usd
        );

        let payload = payment_type.to_payload_string(user_id);
        let amount_cents = (amount_usd * 100.0) as i64;

        let client = self.http_client.clone();
        let params = [
            ("mode", "payment"),
            (
                "success_url",
                &format!("https://t.me/{}", self.get_bot_username().await),
            ),
            (
                "cancel_url",
                &format!("https://t.me/{}", self.get_bot_username().await),
            ),
            ("client_reference_id", &payload),
            ("line_items[0][price_data][currency]", "usd"),
            (
                "line_items[0][price_data][product_data][name]",
                "Balance Top-up",
            ),
            (
                "line_items[0][price_data][unit_amount]",
                &amount_cents.to_string(),
            ),
            ("line_items[0][quantity]", "1"),
        ];

        let resp: reqwest::Response = client
            .post("https://api.stripe.com/v1/checkout/sessions")
            .basic_auth(&self.stripe_secret_key, None::<&str>)
            .form(&params)
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        if let Some(url) = body["url"].as_str() {
            Ok(url.to_string())
        } else {
            Err(anyhow::anyhow!("Stripe Error: {:?}", body))
        }
    }

    pub async fn create_cryptomus_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        if let Some(adapter) = self.adapters.get("cryptomus") {
            info!("Redirecting to {} adapter", adapter.name());
            let bot_user = self.get_bot_username().await;
            adapter
                .create_invoice(
                    user_id,
                    amount_usd,
                    payment_type,
                    &bot_user,
                    &self.api_domain,
                )
                .await
        } else {
            Err(anyhow::anyhow!("Cryptomus adapter not initialized"))
        }
    }

    pub async fn create_aaio_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating Aaio invoice for user {}: ${}",
            user_id, amount_usd
        );

        let pay_desc = format!("Payment for User {}", user_id);
        let order_id = format!(
            "{}:{}:{}",
            user_id,
            Utc::now().timestamp(),
            payment_type.to_payload_string(user_id)
        );
        let currency = "USD";
        let amount_str = format!("{:.2}", amount_usd);

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let sign_data = format!(
            "{}:{}:{}:{}:{}",
            self.aaio_merchant_id, amount_str, currency, self.aaio_secret_1, order_id
        );
        hasher.update(sign_data.as_bytes());
        let sign = hex::encode(hasher.finalize());

        let client = self.http_client.clone();
        let params = [
            ("merchant_id", self.aaio_merchant_id.as_str()),
            ("amount", amount_str.as_str()),
            ("currency", currency),
            ("order_id", order_id.as_str()),
            ("sign", sign.as_str()),
            ("desc", pay_desc.as_str()),
            ("lang", "en"),
        ];

        let resp: reqwest::Response = client
            .post("https://aaio.so/merchant/get_pay_url")
            .form(&params)
            .send()
            .await?;

        if let Ok(url_str) = resp.text().await {
            if url_str.starts_with("http") {
                return Ok(url_str);
            }
            return Ok(url_str);
        }

        Err(anyhow::anyhow!("Aaio Error"))
    }

    pub async fn create_lava_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating Lava.top invoice for user {}: ${}",
            user_id, amount_usd
        );

        let order_id = format!("LAVA-{}-{}", user_id, Utc::now().timestamp());
        let payload_str = payment_type.to_payload_string(user_id);

        let json_body = serde_json::json!({
            "sum": amount_usd,
            "orderId": order_id,
            "shopId": self.lava_project_id,
            "comment": format!("Payment for User {}", user_id),
            "customFields": payload_str,
            "expire": 3600
        });

        let body_str = serde_json::to_string(&json_body)?;

        use hmac::{Hmac, Mac};
        type HmacSha256 = Hmac<sha2::Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.lava_secret_key.as_bytes())
            .map_err(|e| anyhow!("Invalid Lava Secret: {}", e))?;
        mac.update(body_str.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        let client = self.http_client.clone();
        let res = client
            .post("https://api.lava.ru/business/invoice/create")
            .header("Signature", signature)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?;

        #[derive(Deserialize)]
        struct LavaResponse {
            data: Option<LavaData>,
            error: Option<serde_json::Value>,
        }
        #[derive(Deserialize)]
        struct LavaData {
            url: String,
            _id: String,
        }

        let lava_res: LavaResponse = res.json().await?;

        if let Some(data) = lava_res.data {
            Ok(data.url)
        } else {
            Err(anyhow!(
                "Failed to create Lava invoice (No URL returned): {:?}",
                lava_res.error
            ))
        }
    }

    pub async fn create_stars_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
    ) -> Result<String> {
        info!(
            "Creating Telegram Stars invoice for user {}: ${}",
            user_id, amount_usd
        );

        let stars_amount = (amount_usd * 50.0).ceil() as i64;
        let payload = payment_type.to_payload_string(user_id);

        let client = self.http_client.clone();
        let bot_token = self.bot_token.clone();
        if bot_token.is_empty() {
            return Err(anyhow!("Bot token required for Stars"));
        }

        let url = format!(
            "https://api.telegram.org/bot{}/createInvoiceLink",
            bot_token
        );

        let params = serde_json::json!({
            "title": "Balance Top-up",
            "description": format!("Top-up balance by ${:.2}", amount_usd),
            "payload": payload,
            "provider_token": "",
            "currency": "XTR",
            "prices": [{"label": "Top-up", "amount": stars_amount}]
        });

        let res = client.post(&url).json(&params).send().await?;

        #[derive(Deserialize)]
        struct TgResponse {
            ok: bool,
            result: Option<String>,
            description: Option<String>,
        }

        let tg_res: TgResponse = res.json().await?;

        if tg_res.ok {
            tg_res.result.ok_or_else(|| anyhow!(
                "Failed to create Stars invoice (ok=true but no result): {:?}",
                tg_res.description
            ))
        } else {
            Err(anyhow!(
                "Failed to create Stars invoice: {:?}",
                tg_res.description
            ))
        }
    }

    pub async fn handle_webhook(
        &self,
        source: &str,
        payload: &str,
        crypto_sig: Option<&str>,
        nowpayments_sig: Option<&str>,
        stripe_sig: Option<&str>,
        cryptomus_sig: Option<&str>,
    ) -> Result<()> {
        let body: serde_json::Value = serde_json::from_str(payload)?;

        match source {
            "cryptobot" => {
                self.verify_cryptobot_signature(payload, crypto_sig)?;

                if let Some(update_type) = body["update_type"].as_str() {
                    if update_type == "invoice_paid" {
                        let invoice = &body["update_payload"];
                        let status = invoice["status"].as_str().unwrap_or("");
                        if status == "paid" {
                            let amount: f64 = invoice["amount"]
                                .as_str()
                                .unwrap_or("0")
                                .parse()
                                .unwrap_or(0.0);
                            let payload_str = invoice["payload"].as_str().unwrap_or("");
                            let id = invoice["invoice_id"].to_string();
                            self.process_any_payment(amount, "cryptobot", Some(id), payload_str)
                                .await?;
                        } else if status == "expired" || status == "cancelled" {
                            // CryptoBot invoice expired or was cancelled — уведомляем пользователя
                            let amount: f64 = invoice["amount"]
                                .as_str()
                                .unwrap_or("0")
                                .parse()
                                .unwrap_or(0.0);
                            let payload_str = invoice["payload"].as_str().unwrap_or("");
                            self.notify_payment_declined(payload_str, amount, "CryptoBot").await;
                        }
                    }
                }
            }
            "nowpayments" => {
                self.verify_nowpayments_signature(payload, nowpayments_sig)?;

                if let Some(status) = body["payment_status"].as_str() {
                    if status == "finished" {
                        let amount: f64 = body["pay_amount"].as_f64().unwrap_or(0.0);
                        let order_id = body["order_id"].as_str().unwrap_or("");
                        let payload_str = order_id.split('_').next().unwrap_or("");
                        let id = body["payment_id"].to_string();
                        self.process_any_payment(amount, "nowpayments", Some(id), payload_str)
                            .await?;
                    } else if status == "failed" || status == "expired" || status == "refunded" {
                        // NOWPayments terminal failure — уведомляем пользователя
                        let amount: f64 = body["pay_amount"].as_f64().unwrap_or(0.0);
                        let order_id = body["order_id"].as_str().unwrap_or("");
                        let payload_str = order_id.split('_').next().unwrap_or("");
                        self.notify_payment_declined(payload_str, amount, "NOWPayments").await;
                    }
                }
            }
            "crystalpay" => {
                self.verify_crystalpay_signature(&body)?;

                if body["type"].as_str().unwrap_or("") == "payment" {
                    let state = body["state"].as_str().unwrap_or("");
                    if state == "payed" {
                        let amount: f64 = body["amount"].as_f64().unwrap_or(0.0);
                        let extra = body["extra"].as_str().unwrap_or("");
                        let id = body["id"].to_string();
                        self.process_any_payment(amount, "crystalpay", Some(id), extra)
                            .await?;
                    } else if state == "cancelled" || state == "expired" || state == "fail" {
                        // CrystalPay payment failed — уведомляем пользователя
                        let amount: f64 = body["amount"].as_f64().unwrap_or(0.0);
                        let extra = body["extra"].as_str().unwrap_or("");
                        self.notify_payment_declined(extra, amount, "CrystalPay").await;
                    }
                }
            }
            "stripe" => {
                let webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();

                self.verify_stripe_signature(payload, stripe_sig, &webhook_secret)?;

                let event_type = body["type"].as_str().unwrap_or("");
                if event_type == "checkout.session.completed" {
                    let session = &body["data"]["object"];
                    let amount_subtokens = session["amount_total"].as_i64().unwrap_or(0);
                    let amount_usd = amount_subtokens as f64 / 100.0;
                    let payload_str = session["client_reference_id"].as_str().unwrap_or("");
                    let id = session["id"].to_string();
                    self.process_any_payment(amount_usd, "stripe", Some(id), payload_str)
                        .await?;
                } else if event_type == "checkout.session.expired"
                    || event_type == "payment_intent.payment_failed"
                {
                    // Stripe: сессия истекла или платёж отклонён — уведомляем пользователя
                    let session = &body["data"]["object"];
                    let amount_subtokens = session["amount_total"].as_i64().unwrap_or(0);
                    let amount_usd = amount_subtokens as f64 / 100.0;
                    let payload_str = session["client_reference_id"].as_str().unwrap_or("");
                    self.notify_payment_declined(payload_str, amount_usd, "Stripe").await;
                }
            }

            "cryptomus" => {
                if let Some(adapter) = self.adapters.get("cryptomus") {
                    adapter.verify_signature(payload, cryptomus_sig)?;
                } else {
                    return Err(anyhow::anyhow!("Cryptomus adapter not initialized"));
                }

                let status = body["status"].as_str().unwrap_or("");
                if status == "paid" || status == "paid_over" {
                    let amount: f64 = body["amount"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0);
                    let payload_str = body["additional_data"].as_str().unwrap_or("");
                    let id = body["uuid"].as_str().unwrap_or("").to_string();

                    self.process_any_payment(amount, "cryptomus", Some(id), payload_str)
                        .await?;
                } else if status == "cancel" || status == "fail" || status == "system_fail" || status == "wrong_amount_waiting" {
                    // Cryptomus terminal failure — уведомляем пользователя
                    let amount: f64 = body["amount"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0);
                    let payload_str = body["additional_data"].as_str().unwrap_or("");
                    self.notify_payment_declined(payload_str, amount, "Cryptomus").await;
                }
            }
            "aaio" => {
                let data: serde_json::Value = if let Ok(v) = serde_json::from_str(payload) {
                    v
                } else {
                    let parsed: std::collections::HashMap<String, String> =
                        serde_urlencoded::from_str(payload).unwrap_or_default();
                    serde_json::to_value(parsed).unwrap_or(serde_json::json!({}))
                };

                let merchant_id = data["merchant_id"].as_str().unwrap_or("");
                let amount = data["amount"].as_str().unwrap_or("");
                let currency = data["currency"].as_str().unwrap_or("");
                let order_id = data["order_id"].as_str().unwrap_or("");
                let sign = data["sign"].as_str().unwrap_or("");

                self.verify_aaio_signature(merchant_id, amount, currency, order_id, sign)?;

                let parts: Vec<&str> = order_id.splitn(3, ':').collect();
                if parts.len() == 3 {
                    let amount_val: f64 = amount.parse().unwrap_or(0.0);
                    let payload_str = parts[2];
                    let id = data["invoice_id"].as_str().unwrap_or(order_id).to_string();
                    self.process_any_payment(amount_val, "aaio", Some(id), payload_str)
                        .await?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Уведомляет пользователя об отклонённом платеже.
    ///
    /// `payload_str` — строка вида `user_id:type:target_id` или просто `user_id`.
    /// `amount_usd`  — сумма в долларах.
    /// `provider`    — человекочитаемое название провайдера (например, "Stripe").
    ///
    /// Метод не возвращает ошибку: сбой уведомления не должен влиять на обработку вебхука.
    async fn notify_payment_declined(&self, payload_str: &str, amount_usd: f64, provider: &str) {
        // Извлекаем user_id из строки payload — он всегда идёт первым числом
        let user_db_id: i64 = payload_str
            .split(':')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if user_db_id == 0 {
            return;
        }

        let row = sqlx::query_as::<_, (i64, Option<String>)>(
            "SELECT tg_id, language_code FROM users WHERE id = $1",
        )
        .bind(user_db_id)
        .fetch_optional(&self.pool)
        .await;

        if let Ok(Some((tg_id, lang))) = row {
            let is_ru = lang.as_deref().map_or(true, |l| l.starts_with("ru"));
            let amount_str = format!("{:.2}", amount_usd);

            let msg = if is_ru {
                format!(
                    "❌ *Платёж отклонён*\n\n\
                     Платёж на сумму *${amount_str}* через *{provider}* не прошёл\\.\n\n\
                     Попробуйте другой способ оплаты или обратитесь в поддержку\\."
                )
            } else {
                format!(
                    "❌ *Payment Declined*\n\n\
                     Your payment of *${amount_str}* via *{provider}* was declined\\.\n\n\
                     Please try a different payment method or contact support\\."
                )
            };

            let _ = self.bot_manager.send_notification(tg_id, &msg).await;

            info!(
                "Sent payment declined notification to user {} (tg_id={}) for ${:.2} via {}",
                user_db_id, tg_id, amount_usd, provider
            );
        }
    }

    pub async fn process_any_payment(
        &self,
        amount_usd: f64,
        method: &str,
        external_id: Option<String>,
        payload: &str,
    ) -> Result<()> {
        // Idempotency check: skip if this (method, external_id) was already processed
        if let Some(ref ext_id) = external_id {
            let already = sqlx::query_scalar::<_, i64>(
                "SELECT id FROM payments WHERE method = $1 AND external_id = $2 LIMIT 1",
            )
            .bind(method)
            .bind(ext_id)
            .fetch_optional(&self.pool)
            .await?;

            if already.is_some() {
                info!(
                    "Duplicate webhook ignored: method={}, external_id={}",
                    method, ext_id
                );
                let _ = ActivityService::log(
                    &self.pool,
                    "Payment:Duplicate",
                    &format!("Ignored duplicate {} / {}", method, ext_id),
                )
                .await;
                return Ok(());
            }
        }

        let parts: Vec<&str> = payload.split(':').collect();
        if parts.len() < 3 {
            if let Ok(user_id) = payload.parse::<i64>() {
                return self
                    .process_balance_topup(user_id, amount_usd, method, external_id)
                    .await;
            }
            return Err(anyhow::anyhow!("Invalid payload: {}", payload));
        }

        let user_id: i64 = parts[0].parse().unwrap_or(0);
        let type_code = parts[1];
        let target_id: i64 = parts[2].parse().unwrap_or(0);

        if user_id == 0 {
            return Err(anyhow::anyhow!("Zero User ID"));
        }

        match type_code {
            "bal" => {
                self.process_balance_topup(user_id, amount_usd, method, external_id)
                    .await
            }
            "ord" => {
                self.process_order_purchase(user_id, target_id, amount_usd, method, external_id)
                    .await
            }
            "sub" => {
                self.process_subscription_purchase(
                    user_id,
                    target_id,
                    amount_usd,
                    method,
                    external_id,
                )
                .await
            }
            _ => Err(anyhow::anyhow!("Unknown Type: {}", type_code)),
        }
    }

    async fn process_order_purchase(
        &self,
        user_id: i64,
        order_id: i64,
        amount_usd: f64,
        method: &str,
        external_id: Option<String>,
    ) -> Result<()> {
        info!(
            "Processing ORDER payment #${} for user {}",
            order_id, user_id
        );
        let amount_units = (amount_usd * 100.0) as i64;
        self.store_service
            .log_payment(
                user_id,
                method,
                amount_units,
                external_id.as_deref(),
                "paid",
            )
            .await?;
        self.catalog_service.process_order_payment(order_id).await?;

        // Получаем tg_id пользователя — bot_manager принимает tg_id, не DB id
        let tg_id: Option<i64> =
            sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);

        if let Some(tg_id) = tg_id {
            let _ = self
                .bot_manager
                .send_notification(tg_id, "✅ Your order has been paid successfully!")
                .await;
        }

        let _ = crate::services::analytics_service::AnalyticsService::track_revenue(
            &self.store_service.get_pool(),
            amount_units,
        )
        .await;

        let _ = ActivityService::log(
            &self.pool,
            "Order",
            &format!("Order #{} paid ${:.2} via {} for user {}", order_id, amount_usd, method, user_id),
        )
        .await;

        Ok(())
    }

    async fn process_subscription_purchase(
        &self,
        user_id: i64,
        plan_id: i64,
        amount_usd: f64,
        method: &str,
        external_id: Option<String>,
    ) -> Result<()> {
        info!(
            "Processing SUBSCRIPTION payment for user {} (Plan: {})",
            user_id, plan_id
        );

        self.process_balance_topup(user_id, amount_usd, method, external_id.clone())
            .await?;

        let durations = sqlx::query_as::<_, caramba_db::models::store::PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = $1 ORDER BY duration_days ASC LIMIT 1",
        )
        .bind(plan_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(duration) = durations {
            match self.store_service.purchase_plan(user_id, duration.id, false).await {
                Ok(crate::services::store_service::PurchaseResult::Subscription(_)) => {
                    let _ = self
                        .bot_manager
                        .send_notification(user_id, "✅ Subscription activated successfully!")
                        .await;
                }
                Ok(crate::services::store_service::PurchaseResult::GiftCode(_)) => {
                    // Не должно случаться при as_gift=false, игнорируем
                }
                Err(e) => {
                    error!("Failed to auto-purchase subscription after payment: {}", e);
                    let _ = self.bot_manager.send_notification(user_id, "⚠️ Payment received but subscription activation failed. Please contact support.").await;
                }
            }
        } else {
            error!("No duration found for plan {}", plan_id);
            let _ = self
                .bot_manager
                .send_notification(
                    user_id,
                    "⚠️ Error: Plan duration not found. Balance credited.",
                )
                .await;
        }

        Ok(())
    }

    async fn process_balance_topup(
        &self,
        user_id: i64,
        amount_usd: f64,
        method: &str,
        external_id: Option<String>,
    ) -> Result<()> {
        info!(
            "Processing BALANCE top-up of ${} for user {} via {}",
            amount_usd, user_id, method
        );
        let amount_units = (amount_usd * 100.0) as i64;

        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(amount_units)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let payment_id: i64 = match sqlx::query_scalar(
            "INSERT INTO payments (user_id, method, amount, external_id, status) VALUES ($1, $2, $3, $4, 'paid') RETURNING id"
        )
            .bind(user_id).bind(method).bind(amount_units).bind(&external_id).fetch_one(&mut *tx).await {
            Ok(id) => id,
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                info!("Duplicate payment insert caught by DB constraint: method={}, external_id={:?}", method, external_id);
                tx.rollback().await?;
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        if let Some((referrer_tg_id, bonus)) = self
            .store_service
            .apply_referral_bonus(&mut tx, user_id, amount_units, Some(payment_id))
            .await?
        {
            let formatted_bonus = format!("{:.2}", bonus as f64 / 100.0);
            let msg = format!(
                "🎉 *Referral Bonus* from your invited user!\n+${}",
                formatted_bonus
            );
            let _ = self
                .bot_manager
                .send_notification(referrer_tg_id, &msg)
                .await;
        }

        tx.commit().await?;

        let _ = self
            .bot_manager
            .send_notification(
                user_id,
                &format!("✅ Balance topped up: +${:.2}", amount_usd),
            )
            .await;
        let _ = crate::services::analytics_service::AnalyticsService::track_revenue(
            &self.pool,
            amount_units,
        )
        .await;

        let _ = ActivityService::log(
            &self.pool,
            "Payment",
            &format!(
                "Balance +${:.2} for user {} via {}, ext_id={:?}",
                amount_usd, user_id, method, external_id
            ),
        )
        .await;

        // Admin notification
        let username: String = sqlx::query_scalar("SELECT COALESCE(username, tg_id::TEXT) FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| user_id.to_string());
        self.bot_manager
            .notify_admins(
                &self.pool,
                &format!("💰 Payment ${:.2} from {} via {}", amount_usd, username, method),
            )
            .await;

        Ok(())
    }
}
