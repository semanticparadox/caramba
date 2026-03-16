use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};

use super::provider::{PaymentProvider, PaymentWebhookAction};

pub struct StarsProvider;

#[async_trait]
impl PaymentProvider for StarsProvider {
    fn name(&self) -> &str {
        "stars"
    }

    async fn create_invoice(
        &self,
        _session: &PaymentSession,
        _user: &User,
        _client: &reqwest::Client,
    ) -> Result<String> {
        // TODO: Generate standard Telegram Stars payload
        Ok("Stars Payload Stub".to_string())
    }

    async fn verify_webhook(&self, _payload: &[u8], _signature: &str) -> Result<bool> {
        // Telegram stars are verified via the bot API pre_checkout_query, not a standard webhook
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
