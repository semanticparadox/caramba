use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

pub struct StripeProvider {
    /// Stripe secret key (sk_live_... or sk_test_...)
    pub secret_key: String,
    /// Stripe webhook signing secret (whsec_...)
    pub webhook_secret: String,
    /// Full API domain for success/cancel redirect
    pub bot_username: String,
}

#[derive(Deserialize)]
struct StripeCheckoutSession {
    id: String,
    #[serde(default)]
    url: Option<String>,
}

#[async_trait]
impl PaymentProvider for StripeProvider {
    fn name(&self) -> &str {
        "stripe"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount_cents = session.amount; // уже в центах
        let success_url = format!("https://t.me/{}", self.bot_username);
        let cancel_url = success_url.clone();

        // Stripe expects a lowercase ISO currency. Honor the per-method override; default usd.
        let currency = {
            let c = session.currency.trim().to_lowercase();
            if c.is_empty() {
                "usd".to_string()
            } else {
                c
            }
        };

        // Stripe принимает form-encoded для checkout.sessions
        let params = [
            ("mode", "payment".to_string()),
            ("success_url", success_url),
            ("cancel_url", cancel_url),
            ("client_reference_id", session.id.to_string()),
            ("line_items[0][price_data][currency]", currency),
            (
                "line_items[0][price_data][product_data][name]",
                "VPN Subscription".to_string(),
            ),
            (
                "line_items[0][price_data][unit_amount]",
                amount_cents.to_string(),
            ),
            ("line_items[0][quantity]", "1".to_string()),
        ];

        let res = client
            .post("https://api.stripe.com/v1/checkout/sessions")
            .basic_auth(&self.secret_key, None::<&str>)
            .form(&params)
            .send()
            .await
            .context("Failed to send request to Stripe")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Stripe API Error: {}", error_text);
        }

        let checkout: StripeCheckoutSession =
            res.json().await.context("Failed to parse Stripe response")?;

        checkout.url.ok_or_else(|| {
            anyhow::anyhow!(
                "Stripe returned checkout session {} but no URL",
                checkout.id
            )
        })
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        if self.webhook_secret.is_empty() {
            anyhow::bail!("Stripe webhook_secret is not configured");
        }

        // Stripe-Signature header format: "t=<timestamp>,v1=<hex_hmac>"
        let payload_str = std::str::from_utf8(payload).unwrap_or("");

        let mut timestamp = "";
        let mut sig_v1 = "";
        for part in signature.split(',') {
            if let Some(val) = part.strip_prefix("t=") {
                timestamp = val;
            } else if let Some(val) = part.strip_prefix("v1=") {
                sig_v1 = val;
            }
        }

        if timestamp.is_empty() || sig_v1.is_empty() {
            return Ok(false);
        }

        // Защита от replay: вебхуки старше 5 минут отклоняются
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts: u64 = timestamp.parse().unwrap_or(0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now.saturating_sub(ts) > 300 {
            anyhow::bail!("Stripe webhook timestamp too old (replay attack protection)");
        }

        use hmac::{Hmac, Mac};
        type HmacSha256 = Hmac<sha2::Sha256>;

        let signed_payload = format!("{}.{}", timestamp, payload_str);
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .context("Invalid HMAC key")?;
        mac.update(signed_payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        Ok(sig_v1 == expected)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid Stripe webhook JSON")?;

        let event_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "checkout.session.completed" => {
                let session = &data["data"]["object"];
                // client_reference_id хранит PaymentSession UUID
                let session_id = session["client_reference_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if session_id.is_empty() {
                    tracing::warn!("Stripe webhook: checkout.session.completed missing client_reference_id");
                    return Ok(PaymentWebhookAction::Ignored);
                }
                Ok(PaymentWebhookAction::Completed {
                    external_id: session_id,
                })
            }
            "checkout.session.expired" | "payment_intent.payment_failed" => {
                let reason = format!("Stripe event: {}", event_type);
                Ok(PaymentWebhookAction::Failed { reason })
            }
            _ => Ok(PaymentWebhookAction::Ignored),
        }
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}
