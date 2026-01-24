use sqlx::SqlitePool;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;
use std::sync::Arc;
use chrono::Utc;
use crate::services::store_service::StoreService;
use crate::bot_manager::BotManager;

#[derive(Debug, Serialize, Deserialize)]
pub struct CryptoBotInvoice {
    pub asset: String,
    pub amount: String,
    pub description: Option<String>,
    pub payload: Option<String>,
    pub paid_btn_name: Option<String>,
    pub paid_btn_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CryptoBotResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
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
}

impl PaymentType {
    pub fn to_payload_string(&self, user_id: i64) -> String {
        match self {
            PaymentType::BalanceTopup => format!("{}:bal:0", user_id),
            PaymentType::OrderPurchase(order_id) => format!("{}:ord:{}", user_id, order_id),
        }
    }
}

pub struct PayService {
    pool: SqlitePool,
    #[allow(dead_code)]
    store_service: Arc<StoreService>,
    bot_manager: Arc<BotManager>,
    cryptobot_token: String,
    nowpayments_key: String,
    is_testnet: bool,
}

impl PayService {
    pub fn new(pool: SqlitePool, store_service: Arc<StoreService>, bot_manager: Arc<BotManager>, cryptobot_token: String, nowpayments_key: String, is_testnet: bool) -> Self {
        Self { pool, store_service, bot_manager, cryptobot_token, nowpayments_key, is_testnet }
    }

    fn get_cryptobot_url(&self) -> &str {
        if self.is_testnet {
            "https://testnet-pay.crypt.bot/api"
        } else {
            "https://pay.crypt.bot/api"
        }
    }

    pub async fn create_cryptobot_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating CryptoPay invoice for user {}: ${} ({:?})", user_id, amount_usd, payment_type);
        
        let payload = payment_type.to_payload_string(user_id);
        let description = match payment_type {
            PaymentType::BalanceTopup => "EXA ROBOT Balance Top-up".to_string(),
            PaymentType::OrderPurchase(oid) => format!("Example Order #{}", oid),
        };

        let invoice = CryptoBotInvoice {
            asset: "USDT".to_string(),
            amount: format!("{:.2}", amount_usd),
            description: Some(description),
            payload: Some(payload),
            paid_btn_name: Some("callback".to_string()),
            paid_btn_url: None,
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!("{}/createInvoice", self.get_cryptobot_url()))
            .header("Crypto-Pay-API-Token", &self.cryptobot_token)
            .json(&invoice)
            .send()
            .await?;

        let result: CryptoBotResponse<CreateInvoiceResult> = resp.json().await?;
        if !result.ok {
            return Err(anyhow::anyhow!("CryptoBot error: {:?}", result.error));
        }

        Ok(result.result.unwrap().bot_invoice_url)
    }

    pub async fn create_nowpayments_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType) -> Result<String> {
        info!("Creating NOWPayments invoice for user {}: ${} ({:?})", user_id, amount_usd, payment_type);
        
        // NOWPayments doesn't have a free payload field, so we pack metadata into order_id
        // Format: user_id:type:id_timestamp
        // But we want to use the same logic as PaymentType::to_payload_string if possible,
        // however NOWPayments order_id must be unique.
        // Let's use: payload_string + "_" + timestamp
        
        let payload_base = payment_type.to_payload_string(user_id);
        let unique_order_id = format!("{}_{}", payload_base, Utc::now().timestamp());

        let invoice = serde_json::json!({
            "price_amount": amount_usd,
            "price_currency": "usd",
            "pay_currency": "usdttrc20",
            "order_id": unique_order_id,
            "ipn_callback_url": "https://api.exa.robot/api/payments/nowpayments", 
            "success_url": "https://t.me/exarobot",
            "cancel_url": "https://t.me/exarobot"
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

    pub async fn handle_webhook(&self, payload: &str) -> Result<()> {
        let body: serde_json::Value = serde_json::from_str(payload)?;
        
        // Handle CryptoBot
        if let Some(update_type) = body["update_type"].as_str() {
            if update_type == "invoice_paid" {
                let invoice = &body["update_payload"];
                let status = invoice["status"].as_str().unwrap_or("");
                
                if status == "paid" {
                    let amount_str = invoice["amount"].as_str().unwrap_or("0");
                    let amount: f64 = amount_str.parse().unwrap_or(0.0);
                    let payload_str = invoice["payload"].as_str().unwrap_or("");
                    
                    self.process_any_payment(amount, "cryptobot", Some(invoice["invoice_id"].to_string()), payload_str).await?;
                }
            }
        }
        
        // Handle NOWPayments
        if let Some(payment_status) = body["payment_status"].as_str() {
             if payment_status == "finished" {
                let amount: f64 = body["pay_amount"].as_f64().unwrap_or(0.0);
                let order_id_field = body["order_id"].as_str().unwrap_or("");
                // NOWPayments order_id format: payload_timestamp
                let payload_str = order_id_field.split('_').next().unwrap_or("");

                self.process_any_payment(amount, "nowpayments", Some(body["payment_id"].to_string()), payload_str).await?;
             }
        }

        Ok(())
    }

    async fn process_any_payment(&self, amount_usd: f64, method: &str, external_id: Option<String>, payload: &str) -> Result<()> {
        let parts: Vec<&str> = payload.split(':').collect();
        if parts.len() < 3 {
            // Fallback for legacy format (just user_id)
            if let Ok(user_id) = payload.parse::<i64>() {
                return self.process_balance_topup(user_id, amount_usd, method, external_id).await;
            }
            return Err(anyhow::anyhow!("Invalid payload format: {}", payload));
        }

        let user_id: i64 = parts[0].parse().unwrap_or(0);
        let type_code = parts[1];
        let target_id: i64 = parts[2].parse().unwrap_or(0);

        if user_id == 0 {
             return Err(anyhow::anyhow!("Invalid user_id in payload"));
        }

        match type_code {
            "bal" => self.process_balance_topup(user_id, amount_usd, method, external_id).await,
            "ord" => self.process_order_purchase(user_id, target_id, amount_usd, method, external_id).await,
            _ => Err(anyhow::anyhow!("Unknown payment type: {}", type_code)),
        }
    }

    async fn process_order_purchase(&self, user_id: i64, order_id: i64, amount_usd: f64, method: &str, external_id: Option<String>) -> Result<()> {
        info!("Processing ORDER payment #${} for user {}", order_id, user_id);
        let amount_units = (amount_usd * 100.0) as i64;
        
        // 1. Log generic payment
        self.store_service.log_payment(user_id, method, amount_units, external_id.as_deref(), "paid").await?;

        // 2. Mark order as paid
        self.store_service.process_order_payment(order_id).await?;

        Ok(())
    }

    async fn process_balance_topup(&self, user_id: i64, amount_usd: f64, method: &str, external_id: Option<String>) -> Result<()> {
        info!("Processing BALANCE top-up of ${} for user {} via {}", amount_usd, user_id, method);
        let amount_units = (amount_usd * 100.0) as i64; 
        
        let mut tx = self.pool.begin().await?;

        // 1. Update balance
        sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
            .bind(amount_units)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let payment_id: i64 = sqlx::query_scalar(
            "INSERT INTO payments (user_id, method, amount, external_id, status) VALUES (?, ?, ?, ?, 'paid') RETURNING id"
        )
            .bind(user_id)
            .bind(method)
            .bind(amount_units)
            .bind(external_id)
            .fetch_one(&mut *tx)
            .await?;

        // 3. Apply referral bonus (10% of top-up)
        if let Some((referrer_tg_id, bonus)) = self.store_service.apply_referral_bonus(&mut tx, user_id, amount_units, Some(payment_id)).await? {
            // Send notification to referrer
            let formatted_bonus = format!("{:.2}", bonus as f64 / 100.0);
            let msg = format!("🎉 *Referral Bonus Received\\!*\n\nA user you invited has topped up their balance\\.\nYou earned: *${}*", formatted_bonus);
            let _ = self.bot_manager.send_notification(referrer_tg_id, &msg).await;
        }

        tx.commit().await?;
        Ok(())
    }
}
