use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};

use super::provider::{PaymentProvider, PaymentWebhookAction};

pub struct ManualProvider;

#[async_trait]
impl PaymentProvider for ManualProvider {
    fn name(&self) -> &str {
        "manual"
    }

    async fn create_invoice(&self, _session: &PaymentSession, _user: &User, _client: &reqwest::Client) -> Result<String> {
        // Returns the static page URL to upload a screenshot
        Ok("/manual-upload".to_string())
    }

    async fn verify_webhook(&self, _payload: &[u8], _signature: &str) -> Result<bool> {
        // Manual doesn't use webhooks
        Ok(true)
    }

    async fn handle_webhook(&self, _payload: &[u8]) -> Result<PaymentWebhookAction> {
        Ok(PaymentWebhookAction::Ignored)
    }

    async fn check_status(&self, _session: &PaymentSession, _client: &reqwest::Client) -> Result<String> {
        Ok("pending".to_string())
    }
}
