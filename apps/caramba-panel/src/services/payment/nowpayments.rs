use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha512;
use subtle::ConstantTimeEq;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct NowPaymentsInvoiceReq {
    price_amount: f64,
    price_currency: String,
    order_id: String,
    order_description: String,
    ipn_callback_url: String,
    success_url: String,
    cancel_url: String,
}

#[derive(Deserialize)]
struct NowPaymentsInvoiceRes {
    invoice_url: String,
}

pub struct NowPaymentsProvider {
    pub api_key: String,
    pub ipn_secret: String,
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for NowPaymentsProvider {
    fn name(&self) -> &str {
        "nowpayments"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        // No `pay_currency`: omitting it lets the customer pick any coin/network on
        // the NOWPayments hosted page, which hides coins whose per-network minimum
        // exceeds the order amount. Pinning a single network (the old USDTTRC20
        // hardcode) priced low-ticket CIS subscriptions out of viable networks.
        let req_body = NowPaymentsInvoiceReq {
            price_amount: (session.amount as f64) / 100.0, // Amount is in cents
            price_currency: session.currency.to_uppercase(),
            order_id: session.id.to_string(),
            order_description: invoice_description(session, user),
            ipn_callback_url: format!(
                "https://{}/api/webhooks/payment/nowpayments",
                self.api_domain
            ),
            success_url: format!("https://t.me/{}", self.bot_username),
            cancel_url: format!("https://t.me/{}", self.bot_username),
        };

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let res = client
            .post("https://api.nowpayments.io/v1/invoice")
            .headers(headers)
            .json(&req_body)
            .send()
            .await
            .context("Failed to send request to NowPayments")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("NowPayments API Error: {}", error_text);
        }

        let invoice: NowPaymentsInvoiceRes = res
            .json()
            .await
            .context("Failed to parse NowPayments response")?;
        Ok(invoice.invoice_url)
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // NOWPayments IPN signature (official spec, confirmed against the
        // HelpCenter "IPN and how to setup" article and the official JS/PHP/Python
        // examples):
        //   1. Take the IPN JSON body and sort its keys ALPHABETICALLY — recursively,
        //      so nested objects are sorted too (official JS uses a recursive
        //      `sortObject`; PHP uses a recursive `tksort`; Python uses
        //      `json.dumps(..., sort_keys=True, separators=(',', ':'))`).
        //   2. Serialize the sorted object to a COMPACT JSON string (no extra
        //      whitespace, separators "," and ":").
        //   3. HMAC-SHA512 that string with the IPN secret as the key.
        //   4. Hex-encode and compare (case-sensitive) with the `x-nowpayments-sig`
        //      request header — which the router passes in as `signature`.
        //
        // We do NOT hash the raw wire bytes: NOWPayments signs the *sorted*
        // serialization, and the bytes we receive are not guaranteed to be in that
        // order. Re-parsing into `serde_json::Value` and re-serializing reproduces
        // exactly the sorted/compact form, because this workspace does NOT enable
        // serde_json's `preserve_order` feature — `Value::Object` is a BTreeMap, so
        // `to_string` emits keys sorted recursively with compact separators, matching
        // the official Python `json.dumps(sort_keys=True, separators=(',', ':'))`.

        // An empty/missing header can never match a real HMAC; reject early.
        if signature.is_empty() {
            return Ok(false);
        }

        let data: Value =
            serde_json::from_slice(payload).context("Invalid JSON in NowPayments IPN webhook")?;

        // serde_json (no preserve_order) serializes object keys in sorted order,
        // recursively, with compact separators — exactly the canonical form
        // NowPayments signs.
        let canonical = serde_json::to_string(&data)
            .context("Failed to serialize NowPayments IPN body for verification")?;

        type HmacSha512 = Hmac<Sha512>;
        let mut mac =
            HmacSha512::new_from_slice(self.ipn_secret.as_bytes()).context("Invalid HMAC key")?;
        mac.update(canonical.as_bytes());
        let computed_sig = hex::encode(mac.finalize().into_bytes());

        // Constant-time comparison to avoid leaking timing information about the
        // expected signature.
        Ok(computed_sig.as_bytes().ct_eq(signature.as_bytes()).into())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;

        let status = data
            .get("payment_status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let order_id = data.get("order_id").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        // Official NowPayments payment_status enum (HelpCenter "Payment statuses"):
        //   waiting, confirming, confirmed, sending, partially_paid, finished,
        //   failed, refunded, expired.
        // There is NO "completed" status — that literal was bogus and is dropped.
        //   - finished  : funds reached the merchant address — payment complete.
        //   - confirmed : blockchain confirmations accumulated, funds en route to the
        //                 merchant; treated as success for VPN provisioning so the user
        //                 isn't blocked if a later `sending`/`finished` IPN is missed.
        //   - failed/refunded/expired : terminal failure.
        //   - everything else (waiting/confirming/sending/partially_paid) : pending.
        match status {
            "finished" | "confirmed" => {
                // U18: emit the INVOICE fiat amount/currency so MarketplaceService
                // verifies the paid magnitude before granting access. NowPayments
                // echoes `price_amount` (the fiat amount we charged) in
                // `price_currency` — directly comparable to the session, which is
                // priced in the same currency. We deliberately use price_amount, NOT
                // `actually_paid` (which is denominated in the crypto pay-currency and
                // is not comparable to a USD-cent session amount).
                //
                // Missing/unparseable fields degrade to the legacy `Completed` (no
                // amount check) so a real payment is never wrongly rejected.
                let invoice_amount = data.get("price_amount").and_then(parse_decimal_to_minor);
                let invoice_currency = data
                    .get("price_currency")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                match (invoice_amount, invoice_currency) {
                    (Some(paid_amount_minor), Some(paid_currency)) => {
                        Ok(PaymentWebhookAction::CompletedWithAmount {
                            external_id: order_id.to_string(),
                            paid_amount_minor,
                            paid_currency,
                        })
                    }
                    _ => Ok(PaymentWebhookAction::Completed {
                        external_id: order_id.to_string(),
                    }),
                }
            }
            "failed" | "expired" | "refunded" => Ok(PaymentWebhookAction::Failed {
                reason: status.to_string(),
            }),
            _ => Ok(PaymentWebhookAction::Pending),
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

/// Build the text the customer sees on the hosted payment page and the operator
/// sees in the NOWPayments dashboard. The dashboard shows only `order_id` (an
/// opaque uuid) and this line, so it has to carry enough to reconcile a late or
/// disputed payment without a database lookup: what was bought, for how long,
/// and by whom.
fn invoice_description(session: &PaymentSession, user: &User) -> String {
    let plan = session
        .metadata
        .as_ref()
        .and_then(|m| m.get("resource_label"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("VPN subscription");

    let days = session
        .metadata
        .as_ref()
        .and_then(|m| m.get("duration_days"))
        .and_then(|v| v.as_i64());

    // Telegram handle when we have one; the numeric id always resolves in the
    // admin panel, so support is never left guessing which account paid.
    let who = match user.username.as_deref().filter(|u| !u.is_empty()) {
        Some(handle) => format!("@{}", handle),
        None => format!("id{}", user.tg_id),
    };

    match days {
        Some(d) => format!("{} — {} days · {}", plan, d, who),
        None => format!("{} · {}", plan, who),
    }
}

/// Parse a JSON amount (string like `"10.00"` or a JSON number) into MINOR units
/// (×100, rounded) for the U18 amount-verification gate. Returns `None` on absent
/// or unparseable input so the caller falls back to the legacy `Completed` path
/// rather than reject a real payment.
fn parse_decimal_to_minor(v: &Value) -> Option<i64> {
    let major = match v {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    if !major.is_finite() || major < 0.0 {
        return None;
    }
    Some((major * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::invoice_description;
    use caramba_db::models::store::{PaymentSession, User};
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn user(username: Option<&str>) -> User {
        User {
            id: 1,
            tg_id: 316766484,
            username: username.map(str::to_string),
            full_name: None,
            balance: 0,
            referral_code: None,
            referrer_id: None,
            referred_by: None,
            is_banned: false,
            language_code: None,
            terms_accepted_at: None,
            warning_count: 0,
            trial_used: None,
            trial_used_at: None,
            last_bot_msg_id: None,
            created_at: Utc::now(),
            parent_id: None,
        }
    }

    fn session(metadata: Option<serde_json::Value>) -> PaymentSession {
        PaymentSession {
            id: Uuid::nil(),
            user_id: 1,
            product_id: 1,
            provider: "nowpayments".to_string(),
            external_id: None,
            amount: 1500,
            currency: "USD".to_string(),
            status: "pending".to_string(),
            metadata,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn description_names_the_plan_the_duration_and_the_buyer() {
        let s = session(Some(
            json!({"type": "plan", "resource_label": "Gold", "duration_days": 180}),
        ));
        assert_eq!(
            invoice_description(&s, &user(Some("art"))),
            "Gold — 180 days · @art"
        );
    }

    #[test]
    fn description_falls_back_to_the_numeric_id_without_a_handle() {
        let s = session(Some(
            json!({"resource_label": "Gold", "duration_days": 360}),
        ));
        assert_eq!(
            invoice_description(&s, &user(None)),
            "Gold — 360 days · id316766484"
        );
    }

    #[test]
    fn description_survives_missing_metadata() {
        // Older sessions carry no label; the line must still identify the buyer
        // rather than degrade to the useless "Product: 1" of the old format.
        let out = invoice_description(&session(None), &user(Some("art")));
        assert_eq!(out, "VPN subscription · @art");
        assert!(!out.contains("Product:"));
    }

    #[test]
    fn description_ignores_a_blank_label() {
        let s = session(Some(json!({"resource_label": "   ", "duration_days": 30})));
        assert_eq!(
            invoice_description(&s, &user(Some("art"))),
            "VPN subscription — 30 days · @art"
        );
    }
}
