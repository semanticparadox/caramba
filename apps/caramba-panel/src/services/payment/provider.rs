use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};

pub enum PaymentWebhookAction {
    Completed {
        external_id: String,
    },
    /// Like `Completed`, but carries the amount/currency the provider says was
    /// actually paid so the fulfillment path can verify it against the stored
    /// session before granting the resource (anti under/over-pay abuse, U18).
    ///
    /// `paid_amount_minor` is in the MINOR units of `paid_currency` (e.g. cents
    /// for USD), in the SAME currency the session was priced in — providers that
    /// emit this MUST echo back the invoice (fiat) amount/currency, not the
    /// settled crypto amount, so the comparison against `PaymentSession::amount`
    /// is apples-to-apples. Introduced as a NEW variant (rather than extending
    /// `Completed`) so the ~13 providers still emitting `Completed` keep working
    /// unchanged; only providers where under/over-payment is a real risk migrate.
    CompletedWithAmount {
        external_id: String,
        paid_amount_minor: i64,
        paid_currency: String,
    },
    Failed {
        reason: String,
    },
    Pending,
    Ignored,
}

#[async_trait]
pub trait PaymentProvider: Send + Sync {
    /// Identifier for the provider (e.g., "stars", "nowpayments", "manual")
    fn name(&self) -> &str;

    /// Create an invoice or initialization payload using the provider's API.
    /// Returns a URL, a payload string, or another identifier to surface to the user.
    async fn create_invoice(
        &self,
        session: &PaymentSession,
        user: &User,
        client: &reqwest::Client,
    ) -> Result<String>;

    /// Verify a webhook signature using the raw payload and headers.
    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool>;

    /// Parse the webhook payload and determine the transaction outcome.
    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction>;

    /// Actively check the status of a specific session (polling fallback).
    ///
    /// CONTRACT for the monitoring polling loop (U20/U13): the returned status
    /// string is normalized to the internal vocabulary —
    ///   - `"paid"` / `"completed"` / `"success"` → funds received, fulfill;
    ///   - `"failed"`                              → terminal failure;
    ///   - anything else (incl. `"pending"`)        → still open, do nothing.
    /// Providers WITHOUT a real status-poll endpoint return a constant
    /// `"pending"` here; the poller treats that as a harmless no-op, so it is
    /// safe (if slightly wasteful) to poll them. See `supports_polling`.
    async fn check_status(
        &self,
        session: &PaymentSession,
        client: &reqwest::Client,
    ) -> Result<String>;

    /// Whether this provider exposes a real `check_status` poll that can confirm
    /// a payment out-of-band (a webhook-loss fallback). Defaults to `true` so a
    /// new provider with a genuine `check_status` is polled automatically; the
    /// poller additionally ignores any provider whose `check_status` only ever
    /// returns `"pending"` (every provider except `cryptomus` today), so leaving
    /// the default in place never fulfills anything spuriously. Override to
    /// `false` only to suppress the (cheap) poll attempt entirely — e.g. for
    /// purely off-chain methods like `manual`/`balance`/`telegram_stars` that
    /// can never be confirmed by an HTTP poll.
    fn supports_polling(&self) -> bool {
        true
    }
}
