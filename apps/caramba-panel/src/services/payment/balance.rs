use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};

use super::provider::{PaymentProvider, PaymentWebhookAction};

pub struct BalanceProvider;

#[async_trait]
impl PaymentProvider for BalanceProvider {
    fn name(&self) -> &str {
        "balance"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        user: &User,
        _client: &reqwest::Client,
    ) -> Result<String> {
        if user.balance < session.amount {
            return Err(anyhow::anyhow!(
                "Insufficient balance. You need ${:.2}, but have ${:.2}",
                session.amount as f64 / 100.0,
                user.balance as f64 / 100.0
            ));
        }

        // We don't deduct here yet, we wait for fulfillment or handle it in client.rs synchronously
        // Actually, for "balance", returning a specific payload helps the frontend know it's done.
        Ok("BALANCE_PAYMENT_PENDING".to_string())
    }

    async fn verify_webhook(&self, _payload: &[u8], _signature: &str) -> Result<bool> {
        Ok(true)
    }

    async fn handle_webhook(&self, _payload: &[u8]) -> Result<PaymentWebhookAction> {
        Ok(PaymentWebhookAction::Ignored)
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}
