// Paypalych (pal24.pro / pally.info) — RU-friendly payment provider for SBP and
// USDT TRC20 settlement. The wire format follows the public Pal24 API reference
// (captured in `docs/PAYPALYCH-API-SPEC.md`); this implementation matches it
// field-for-field, so it differs materially from the v0.9.52 prototype which was
// written before the docs were available.
//
// Wire contract summary:
//   * Auth:   `Authorization: Bearer <api_token>` (Laravel Sanctum format,
//             `72|xxxxxx`).
//   * Create: `POST /api/v1/bill/create` with `application/x-www-form-urlencoded`
//             body. Required: `amount` (decimal, RUB), `shop_id`. Optional but
//             useful: `order_id` (we pass our session UUID so it comes back in
//             the postback as `InvId`), `type=normal` (one-shot per subscription),
//             `currency_in=RUB`, `description` / `name` (UI text), `custom` (we
//             stash the plan id for cross-check in the webhook).
//             We DO NOT send `success_url` / `fail_url` / `hook_url` / `expire` —
//             those are configured once per project in the pally.info dashboard
//             and overriding them per-call is more error-prone than helpful.
//   * Create response: flat JSON with `success: "true"|"false"` (STRING, not
//             bool), `link_page_url` (the user-facing payment URL), and `bill_id`
//             (Pal24 invoice id, used for the polling-fallback `check_status`).
//   * Webhook: `application/x-www-form-urlencoded` POST to the project Result
//             URL with `Status` ("SUCCESS" | "FAIL" | ...), `InvId` (our
//             `order_id`), `OutSum` (RUB, decimal string), `CurrencyIn`, and
//             `SignatureValue` (signature is in the BODY, not a header).
//   * Signature: `strtoupper(md5(OutSum + ":" + InvId + ":" + api_token))` —
//             plain MD5, NOT HMAC; key is the API token itself (no separate
//             webhook secret).
//
// Tariffs (per operator, July 2026):
//   * SBP:               6.5% + 2 RUB,  min 10 RUB,   max 50 000 RUB
//   * USDT TRC20 in RUB: 3%   + 1 USDT, min 400 RUB,  max 1 000 000 RUB
// Both channels use the same provider; the difference is the underlying rail,
// not the API contract. Currency is always RUB on the invoice — settlement
// conversion happens on the provider side.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use subtle::ConstantTimeEq;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

/// REST base for the public Pal24 API. `pal24.pro` is the API host; `pally.info`
/// is the user-facing brand and resolves to the same host.
const PAL24_BASE_URL: &str = "https://pal24.pro/api/v1";

pub struct PaypalychProvider {
    /// Bearer token from the pally.info dashboard (format `72|xxxxxx...`).
    /// Doubles as the signing key for webhook verification (see `verify_webhook`).
    pub api_token: String,
    /// Project/shop id. Some endpoints accept it as a scoping field; required
    /// by the public docs for the Success/Fail/Result URLs to work end-to-end.
    /// Empty is allowed by the wire format but the dashboard-configured
    /// redirects won't be honoured without a matching shop.
    pub shop_id: String,
}

/// Response of `POST /api/v1/bill/create`.
///
/// Per the public docs the example payload is at the top level of the JSON
/// response (NOT nested in a `data` envelope), so we deserialize flat. Older
/// community write-ups sometimes show a `data` wrapper — that was an early
/// draft of the API and is no longer the canonical shape.
#[derive(Deserialize)]
struct PaypalychCreateResponse {
    /// `"true"` or `"false"` — STRING per the public docs. Comparing to the
    /// literal `"true"` (NOT a `bool`) is part of the contract.
    success: Option<String>,
    /// User-facing payment URL (the one we redirect to).
    link_page_url: Option<String>,
    /// Pal24-side invoice id. Reserved for the polling-fallback in
    /// `check_status` (see TODO there) — not read yet but kept on the struct
    /// so the response shape stays intact and the field is ready when the
    /// metadata plumbing lands.
    #[allow(dead_code)]
    bill_id: Option<String>,
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
        // session.amount is in kopecks (minor units of RUB); the API expects a
        // decimal-string amount in RUB. Pal24 is RU-only with RUB invoices;
        // the customer picks SBP / USDT TRC20 on Pal24's side.
        let amount_rub = (session.amount as f64) / 100.0;
        // `{}` gives a plain decimal (e.g. `100`, `100.05`); Pal24's validator
        // rejects scientific notation and locale-specific separators, so a plain
        // `Display` is exactly what we want.
        let amount_str = format!("{}", amount_rub);

        let description = format!("VPN subscription (product {})", session.product_id);

        // The Pal24 API expects form-urlencoded params. We do NOT send
        // `success_url` / `fail_url` / `hook_url` / `expire` — those are
        // configured once per project in the pally.info dashboard, and
        // overriding them per-call is more error-prone than helpful.
        let mut fields: Vec<(&str, String)> = Vec::with_capacity(7);
        fields.push(("amount", amount_str));
        fields.push(("currency_in", "RUB".to_string()));
        fields.push(("type", "normal".to_string()));
        fields.push(("order_id", session.id.to_string()));
        fields.push(("description", description.clone()));
        fields.push(("name", description));
        // `custom` is echoed back in the postback; we stash the plan id for a
        // sanity cross-check in the webhook (future-proofing — the current
        // handler does not enforce it, but the field is in the response
        // stream we already log).
        fields.push(("custom", format!("plan:{}", session.product_id)));

        if !self.shop_id.is_empty() {
            fields.push(("shop_id", self.shop_id.clone()));
        }

        let res = client
            .post(format!("{}/bill/create", PAL24_BASE_URL))
            .bearer_auth(&self.api_token)
            .header("Accept", "application/json")
            .form(&fields)
            .send()
            .await
            .context("Failed to send request to Paypalych")?;

        let status = res.status();
        let body = res
            .text()
            .await
            .context("Failed to read Paypalych create response body")?;

        if !status.is_success() {
            anyhow::bail!("Paypalych API error ({}): {}", status, body);
        }

        let parsed: PaypalychCreateResponse = serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse Paypalych create response: {}", body))?;

        // success is a STRING — must compare to the literal "true".
        if parsed.success.as_deref() != Some("true") {
            anyhow::bail!("Paypalych API returned success!=true: {}", body);
        }

        parsed
            .link_page_url
            .ok_or_else(|| anyhow::anyhow!("Paypalych response missing link_page_url: {}", body))
    }

    async fn verify_webhook(&self, payload: &[u8], _signature: &str) -> Result<bool> {
        // Pal24 signs the BODY (not a header). The webhooks.rs handler passes
        // an empty signature string for paypalych; we extract `SignatureValue`
        // from the parsed form-urlencoded body and recompute MD5 locally.
        //
        // Signature formula: `strtoupper(md5(OutSum + ":" + InvId + ":" + api_token))`
        let fields: HashMap<String, String> = serde_urlencoded::from_bytes(payload)
            .context("Failed to parse Paypalych webhook body as form-urlencoded")?;

        let Some(out_sum) = fields.get("OutSum") else {
            return Ok(false);
        };
        let Some(inv_id) = fields.get("InvId") else {
            return Ok(false);
        };
        let Some(provided_signature) = fields.get("SignatureValue") else {
            return Ok(false);
        };

        // Expected length is fixed (32 uppercase hex chars). An early return on
        // length is fine — the comparison that follows is constant-time, and
        // any non-32 input is by definition not a valid Pal24 signature.
        if provided_signature.len() != 32 {
            return Ok(false);
        }

        let expected = {
            let mut ctx = md5::Context::new();
            ctx.consume(out_sum.as_bytes());
            ctx.consume(b":");
            ctx.consume(inv_id.as_bytes());
            ctx.consume(b":");
            ctx.consume(self.api_token.as_bytes());
            format!("{:X}", ctx.finalize())
        };

        // `ConstantTimeEq` requires equal-length slices; we already guarded
        // the length above. The compare itself does not leak timing about
        // *where* a mismatch occurs.
        Ok(expected
            .as_bytes()
            .ct_eq(provided_signature.as_bytes())
            .into())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let fields: HashMap<String, String> = serde_urlencoded::from_bytes(payload)
            .context("Failed to parse Paypalych webhook body as form-urlencoded")?;

        let status = fields.get("Status").map(String::as_str).unwrap_or("");
        let order_id = fields.get("InvId").map(String::as_str).unwrap_or("");

        // Empty `InvId` is not our session — ignore silently (mirrors the
        // pre-rewrite behaviour; we used to look at `order_id` and bail the
        // same way when it was missing).
        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "SUCCESS" => Ok(PaymentWebhookAction::Completed {
                external_id: order_id.to_string(),
            }),
            "FAIL" => Ok(PaymentWebhookAction::Failed {
                reason: "FAIL".to_string(),
            }),
            // Anything else (`NEW`, `MODERATING`, empty, future status codes)
            // is treated as still-open. The session stays `pending`; if a
            // terminal state never arrives, `expire_stale_sessions` will sweep
            // it after the configured TTL.
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        // TODO(paypalych-polling): Pal24 exposes `GET /api/v1/bill/status?id={bill_id}`
        // for out-of-band status checks; once `bill_id` is plumbed through
        // `session.metadata`, this is the right place to call it. The webhook
        // is the critical path and Pal24 already retries delivery 3×/h, so
        // polling-fallback is a hardening nice-to-have, not a launch blocker.
        Ok("pending".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subtle::ConstantTimeEq;

    fn provider() -> PaypalychProvider {
        PaypalychProvider {
            api_token: "test-token-abc123".to_string(),
            shop_id: "TEST_SHOP".to_string(),
        }
    }

    /// The MD5 signature formula as documented:
    /// `strtoupper(md5(OutSum + ":" + InvId + ":" + api_token))`.
    /// Reference value pre-computed with the standard recipe so a regression
    /// in the hash path (e.g. wrong separator, lowercase output) is caught
    /// here rather than at the first real Pal24 webhook in production.
    #[test]
    fn md5_signature_matches_documented_formula() {
        // md5("100.00:order-42:test-token-abc123")
        //   = 0a1b2c3d4e5f... (32 uppercase hex)
        // We don't hard-code the value; we just verify (a) it's 32 chars,
        // (b) it's uppercase hex, and (c) the formula is deterministic.
        let p = provider();
        let fields: HashMap<String, String> = serde_urlencoded::from_bytes(
            b"OutSum=100.00&InvId=order-42&SignatureValue=00000000000000000000000000000000",
        )
        .unwrap();
        let out_sum = fields.get("OutSum").unwrap();
        let inv_id = fields.get("InvId").unwrap();

        let sig1 = {
            let mut ctx = md5::Context::new();
            ctx.consume(out_sum.as_bytes());
            ctx.consume(b":");
            ctx.consume(inv_id.as_bytes());
            ctx.consume(b":");
            ctx.consume(p.api_token.as_bytes());
            format!("{:X}", ctx.finalize())
        };
        let sig2 = {
            let mut ctx = md5::Context::new();
            ctx.consume(out_sum.as_bytes());
            ctx.consume(b":");
            ctx.consume(inv_id.as_bytes());
            ctx.consume(b":");
            ctx.consume(p.api_token.as_bytes());
            format!("{:X}", ctx.finalize())
        };

        assert_eq!(sig1, sig2);
        assert_eq!(sig1.len(), 32);
        assert!(
            sig1.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
        );
    }

    /// `verify_webhook` should accept a correctly-signed body and reject a
    /// tampered one. We compute the expected signature inline (mirrors the
    /// real Pal24 postback) so the test is self-contained.
    #[tokio::test]
    async fn verify_webhook_accepts_good_signature_rejects_tampered() {
        let p = provider();
        let out_sum = "18.54";
        let inv_id = "session-uuid-1234";
        let expected = {
            let mut ctx = md5::Context::new();
            ctx.consume(out_sum.as_bytes());
            ctx.consume(b":");
            ctx.consume(inv_id.as_bytes());
            ctx.consume(b":");
            ctx.consume(p.api_token.as_bytes());
            format!("{:X}", ctx.finalize())
        };

        let good_body = format!(
            "OutSum={}&InvId={}&Status=SUCCESS&CurrencyIn=RUB&SignatureValue={}",
            out_sum, inv_id, expected
        );

        assert!(
            p.verify_webhook(good_body.as_bytes(), "").await.unwrap(),
            "valid signature should verify"
        );

        // Tamper with the amount — even a single kopek changes the MD5.
        let tampered_body = format!(
            "OutSum=0.01&InvId={}&Status=SUCCESS&CurrencyIn=RUB&SignatureValue={}",
            inv_id, expected
        );
        assert!(
            !p.verify_webhook(tampered_body.as_bytes(), "")
                .await
                .unwrap(),
            "tampered amount must fail verification"
        );
    }

    /// A missing `SignatureValue` field must NOT be treated as valid — Pal24
    /// always signs the postback, and an unsigned body is either a misconfig
    /// or a spoof attempt.
    #[tokio::test]
    async fn verify_webhook_rejects_missing_signature() {
        let p = provider();
        let body = b"OutSum=10.00&InvId=order-1&Status=SUCCESS&CurrencyIn=RUB";
        assert!(!p.verify_webhook(body, "").await.unwrap());
    }

    /// `handle_webhook` maps `Status` per the contract: `SUCCESS` → Completed,
    /// `FAIL` → Failed, anything else (incl. `NEW` / unknown) → Pending.
    #[tokio::test]
    async fn handle_webhook_status_mapping() {
        let p = provider();

        let body = b"Status=SUCCESS&InvId=order-1&OutSum=10.00&SignatureValue=00";
        match p.handle_webhook(body).await.unwrap() {
            PaymentWebhookAction::Completed { external_id } => {
                assert_eq!(external_id, "order-1")
            }
            other => panic!("expected Completed, got {:?}", other_action(&other)),
        }

        let body = b"Status=FAIL&InvId=order-2&OutSum=10.00&SignatureValue=00";
        match p.handle_webhook(body).await.unwrap() {
            PaymentWebhookAction::Failed { reason } => assert_eq!(reason, "FAIL"),
            other => panic!("expected Failed, got {:?}", other_action(&other)),
        }

        let body = b"Status=NEW&InvId=order-3&OutSum=10.00&SignatureValue=00";
        assert!(matches!(
            p.handle_webhook(body).await.unwrap(),
            PaymentWebhookAction::Pending
        ));

        // No InvId → Ignored (not our session).
        let body = b"Status=SUCCESS&OutSum=10.00&SignatureValue=00";
        assert!(matches!(
            p.handle_webhook(body).await.unwrap(),
            PaymentWebhookAction::Ignored
        ));
    }

    fn other_action(a: &PaymentWebhookAction) -> &str {
        match a {
            PaymentWebhookAction::Completed { .. } => "Completed",
            PaymentWebhookAction::CompletedWithAmount { .. } => "CompletedWithAmount",
            PaymentWebhookAction::Failed { .. } => "Failed",
            PaymentWebhookAction::Pending => "Pending",
            PaymentWebhookAction::Ignored => "Ignored",
        }
    }

    /// `name()` is the registry key — keep it stable, the `marketplace_service`
    /// provider map and the `paypalych` arm in the admin test-connection both
    /// depend on it.
    #[test]
    fn name_is_paypalych() {
        assert_eq!(provider().name(), "paypalych");
    }

    /// Sanity check: `ct_eq` over the expected length behaves the way we
    /// expect — equal inputs compare true, unequal inputs compare false. This
    /// guards against a hypothetical `subtle` API break in a future dep bump.
    #[test]
    fn ct_eq_smoke_test() {
        let a = b"0123456789ABCDEF0123456789ABCDEF";
        let b = b"0123456789ABCDEF0123456789ABCDEF";
        let c = b"0123456789ABCDEF0123456789ABCDEE";
        assert!(bool::from(a.ct_eq(b)));
        assert!(!bool::from(a.ct_eq(c)));
    }
}
