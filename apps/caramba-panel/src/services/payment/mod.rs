use async_trait::async_trait;
use caramba_db::models::payment::PaymentType;
use anyhow::Result;

pub mod cryptomus;

#[async_trait]
pub trait PaymentAdapter: Send + Sync {
    /// Create an invoice and return the payment URL
    async fn create_invoice(&self, user_id: i64, amount_usd: f64, payment_type: PaymentType, bot_username: &str, api_domain: &str) -> Result<String>;
    
    /// Verify the webhook signature
    fn verify_signature(&self, payload: &str, signature: Option<&str>) -> Result<()>;
    
    /// Get the adapter name
    fn name(&self) -> &str;
}
