use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};

pub enum PaymentWebhookAction {
    Completed { external_id: String },
    Failed { reason: String },
    Pending,
    Ignored,
}

#[async_trait]
pub trait PaymentProvider: Send + Sync {
    /// Identifier for the provider (e.g., "stars", "nowpayments", "manual")
    fn name(&self) -> &str;

    /// Create an invoice or initialization payload using the provider's API.
    /// Returns a URL, a payload string, or another identifier to surface to the user.
    async fn create_invoice(&self, session: &PaymentSession, user: &User, client: &reqwest::Client) -> Result<String>;

    /// Verify a webhook signature using the raw payload and headers.
    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool>;

    /// Parse the webhook payload and determine the transaction outcome.
    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction>;

    /// Actively check the status of a specific session (polling fallback).
    async fn check_status(&self, session: &PaymentSession, client: &reqwest::Client) -> Result<String>;
}
