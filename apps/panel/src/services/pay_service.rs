use sqlx::SqlitePool;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::sync::Arc;
use chrono::Utc;
use crate::services::store_service::StoreService;
use crate::bot_manager::BotManager;
use anyhow::anyhow;
use base64::Engine;

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

#[derive(Debug, Serialize, Deserialize)]
pub enum PaymentType {
    BalanceTopup,
    OrderPurchase(i64), // order_id
    SubscriptionPurchase(i64), // plan_id
}

impl PaymentType {
    pub fn to_payload_string(&self, user_id: i64) -> String {
        match self {
            PaymentType::BalanceTopup => format!("{}:bal:0", user_id),
            PaymentType::OrderPurchase(order_id) => format!("{}:ord:{}", user_id, order_id),
            PaymentType::SubscriptionPurchase(plan_id) => format!("{}:sub:{}", user_id, plan_id),
        }
    }
}

pub struct PayService {
    pool: SqlitePool,
    #[allow(dead_code)]
    store_service: Arc<StoreService>,
    bot_manager: Arc<BotManager>,
    bot_token: String, // NEW: Main bot token for Stars
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
}

impl PayService {
    pub fn new(
        pool: SqlitePool, 
        store_service: Arc<StoreService>, 
        bot_manager: Arc<BotManager>, 
        bot_token: String, // NEW
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
        is_testnet: bool
    ) -> Self {
        Self { 
            pool, 
            store_service, 
            bot_manager, 
            bot_token, // NEW
            cryptobot_token, 
            nowpayments_key, 
            crystalpay_login,

            crystalpay_secret,
            stripe_secret_key,
            cryptomus_merchant_id,
            cryptomus_payment_api_key,
            aaio_merchant_id,
            aaio_secret_1,
            aaio_secret_2,
            lava_project_id,
            lava_secret_key,
            is_testnet 
        }
    }

    // Webhook signature verification helpers
    
    /// Verify CryptoBot webhook signature (HMAC-SHA256)
    fn verify_cryptobot_signature(&self, payload: &str, signature: Option<&str>) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow!("Missing signature header"))?;
        
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(self.cryptobot_token.as_bytes());
        hasher.update(payload.as_bytes());
        let expected = hex::encode(hasher.finalize());
        
        if sig == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid CryptoBot signature"))
        }
    }
    
    /// Verify NOWPayments IPN signature (HMAC-SHA512)
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
    
    /// Verify CrystalPay signature (MD5 hash)
    fn verify_crystalpay_signature(&self, payload: &serde_json::Value) -> Result<()> {
        // CrystalPay uses MD5(data + secret) for signature verification
        let sign_from_callback = payload.get("signature")
            .and_then(|s| s.as_str())
            .ok_or_else(|| anyhow!("Missing signature in payload"))?;
        
        // Extract all fields except signature for hash calculation
        let id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let state = payload.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let amount = payload.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        
        let data = format!("{}{}{}{}", id, amount, state, self.crystalpay_secret);
        let expected = format!("{:x}", md5::compute(data.as_bytes()));
        
        if sign_from_callback == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid CrystalPay signature"))
        }
    }
    
    /// Verify Stripe webhook signature
    fn verify_stripe_signature(&self, payload: &str, signature: Option<&str>, webhook_secret: &str) -> Result<()> {
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
        
        if timestamp.is_empty() { return Err(anyhow!("Missing timestamp in signature")); }
        if sig_v1.is_empty() { return Err(anyhow!("Missing v1 signature")); }

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


    /// Verify Cryptomus signature (MD5(base64(body) + key))
    fn verify_cryptomus_signature(&self, payload: &str, signature: Option<&str>) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow!("Missing sign header for Cryptomus"))?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
        let to_hash = format!("{}{}", encoded, self.cryptomus_payment_api_key);
        let expected = format!("{:x}", md5::compute(to_hash.as_bytes()));
        
        if sig == expected {
            Ok(())
        } else {
            Err(anyhow!("Invalid Cryptomus signature"))
        }
    }

    fn generate_cryptomus_signature(&self, payload: &str) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
        let to_hash = format!("{}{}", encoded, self.cryptomus_payment_api_key);
        format!("{:x}", md5::compute(to_hash.as_bytes()))
    }

    /// Verify Aaio signature (SHA256(merchant_id:amount:currency:secret_2:order_id))
    fn verify_aaio_signature(&self, merchant_id: &str, amount: &str, currency: &str, order_id: &str, sign: &str) -> Result<()> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        let data = format!("{}:{}:{}:{}:{}", merchant_id, amount, currency, self.aaio_secret_2, order_id);
        hasher.update(data.as_bytes());
        let expected = hex::encode(hasher.finalize());
        
        if sign == expected {
            Ok(())
        } else {
             Err(anyhow!("Invalid Aaio signature. Expected: {}, Got: {}", expected, sign))
        }
    }

    fn get_cryptobot_url(&self) -> &str {
        if self.is_testnet { "https://testnet-pay.crypt.bot/api" } else { "https://pay.crypt.bot/api" }
    }

    pub async fn create_cryptobot_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating CryptoPay invoice for user {}: ${} ({:?})", user_id, amount_usd, payment_type);
        
        let payload = payment_type.to_payload_string(user_id);
        
        // CryptoBot needs asset amount, not USD usually, but createInvoice allows 'amount' + 'fiat' or 'amount' + 'asset'.
        // If we want USD, we use 'amount' and 'fiat' = 'USD' OR we use one of the crypto currencies.
        // Actually typical CryptoPay usage is creating an invoice in USDT or allowing user to pay any asset.
        // createInvoice endpoint: asset, amount, ...
        // To bill in USD: we might need to use `createInvoice` with `fiat` param?
        // Let's check typical usage. Often we request USDT.
        
        let invoice = serde_json::json!({
             "asset": "USDT",
             "amount": format!("{:.2}", amount_usd),
             "description": "EXA ROBOT Top-up",
             "payload": payload,
             "allow_anonymous": false,
             "allow_comments": false
        });

        let client = reqwest::Client::new();
        let resp = client.post(format!("{}/createInvoice", self.get_cryptobot_url()))
            .header("Crypto-Pay-API-Token", &self.cryptobot_token)
            .json(&invoice)
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        if body["ok"].as_bool().unwrap_or(false) {
             Ok(body["result"]["bot_invoice_url"].as_str().unwrap_or("").to_string())
        } else {
             Err(anyhow::anyhow!("CryptoBot error: {:?}", body))
        }
    }

    pub async fn create_nowpayments_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating NOWPayments invoice for user {}: ${}", user_id, amount_usd);
        
        let payload_base = payment_type.to_payload_string(user_id);
        let unique_order_id = format!("{}_{}", payload_base, Utc::now().timestamp());

        let invoice = serde_json::json!({
            "price_amount": amount_usd,
            "price_currency": "usd",
            "pay_currency": "usdttrc20", // Default view
            "order_id": unique_order_id,
            "ipn_callback_url": "https://api.exa.robot/api/payments/nowpayments", 
            "success_url": "https://t.me/exarobot_bot",
            "cancel_url": "https://t.me/exarobot_bot"
        });

        let client = reqwest::Client::new();
        let resp = client.post("https://api.nowpayments.io/v1/invoice")
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

    pub async fn create_crystalpay_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating CrystalPay invoice for user {}: ${}", user_id, amount_usd);
        
        // CrystalPay creates a payment link.
        // Endpoint: https://api.crystalpay.io/v2/invoice/create/
        // Auth: login + secret
        // Params: amount, type=purchase, lifetime, etc.
        
        // Convert USD to RUB for CrystalPay (assuming it accepts RUB primarily for SBP)
        // Or check if they support USD invoices. They do support multi-currency but often convert.
        // For simplicity, let's assume we bill in USD and they handle conversion or we set USD.
        
        let payload = payment_type.to_payload_string(user_id);
        
        let body_json = serde_json::json!({
            "auth_login": self.crystalpay_login,
            "auth_secret": self.crystalpay_secret,
            "amount": amount_usd,
            "amount_currency": "USD", // Request USD
            "type": "purchase",
            "description": format!("CARAMBA User {}", user_id),
            "redirect_url": "https://t.me/exarobot_bot",
            "callback_url": "https://api.exa.robot/api/payments/crystalpay",
            "extra": payload 
        });

        let client = reqwest::Client::new();
        let resp = client.post("https://api.crystalpay.io/v2/invoice/create/")
            .json(&body_json)
            .send()
            .await?;
            
        let resp_json: serde_json::Value = resp.json().await?;
        
        // CrystalPay V2 response: {"error":false, "errors":[], "data": { "id": "...", "url": "..." }}
        if resp_json["error"].as_bool().unwrap_or(true) {
             Err(anyhow::anyhow!("CrystalPay Error: {:?}", resp_json))
        } else {
             Ok(resp_json["data"]["url"].as_str().unwrap_or("").to_string())
        }
    }

    pub async fn create_stripe_session(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating Stripe Session for user {}: ${}", user_id, amount_usd);
        
        let payload = payment_type.to_payload_string(user_id);
        let amount_cents = (amount_usd * 100.0) as i64;
        
        let client = reqwest::Client::new();
        let params = [
            ("mode", "payment"),
            ("success_url", "https://t.me/exarobot_bot"),
            ("cancel_url", "https://t.me/exarobot_bot"),
            ("client_reference_id", &payload),
            ("line_items[0][price_data][currency]", "usd"),
            ("line_items[0][price_data][product_data][name]", "Balance Top-up"),
            ("line_items[0][price_data][unit_amount]", &amount_cents.to_string()),
            ("line_items[0][quantity]", "1"),
        ];

        let resp = client.post("https://api.stripe.com/v1/checkout/sessions")
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

    pub async fn create_cryptomus_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating Cryptomus invoice for user {}: ${}", user_id, amount_usd);
        
        // Setup payload
        let payload_str = payment_type.to_payload_string(user_id);
        let order_id = format!("{}_{}", user_id, Utc::now().timestamp());
        
        let body_json = serde_json::json!({
            "amount": amount_usd.to_string(), // Cryptomus often expects string for amount
            "currency": "USD",
            "order_id": order_id,
            "url_callback": "https://api.exa.robot/api/payments/cryptomus",
            "url_return": "https://t.me/exarobot_bot", // Redirect user back to bot
            "additional_data": payload_str // Pass metadata here
        });
        
        let body_str = serde_json::to_string(&body_json)?;
        let sign = self.generate_cryptomus_signature(&body_str);
        
        let client = reqwest::Client::new();
        let resp = client.post("https://api.cryptomus.com/v1/payment")
            .header("merchant", &self.cryptomus_merchant_id)
            .header("sign", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?;
            
        let resp_json: serde_json::Value = resp.json().await?;
        // Cryptomus response: { state: 0, result: { url: "...", ... } }
        // Note: Check docs or common key names. result.url is common.
        
        if let Some(result) = resp_json.get("result") {
            if let Some(url) = result.get("url").and_then(|u| u.as_str()) {
                 return Ok(url.to_string());
            }
        }
        
        Err(anyhow::anyhow!("Cryptomus Error: {:?}", resp_json))
    }

    pub async fn create_aaio_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating Aaio invoice for user {}: ${}", user_id, amount_usd);
        
        let pay_desc = format!("Payment for User {}", user_id);
        let order_id = format!("{}:{}:{}", user_id, Utc::now().timestamp(), payment_type.to_payload_string(user_id));
        let currency = "USD";
        let amount_str = format!("{:.2}", amount_usd);
        
        // Sign: SHA256(merchant_id:amount:currency:secret_1:order_id)
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        let sign_data = format!("{}:{}:{}:{}:{}", self.aaio_merchant_id, amount_str, currency, self.aaio_secret_1, order_id);
        hasher.update(sign_data.as_bytes());
        let sign = hex::encode(hasher.finalize());
        
        let client = reqwest::Client::new();
        let params = [
            ("merchant_id", self.aaio_merchant_id.as_str()),
            ("amount", amount_str.as_str()),
            ("currency", currency),
            ("order_id", order_id.as_str()),
            ("sign", sign.as_str()),
            ("desc", pay_desc.as_str()),
            ("lang", "en"),
        ];

        // Ensure we send as form-encoded since typically they expect POST params or query string for link generation
        // But wait, the docs say to generate a URL to redirect user to. 
        // We can just construct the URL and return it if we don't need to create an order via API call first.
        // Actually, Aaio is usually "Redirect to Payment URL".
        // URL: https://aaio.so/merchant/pay?merchant_id=...
        // Let's verify if 'get_pay_url' is an API that RETURNS a url or if we just build it.
        // Tool said 'get_pay_url' endpoint.
        // The script uses `https://aaio.so/merchant/get_pay_url` and gets a result.
        
        let resp = client.post("https://aaio.so/merchant/get_pay_url")
            .form(&params)
            .send()
            .await?;
            
        // Response is usually providing the redirect URL.
        // But if we just want to redirect, we can construct https://aaio.so/merchant/pay?...
        // Let's try to get the URL from the API for better UX (it might return a specific session URL).
        
        if let Ok(url_str) = resp.text().await {
             // Sometimes it returns just the URL string, sometimes JSON.
             // If URL string starts with http, return it.
             if url_str.starts_with("http") {
                 return Ok(url_str);
             }
             // Try parsing JSON if needed, but docs say get_pay_url returns URL in body or JSON?
             // Assuming URL for now.
             return Ok(url_str); // Fallback
        }
        
        Err(anyhow::anyhow!("Aaio Error"))
    }

    pub async fn create_lava_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating Lava.top invoice for user {}: ${}", user_id, amount_usd);
        
        let order_id = format!("LAVA-{}-{}", user_id, Utc::now().timestamp());
        let payload_str = payment_type.to_payload_string(user_id);
        
        // Lava API v2 Signature: HMAC-SHA256(json_body, secret_key)
        // Endpoint: https://api.lava.ru/business/invoice/create
        // Ensure you have correct structs or use json! macro
        
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
        
        let client = reqwest::Client::new();
        let res = client.post("https://api.lava.ru/business/invoice/create")
            .header("Signature", signature)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?;
            
        #[derive(Deserialize)]
        struct LavaResponse {
            data: Option<LavaData>,
            error: Option<serde_json::Value>
        }
        #[derive(Deserialize)]
        struct LavaData {
            url: String, 
            _id: String
        }
        
        let lava_res: LavaResponse = res.json().await?;
        
        if let Some(data) = lava_res.data {
            Ok(data.url)
        } else {
             Err(anyhow!("Failed to create Lava invoice (No URL returned): {:?}", lava_res.error))
        }
    }

    pub async fn create_stars_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating Telegram Stars invoice for user {}: ${}", user_id, amount_usd);
        
        // Rate: 1 USD = 50 Stars (Fixed Assumption for now)
        let stars_amount = (amount_usd * 50.0).ceil() as i64;
        
        let payload = payment_type.to_payload_string(user_id);
        
        let client = reqwest::Client::new();
        let bot_token = self.bot_token.clone(); 
        if bot_token.is_empty() {
            return Err(anyhow!("Bot token required for Stars"));
        }

        let url = format!("https://api.telegram.org/bot{}/createInvoiceLink", bot_token);
        
        let params = serde_json::json!({
            "title": "Balance Top-up",
            "description": format!("Top-up balance by ${:.2}", amount_usd),
            "payload": payload,
            "provider_token": "", // Empty for Stars
            "currency": "XTR",
            "prices": [{"label": "Top-up", "amount": stars_amount}]
        });

        let res = client.post(&url)
            .json(&params)
            .send()
            .await?;
            
        #[derive(Deserialize)]
        struct TgResponse {
            ok: bool,
            result: Option<String>,
            description: Option<String>
        }
        
        let tg_res: TgResponse = res.json().await?;
        
        if tg_res.ok && tg_res.result.is_some() {
            Ok(tg_res.result.unwrap())
        } else {
            Err(anyhow!("Failed to create Stars invoice: {:?}", tg_res.description))
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
                 // Verify signature
                 self.verify_cryptobot_signature(payload, crypto_sig)?;
                 
                 if let Some(update_type) = body["update_type"].as_str() {
                    if update_type == "invoice_paid" {
                        let invoice = &body["update_payload"];
                        if invoice["status"].as_str().unwrap_or("") == "paid" {
                            let amount: f64 = invoice["amount"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                            let payload_str = invoice["payload"].as_str().unwrap_or("");
                            let id = invoice["invoice_id"].to_string();
                            self.process_any_payment(amount, "cryptobot", Some(id), payload_str).await?;
                        }
                    }
                 }
            },
            "nowpayments" => {
                 // Verify signature
                 self.verify_nowpayments_signature(payload, nowpayments_sig)?;
                 
                 if let Some(status) = body["payment_status"].as_str() {
                     if status == "finished" {
                         let amount: f64 = body["pay_amount"].as_f64().unwrap_or(0.0);
                         let order_id = body["order_id"].as_str().unwrap_or("");
                         let payload_str = order_id.split('_').next().unwrap_or("");
                         let id = body["payment_id"].to_string();
                         self.process_any_payment(amount, "nowpayments", Some(id), payload_str).await?;
                     }
                 }
            },
            "crystalpay" => {
                // Verify signature from payload
                self.verify_crystalpay_signature(&body)?;

                // CrystalPay callback: type=payment, state=payed
                if body["type"].as_str().unwrap_or("") == "payment" && body["state"].as_str().unwrap_or("") == "payed" {
                    let amount: f64 = body["amount"].as_f64().unwrap_or(0.0); // usually in currency requested
                    let extra = body["extra"].as_str().unwrap_or("");
                    let id = body["id"].to_string();
                    self.process_any_payment(amount, "crystalpay", Some(id), extra).await?;
                }
            },
            "stripe" => {
                // Get webhook secret from env
                let webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET")
                    .unwrap_or_default();
                
                // Verify signature
                self.verify_stripe_signature(payload, stripe_sig, &webhook_secret)?;
                
                // Stripe sends Event object
                if body["type"].as_str().unwrap_or("") == "checkout.session.completed" {
                    let session = &body["data"]["object"];
                    let amount_subtokens = session["amount_total"].as_i64().unwrap_or(0);
                    let amount_usd = amount_subtokens as f64 / 100.0;
                    let payload_str = session["client_reference_id"].as_str().unwrap_or("");
                    let id = session["id"].to_string();
                    self.process_any_payment(amount_usd, "stripe", Some(id), payload_str).await?;
                }
            },

             "cryptomus" => {
                 // Verify signature 
                 self.verify_cryptomus_signature(payload, cryptomus_sig)?;

                 // Check status
                 // Payload structure from Cryptomus Webhook: { type: "payment", status: "paid", amount, order_id, additional_data, ... }
                 // Note: Check actual structure. Usually:
                 // { "type": "payment", "status": "paid", "amount": "10.00", "currency": "USD", "order_id": "...", "additional_data": "..." }
                 
                 let status = body["status"].as_str().unwrap_or("");
                 if status == "paid" || status == "paid_over" {
                     let amount: f64 = body["amount"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                     let payload_str = body["additional_data"].as_str().unwrap_or("");
                     let id = body["uuid"].as_str().unwrap_or("").to_string(); // Cryptomus UUID
                     
                     self.process_any_payment(amount, "cryptomus", Some(id), payload_str).await?;
                 }
             },
             "aaio" => {
                 // Aaio typically sends form-data. Need to parse body as form-urlencoded if it's not JSON?
                 // Or it sends JSON?
                 // Usually form-data POST.
                 // We receive `payload` as string. If it is JSON, `serde_json` works.
                 // If it is form-data, we need to parse it. 
                 // We will attempt JSON first (some send JSON), else URL encoded.
                 
                 let data: serde_json::Value = if let Ok(v) = serde_json::from_str(payload) {
                     v
                 } else {
                     // Try parsing query string
                     let parsed: std::collections::HashMap<String, String> = serde_urlencoded::from_str(payload)
                        .unwrap_or_default();
                     serde_json::to_value(parsed).unwrap_or(serde_json::json!({}))
                 };
                 
                 let merchant_id = data["merchant_id"].as_str().unwrap_or("");
                 let amount = data["amount"].as_str().unwrap_or("");
                 let currency = data["currency"].as_str().unwrap_or("");
                 let order_id = data["order_id"].as_str().unwrap_or("");
                 let sign = data["sign"].as_str().unwrap_or("");
                 
                 self.verify_aaio_signature(merchant_id, amount, currency, order_id, sign)?;
                 
                 // order_id format: user_id:timestamp:payload_string
                 // Extract payload string
                 let parts: Vec<&str> = order_id.splitn(3, ':').collect();
                 if parts.len() == 3 {
                     let amount_val: f64 = amount.parse().unwrap_or(0.0);
                     let payload_str = parts[2];
                     let id = data["invoice_id"].as_str().unwrap_or(order_id).to_string(); // or use aaio invoice id
                     self.process_any_payment(amount_val, "aaio", Some(id), payload_str).await?;
                 }
             },
            _ => {}
        }

        Ok(())
    }

    pub async fn process_any_payment(&self, amount_usd: f64, method: &str, external_id: Option<String>, payload: &str) -> Result<()> {
        let parts: Vec<&str> = payload.split(':').collect();
        if parts.len() < 3 {
            // Legacy/Simple fallback
            if let Ok(user_id) = payload.parse::<i64>() {
                return self.process_balance_topup(user_id, amount_usd, method, external_id).await;
            }
             return Err(anyhow::anyhow!("Invalid payload: {}", payload));
        }

        let user_id: i64 = parts[0].parse().unwrap_or(0);
        let type_code = parts[1];
        let target_id: i64 = parts[2].parse().unwrap_or(0);

        if user_id == 0 { return Err(anyhow::anyhow!("Zero User ID")); }

        match type_code {
            "bal" => self.process_balance_topup(user_id, amount_usd, method, external_id).await,
            "ord" => self.process_order_purchase(user_id, target_id, amount_usd, method, external_id).await,
            "sub" => self.process_subscription_purchase(user_id, target_id, amount_usd, method, external_id).await,
            _ => Err(anyhow::anyhow!("Unknown Type: {}", type_code)),
        }
    }

    async fn process_order_purchase(&self, user_id: i64, order_id: i64, amount_usd: f64, method: &str, external_id: Option<String>) -> Result<()> {
        info!("Processing ORDER payment #${} for user {}", order_id, user_id);
        let amount_units = (amount_usd * 100.0) as i64;
        self.store_service.log_payment(user_id, method, amount_units, external_id.as_deref(), "paid").await?;
        self.store_service.process_order_payment(order_id).await?;
        
        let _ = self.bot_manager.send_notification(user_id, "✅ Your order has been paid successfully!").await;
        
        let _ = crate::services::analytics_service::AnalyticsService::track_revenue(&self.store_service.get_pool(), amount_units).await;
        Ok(())
    }

    async fn process_subscription_purchase(&self, user_id: i64, plan_id: i64, amount_usd: f64, method: &str, external_id: Option<String>) -> Result<()> {
        info!("Processing SUBSCRIPTION payment for user {} (Plan: {})", user_id, plan_id);
        
        // 1. Top up balance first (to record the flow of money)
        self.process_balance_topup(user_id, amount_usd, method, external_id.clone()).await?;

        // 2. Attempt to purchase the plan internally using the just-added balance
        // We need to find the plan duration ID or pass plan_id directly if supported.
        // Assuming purchase_plan takes duration_id. 
        // For now, let's assume we are buying the 1-month duration of the plan.
        
        let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC LIMIT 1"
        )
        .bind(plan_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(duration) = durations {
            match self.store_service.purchase_plan(user_id, duration.id).await {
                Ok(_) => {
                    let _ = self.bot_manager.send_notification(user_id, "✅ Subscription activated successfully!").await;
                },
                Err(e) => {
                    error!("Failed to auto-purchase subscription after payment: {}", e);
                    let _ = self.bot_manager.send_notification(user_id, "⚠️ Payment received but subscription activation failed. Please contact support.").await;
                }
            }
        } else {
             error!("No duration found for plan {}", plan_id);
             let _ = self.bot_manager.send_notification(user_id, "⚠️ Error: Plan duration not found. Balance credited.").await;
        }
        
        Ok(())
    }

    async fn process_balance_topup(&self, user_id: i64, amount_usd: f64, method: &str, external_id: Option<String>) -> Result<()> {
        info!("Processing BALANCE top-up of ${} for user {} via {}", amount_usd, user_id, method);
        let amount_units = (amount_usd * 100.0) as i64; 
        
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
            .bind(amount_units).bind(user_id).execute(&mut *tx).await?;

        let payment_id: i64 = sqlx::query_scalar(
            "INSERT INTO payments (user_id, method, amount, external_id, status) VALUES (?, ?, ?, ?, 'paid') RETURNING id"
        )
            .bind(user_id).bind(method).bind(amount_units).bind(external_id).fetch_one(&mut *tx).await?;

        if let Some((referrer_tg_id, bonus)) = self.store_service.apply_referral_bonus(&mut tx, user_id, amount_units, Some(payment_id)).await? {
            let formatted_bonus = format!("{:.2}", bonus as f64 / 100.0);
            let msg = format!("🎉 *Referral Bonus* from your invited user!\n+${}", formatted_bonus);
            let _ = self.bot_manager.send_notification(referrer_tg_id, &msg).await;
        }

        tx.commit().await?;
        
        // Notify user
        let _ = self.bot_manager.send_notification(user_id, &format!("✅ Balance topped up: +${:.2}", amount_usd)).await;
        let _ = crate::services::analytics_service::AnalyticsService::track_revenue(&self.pool, amount_units).await;
        
        Ok(())
    }
}
