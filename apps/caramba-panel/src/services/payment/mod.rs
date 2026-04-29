use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::payment::PaymentType;

pub mod aaio;
pub mod cryptomus;
pub mod lava;

#[async_trait]
pub trait PaymentAdapter: Send + Sync {
    /// Create an invoice and return the payment URL
    async fn create_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
        bot_username: &str,
        api_domain: &str,
    ) -> Result<String>;

    /// Verify the webhook signature
    fn verify_signature(&self, payload: &str, signature: Option<&str>) -> Result<()>;

    /// Get the adapter name
    fn name(&self) -> &str;
}

pub mod balance;
pub mod btcpay;
pub mod coinbase_commerce;
pub mod crystalpay;
pub mod cryptobot;
pub mod manual;
pub mod nowpayments;
pub mod oxapay;
pub mod plisio;
pub mod provider;
pub mod stripe;
pub mod telegram_stars;
pub mod tribute;
pub mod wata;

#[allow(unused_imports)]
pub use provider::{PaymentProvider, PaymentWebhookAction};
