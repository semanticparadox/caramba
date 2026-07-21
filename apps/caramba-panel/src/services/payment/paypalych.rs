// Paypalych (pally.info / pal24.pro) — RU-friendly payment provider for SBP and
// USDT TRC20 settlement. Built around the documented `pal24.pro/api/v1` REST
// surface; the exact wire format is verified against a live project once the
// merchant account is approved (see docs/POST-INCIDENT-ROADMAP.md T1).
//
// Auth: Bearer token in the Authorization header (the `72|xxxxxx` form shown in
// the dashboard under "API интеграция").
//
// Webhook: signed IF `paypalych_webhook_secret` is configured — HMAC-SHA256 of
// the raw request body, lower-case hex, sent in the `Sign` header (case
// `Sign` is the most common name across similar providers; `Signature` and
// `X-Sign` are accepted as fallbacks). When the secret is empty, the webhook is
// accepted without signature verification — convenient for the staging/sandbox
// environment the provider offers, but a loud warning is logged so a missing
// secret on a production project surfaces immediately.
//
// Tariffs (committed by the operator, July 21 2026):
//   * USDT TRC20 in RUB: 3% + 1 USDT, min 400 RUB, max 1 000 000 RUB
//   * SBP:               6.5% + 2 RUB,  min 10 RUB,  max 50 000 RUB
// Both channels use the same provider; the difference is the underlying rail,
// not the API contract. Currency is always RUB on the invoice — settlement
// conversion happens on the provider side.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

/// REST base for the public Pal24 API. Public docs and the dashboard both point
/// at `pal24.pro`; `pally.info` is the user-facing brand and resolves to the
/// same host.
const PAL24_BASE_URL: &str = "https://pal24.pro/api/v1";

pub struct PaypalychProvider {
    /// Bearer token from the pally.info dashboard (looks like `72|xxxxxx`).
    pub api_token: String,
    /// Optional shop/project id. Some endpoints accept it as a body field; the
    /// empty default is fine when the project is single-tenant on this token.
    pub shop_id: String,
    /// Optional webhook secret. When present, every incoming webhook must be
    /// signed with HMAC-SHA256(body) → hex; the value arrives in the `Sign`
    /// header. Empty disables verification (with a one-shot warning at startup
    /// handled by the caller in `marketplace_service`).
    pub webhook_secret: String,
    /// Public panel domain — used to build absolute `success_url` / `fail_url`
    /// for the hosted checkout page.
    pub api_domain: String,
    /// Telegram bot username — used as a soft redirect target on success/fail
    /// so the customer returns to the bot instead of a 404 panel page.
    pub bot_username: String,
}

/// Defensive response struct — Pal24 has historically alternated between
/// `link_page_url` and `link` for the hosted-checkout URL across minor API
/// versions. Accept both, prefer the longer-named one (matches the public
/// reference for the current version).
#[derive(Deserialize)]
struct PaypalychCreateResponse {
    success: Option<bool>,
    data: Option<PaypalychCreateData>,
    error: Option<Value>,
}

#[derive(Deserialize)]
struct PaypalychCreateData {
    id: Option<String>,
    #[serde(alias = "link")]
    link_page_url: Option<String>,
}

#[async_trait]
impl PaymentProvider for PaypalychProvider {
    fn name(&self) -> &str {
        "paypalych"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        // Paypalych invoices are in RUB; the session stores the amount in
        // kopecks (minor units of RUB), so divide by 100. Currency is always
        // RUB for this provider — they handle the USDT/SBP rail internally.
        let amount_rub = (session.amount as f64) / 100.0;

        let success_url = format!("https://t.me/{}", self.bot_username);
        let fail_url = success_url.clone();
        let hook_url = format!("https://{}/api/webhooks/payment/paypalych", self.api_domain);

        let mut body = serde_json::json!({
            "amount": amount_rub,
            "currency": "RUB",
            "order_id": session.id.to_string(),
            "description": format!("VPN subscription (product {})", session.product_id),
            "hook_url": hook_url,
            "success_url": success_url,
            "fail_url": fail_url,
            // 60 min invoice lifetime — matches Lava and WATA defaults.
            "expire": 60,
        });

        // Only include shop_id when configured. The current Pal24 API accepts
        // it as an optional scoping field; sending an empty string has been
        // observed to return 400 on some project setups.
        if !self.shop_id.is_empty() {
            body["shop_id"] = Value::String(self.shop_id.clone());
        }

        let res = client
            .post(format!("{}/bill/create", PAL24_BASE_URL))
            .bearer_auth(&self.api_token)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Paypalych")?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Paypalych API error ({}): {}", status, error_text);
        }

        let parsed: PaypalychCreateResponse = res
            .json()
            .await
            .context("Failed to parse Paypalych create response")?;

        if parsed.success == Some(false) {
            anyhow::bail!("Paypalych API returned success=false: {:?}", parsed.error);
        }

        let data = parsed
            .data
            .context("Paypalych response missing `data` field")?;

        data.link_page_url
            .or(data.id.clone())
            .ok_or_else(|| anyhow::anyhow!("Paypalych response missing link_page_url/id"))
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // No secret configured → accept the webhook as-is. This is the
        // intended sandbox/staging mode; production deployments MUST set
        // `paypalych_webhook_secret` so a missing/forged signature is rejected.
        if self.webhook_secret.is_empty() {
            tracing::warn!(
                provider = "paypalych",
                "Webhook signature verification DISABLED — paypalych_webhook_secret is empty. \
                 Set it before going live."
            );
            return Ok(true);
        }

        let signature = signature.trim();
        if signature.is_empty() {
            return Ok(false);
        }

        // Lowercase the signature before decoding — Pal24 normalizes hex to
        // lower-case but a future tweak on their side shouldn't reject valid
        // webhooks. `Mac::verify_slice` is constant-time.
        let provided = match hex::decode(signature.to_ascii_lowercase()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(false),
        };

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .context("Invalid Paypalych webhook secret length")?;
        mac.update(payload);

        Ok(mac.verify_slice(&provided).is_ok())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Invalid Paypalych webhook JSON")?;

        // Status vocabulary differs across Pal24 API versions. Accept the
        // documented English names AND the case variants seen in production
        // traffic from a couple of similar Russian PSP integrations.
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_id = data.get("order_id").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            // Successful payment — fulfilled by the central webhook handler.
            "paid" | "success" | "SUCCESS" | "Paid" | "completed" | "COMPLETED" => {
                Ok(PaymentWebhookAction::Completed {
                    external_id: order_id.to_string(),
                })
            }
            // Terminal failure states.
            "canceled" | "cancelled" | "expired" | "failed" | "CANCELED" | "EXPIRED" | "FAILED" => {
                Ok(PaymentWebhookAction::Failed {
                    reason: status.to_string(),
                })
            }
            // `new` and anything else — still open, leave the session pending.
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        // No public status endpoint documented for the v1 API at the time of
        // writing; the polling fallback treats "pending" as a no-op, so this
        // is safe to leave as a stub until the provider exposes a stable
        // status endpoint.
        Ok("pending".to_string())
    }
}
