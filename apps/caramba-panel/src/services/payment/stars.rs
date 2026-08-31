//! Telegram Stars (XTR) payment provider.
//!
//! Stars are Telegram's in-app currency. Unlike every other provider in this
//! module there is no merchant account and no HTTP webhook: the invoice is
//! created with the Bot API (`createInvoiceLink`, `provider_token: ""`,
//! `currency: "XTR"`) and the *only* confirmation Telegram ever sends is a
//! `successful_payment` service message delivered to the bot's own update
//! stream (see `bot/handlers/command.rs`).
//!
//! Historically that made Stars the odd one out: the Mini App advertised
//! "Pay with Telegram Stars" but `MarketplaceService` had no `stars` provider,
//! so `POST /api/client/payment/invoice` failed with "Payment provider not
//! found or disabled" for every Stars purchase. This provider closes that gap
//! by routing Stars through the *same* `PaymentSession` machinery as every
//! other method, which is what gives it plan durations, referral rewards,
//! idempotent fulfillment and the "payment received" DM for free.
//!
//! The bridge between "Telegram tells the bot a payment happened" and "which
//! session was that?" is the invoice payload: this provider stamps the session
//! UUID into it as `sess:{uuid}` (see [`session_invoice_payload`]). The bot's
//! `successful_payment` handler parses it and calls
//! `MarketplaceService::fulfill_payment`. Legacy bot-native flows (balance
//! top-up, the `/buy` keyboard) keep using the older
//! `{user_id}:bal|ord|sub:{id}` payload, which can never be confused with this
//! one because it never starts with `sess:`.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use crate::settings::SettingsService;

/// Telegram Stars (XTR) charged per 1 USD — the built-in FALLBACK rate.
///
/// The live rate is the `stars_per_usd` setting (admin Settings page); this
/// constant is what every helper falls back to when the setting is unset,
/// unparseable or out of bounds. Every place that turns money into Stars or
/// back (`PayService::create_stars_invoice`, the bot's `pay_dur_stars_*`
/// keyboard, the `successful_payment` amount gate) must go through the helpers
/// below with a rate resolved from settings rather than re-hardcoding `50`.
pub const STARS_PER_USD: i64 = 50;

/// Settings key holding the operator-tuned Stars-per-USD rate.
pub const STARS_PER_USD_SETTING: &str = "stars_per_usd";

/// Lower bound for `stars_per_usd`. A rate of 0 or less would price everything
/// at zero Stars (or negative), which Telegram rejects and which would let a
/// purchase through for free.
pub const MIN_STARS_PER_USD: i64 = 1;

/// Upper bound for `stars_per_usd`. 1000 XTR per USD is ~20× the Telegram
/// sticker rate — far beyond any sane markup, so anything above it is a typo
/// (an extra zero) rather than an intent.
pub const MAX_STARS_PER_USD: i64 = 1000;

/// Metadata key under which the expected Stars amount is frozen into the
/// payment session at invoice-creation time.
///
/// Without it the amount gate would recompute the expectation from the CURRENT
/// rate: an operator raising `stars_per_usd` between invoice creation and
/// payment would make every open invoice look underpaid and honest payments
/// would be refused. See [`expected_session_star_amount`].
pub const SESSION_STARS_METADATA_KEY: &str = "stars_amount";

/// Parse the raw `stars_per_usd` setting value, falling back to
/// [`STARS_PER_USD`] for anything missing, unparseable or out of bounds.
///
/// Pure on purpose: the whole "which rate applies" decision is testable without
/// a database, and both the settings-backed reader below and the admin form's
/// validation share it.
pub fn parse_stars_per_usd(raw: Option<&str>) -> i64 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|rate| (MIN_STARS_PER_USD..=MAX_STARS_PER_USD).contains(rate))
        .unwrap_or(STARS_PER_USD)
}

/// The live Stars-per-USD rate from settings (cached in `SettingsService`).
pub async fn stars_per_usd(settings: &SettingsService) -> i64 {
    parse_stars_per_usd(settings.get(STARS_PER_USD_SETTING).await.as_deref())
}

/// Per-invoice sanity cap in Stars. 1 000 000 XTR ≈ $20 000 — orders of
/// magnitude above any real subscription, so it only ever blocks absurd or
/// forged amounts. Shared with the pre-checkout gate.
pub const MAX_STARS_PER_INVOICE: i64 = 1_000_000;

/// Prefix that marks an invoice payload as "a `payment_sessions` row id".
///
/// Deliberately non-numeric so it can never collide with the legacy payload
/// shape `{user_id}:bal|ord|sub:{id}`, whose first segment is always a decimal
/// user id.
pub const SESSION_PAYLOAD_PREFIX: &str = "sess:";

/// Build the Telegram invoice payload for a marketplace payment session.
pub fn session_invoice_payload(session_id: Uuid) -> String {
    format!("{SESSION_PAYLOAD_PREFIX}{session_id}")
}

/// Parse an invoice payload back into a payment-session id.
///
/// Returns `None` for anything that is not the new session form — in
/// particular for every legacy `{user_id}:bal|ord|sub:{id}` payload — so
/// callers can use it as an unambiguous "is this the new flow?" test.
pub fn parse_session_invoice_payload(payload: &str) -> Option<Uuid> {
    payload
        .strip_prefix(SESSION_PAYLOAD_PREFIX)?
        .trim()
        .parse::<Uuid>()
        .ok()
}

/// USD minor units (cents) → whole Stars at `stars_per_usd`, rounding UP.
///
/// Rounding up (never down) keeps us from ever charging less than the priced
/// amount. Non-positive input yields 0 so callers can reject it explicitly.
/// The rate is passed in (never read from a constant) so a single request can
/// never mix two different rates.
pub fn usd_cents_to_stars(amount_cents: i64, stars_per_usd: i64) -> i64 {
    if amount_cents <= 0 || stars_per_usd <= 0 {
        return 0;
    }
    // ceil(cents * stars_per_usd / 100) without relying on unstable signed
    // `div_ceil`; saturating so an absurd amount cannot overflow (it will be
    // rejected by the MAX_STARS_PER_INVOICE cap anyway).
    amount_cents
        .saturating_mul(stars_per_usd)
        .saturating_add(99)
        / 100
}

/// Whole Stars → USD minor units (cents) at `stars_per_usd`.
///
/// Exact for every amount produced by [`usd_cents_to_stars`] from a whole
/// number of cents that is a multiple of the star granularity (at the default
/// rate of 50, one star is exactly 2 cents), so a cents → stars → cents round
/// trip of a real price never drifts.
pub fn stars_to_usd_cents(stars: i64, stars_per_usd: i64) -> i64 {
    if stars <= 0 || stars_per_usd <= 0 {
        return 0;
    }
    stars.saturating_mul(100) / stars_per_usd
}

/// The Stars amount frozen into the session's metadata at invoice creation, if
/// any. `None` for sessions created before this field existed (and for garbage
/// or non-positive values, which must never be trusted as an expectation).
pub fn persisted_session_star_amount(session: &PaymentSession) -> Option<i64> {
    session
        .metadata
        .as_ref()
        .and_then(|meta| meta.get(SESSION_STARS_METADATA_KEY))
        .and_then(Value::as_i64)
        .filter(|stars| *stars > 0 && *stars <= MAX_STARS_PER_INVOICE)
}

/// Freeze `stars` into a session's metadata under [`SESSION_STARS_METADATA_KEY`].
///
/// Creates the metadata object when the session had none. That is safe for the
/// downstream consumers: `payment_resource_type` treats a missing `type` in an
/// object exactly like absent metadata (both default to `"product"`).
pub fn stamp_session_star_amount(metadata: Option<Value>, stars: i64) -> Option<Value> {
    let mut object = match metadata {
        Some(Value::Object(map)) => map,
        // Non-object metadata never reaches here (`normalize_payment_metadata`
        // rejects it), but rather than silently dropping it we leave it alone.
        Some(other) => return Some(other),
        None => serde_json::Map::new(),
    };
    object.insert(
        SESSION_STARS_METADATA_KEY.to_string(),
        Value::from(stars.max(0)),
    );
    Some(Value::Object(object))
}

/// The Stars amount a session must be paid with, or a descriptive error.
///
/// **This is the amount gate's expectation.** It prefers the value frozen into
/// the session's metadata when the invoice was created and only computes a
/// fresh one (at the current rate) for older sessions that predate the frozen
/// field. That ordering is what keeps an open invoice payable after the
/// operator changes `stars_per_usd`: the buyer is charged what the invoice
/// said, so the gate must expect what the invoice said.
pub fn expected_session_star_amount(session: &PaymentSession, stars_per_usd: i64) -> Result<i64> {
    if let Some(stars) = persisted_session_star_amount(session) {
        return Ok(stars);
    }
    session_star_amount(session, stars_per_usd)
}

/// Price a session in Stars at `stars_per_usd`, ignoring anything already
/// frozen into its metadata.
///
/// Used at invoice creation (to produce the amount we then freeze) and as the
/// fallback of [`expected_session_star_amount`] for pre-existing sessions.
///
/// Only USD-priced sessions are convertible. A per-provider price override in
/// another currency is an operator misconfiguration, not something we can
/// silently guess a Star price for, so it fails loudly.
pub fn session_star_amount(session: &PaymentSession, stars_per_usd: i64) -> Result<i64> {
    if !session.currency.eq_ignore_ascii_case("USD") {
        anyhow::bail!(
            "Telegram Stars can only price USD sessions, got '{}' (remove the per-provider \
             price override for 'stars', or price it in USD)",
            session.currency
        );
    }

    let stars = usd_cents_to_stars(session.amount, stars_per_usd);
    if stars <= 0 {
        anyhow::bail!(
            "Session amount {} {} is too small to be paid with Telegram Stars",
            session.amount,
            session.currency
        );
    }
    if stars > MAX_STARS_PER_INVOICE {
        anyhow::bail!(
            "Session amount {} {} exceeds the Telegram Stars per-invoice cap ({} XTR)",
            session.amount,
            session.currency,
            MAX_STARS_PER_INVOICE
        );
    }

    Ok(stars)
}

/// Invoice `title` (≤ 32 chars per Bot API) and price `label` for a session,
/// derived from the metadata `type` written by the checkout endpoint.
fn invoice_texts(session: &PaymentSession) -> (&'static str, &'static str) {
    let kind = session
        .metadata
        .as_ref()
        .and_then(|meta| meta.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or("product");

    match kind {
        "plan" => ("VPN Subscription", "Subscription"),
        "order" => ("Store Order", "Order"),
        _ => ("Purchase", "Purchase"),
    }
}

/// Human-readable invoice `description` (≤ 255 chars per Bot API).
fn invoice_description(session: &PaymentSession) -> String {
    let usd = session.amount as f64 / 100.0;
    let days = session
        .metadata
        .as_ref()
        .and_then(|meta| meta.get("duration_days"))
        .and_then(|value| value.as_i64());

    match days {
        Some(days) if days > 0 => format!("{days} days of VPN access — ${usd:.2}"),
        _ => format!("Payment of ${usd:.2}"),
    }
}

#[derive(Deserialize)]
struct TelegramInvoiceLinkResponse {
    ok: bool,
    result: Option<String>,
    description: Option<String>,
}

/// Telegram Stars provider for `MarketplaceService`.
///
/// Holds the bot token because `createInvoiceLink` is a Bot API call. The token
/// is read from settings in `main.rs` and handed to `MarketplaceService::new`
/// like every other provider credential — this type never touches env vars and
/// never logs the token (Telegram request errors are stripped of their URL,
/// which embeds it).
///
/// It also holds the `SettingsService` so the Stars-per-USD rate is read LIVE
/// on every invoice: the operator can retune `stars_per_usd` without a restart.
pub struct StarsProvider {
    pub bot_token: String,
    pub settings: Arc<SettingsService>,
}

#[async_trait]
impl PaymentProvider for StarsProvider {
    fn name(&self) -> &str {
        "stars"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        if self.bot_token.trim().is_empty() {
            anyhow::bail!("Bot token is required to create a Telegram Stars invoice");
        }

        // Prefer the amount frozen into the session by `create_session`: the
        // invoice and the amount gate must quote the same number even if the
        // operator retunes the rate between the two.
        let stars = expected_session_star_amount(session, stars_per_usd(&self.settings).await)?;
        let (title, label) = invoice_texts(session);
        let description = invoice_description(session);

        // The session id IS the payload: `successful_payment` gives us nothing
        // else to correlate the charge with, and the session is what carries the
        // product, the amount and (for plans) the purchased duration.
        let payload = session_invoice_payload(session.id);

        let params = serde_json::json!({
            "title": title,
            "description": description,
            "payload": payload,
            // Stars invoices are settled by Telegram itself: no payment provider
            // token, and the currency must literally be "XTR".
            "provider_token": "",
            "currency": "XTR",
            "prices": [{ "label": label, "amount": stars }],
        });

        let url = format!(
            "https://api.telegram.org/bot{}/createInvoiceLink",
            self.bot_token
        );

        // `without_url()` strips the request URL from the error — it contains the
        // bot token, which must never reach the logs.
        let res = client.post(&url).json(&params).send().await.map_err(|e| {
            anyhow::anyhow!(
                "Telegram createInvoiceLink request failed: {}",
                e.without_url()
            )
        })?;

        let body: TelegramInvoiceLinkResponse = res
            .json()
            .await
            .context("Failed to parse Telegram createInvoiceLink response")?;

        if !body.ok {
            anyhow::bail!(
                "Telegram rejected createInvoiceLink: {}",
                body.description.unwrap_or_else(|| "unknown error".into())
            );
        }

        body.result
            .context("Telegram returned ok=true for createInvoiceLink but no invoice link (result)")
    }

    // ---- Webhook surface: not applicable to Telegram Stars ----------------
    //
    // Stars never arrive over HTTP. Telegram confirms the charge as a
    // `successful_payment` message on the bot's update stream, which
    // `bot/handlers/command.rs` turns into `fulfill_payment`. There is no
    // `/api/webhooks/payment/stars` integration to configure, so these methods
    // exist only to satisfy the trait: they must never panic and must never
    // fulfill anything.

    async fn verify_webhook(&self, _payload: &[u8], _signature: &str) -> Result<bool> {
        // Nothing to verify — `handle_webhook` below ignores every body anyway,
        // so accepting here cannot grant anything. Returning `true` keeps a
        // stray POST from being logged as a (misleading) signature failure.
        Ok(true)
    }

    async fn handle_webhook(&self, _payload: &[u8]) -> Result<PaymentWebhookAction> {
        // Not applicable: the real confirmation is the bot's successful_payment
        // update. Anything posted to an HTTP endpoint for "stars" is spurious.
        Ok(PaymentWebhookAction::Ignored)
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        // No poll endpoint exists for Stars invoices. Per the trait contract a
        // constant "pending" makes the lost-webhook poller a no-op.
        Ok("pending".to_string())
    }

    fn supports_polling(&self) -> bool {
        // Off-chain / in-Telegram method: an HTTP poll can never confirm it.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn session(amount: i64, currency: &str, metadata: Option<serde_json::Value>) -> PaymentSession {
        PaymentSession {
            id: Uuid::new_v4(),
            user_id: 1,
            product_id: 2,
            provider: "stars".to_string(),
            external_id: None,
            amount,
            currency: currency.to_string(),
            status: "pending".to_string(),
            metadata,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // ---- payload round-trip ------------------------------------------------

    #[test]
    fn session_payload_round_trips() {
        let id = Uuid::new_v4();
        let payload = session_invoice_payload(id);
        assert!(payload.starts_with("sess:"));
        assert_eq!(parse_session_invoice_payload(&payload), Some(id));
    }

    #[test]
    fn legacy_payloads_are_not_parsed_as_sessions() {
        // The three legacy shapes produced by `PaymentType::to_payload_string`
        // plus the bare-user-id fallback accepted by `process_any_payment`.
        for legacy in [
            "42:bal:0",
            "42:ord:7",
            "42:sub:3",
            "42",
            "",
            "sess:",
            "sess:not-a-uuid",
            // A near-miss prefix must not be accepted either.
            "session:00000000-0000-0000-0000-000000000000",
        ] {
            assert_eq!(
                parse_session_invoice_payload(legacy),
                None,
                "payload {legacy:?} must not parse as a session id"
            );
        }
    }

    #[test]
    fn session_payload_is_never_a_legacy_payload() {
        // Structural guarantee behind the dispatch in the successful_payment
        // handler: the new payload can never look like `{user}:bal|ord|sub:{id}`
        // because its first segment is not a number.
        let payload = session_invoice_payload(Uuid::new_v4());
        let first_segment = payload.split(':').next().unwrap();
        assert!(first_segment.parse::<i64>().is_err());
    }

    // ---- configurable rate -------------------------------------------------

    #[test]
    fn rate_setting_falls_back_to_the_constant_when_unusable() {
        for raw in [
            None,
            Some(""),
            Some("   "),
            Some("abc"),
            Some("0"),
            Some("-50"),
            Some("50.5"),
            // Above MAX_STARS_PER_USD — almost certainly a stray zero.
            Some("1001"),
            Some("100000"),
        ] {
            assert_eq!(
                parse_stars_per_usd(raw),
                STARS_PER_USD,
                "raw {raw:?} must fall back to the built-in rate"
            );
        }
    }

    #[test]
    fn rate_setting_is_honoured_inside_bounds() {
        assert_eq!(parse_stars_per_usd(Some("1")), MIN_STARS_PER_USD);
        assert_eq!(parse_stars_per_usd(Some("75")), 75);
        assert_eq!(parse_stars_per_usd(Some(" 120 ")), 120);
        assert_eq!(parse_stars_per_usd(Some("1000")), MAX_STARS_PER_USD);
    }

    // ---- conversion --------------------------------------------------------

    #[test]
    fn usd_to_stars_at_real_price_points() {
        assert_eq!(usd_cents_to_stars(300, 50), 150); // $3.00
        assert_eq!(usd_cents_to_stars(500, 50), 250); // $5.00
        assert_eq!(usd_cents_to_stars(2500, 50), 1250); // $25.00
        assert_eq!(usd_cents_to_stars(100, 50), 50); // $1.00
    }

    #[test]
    fn usd_to_stars_scales_with_the_configured_rate() {
        // Raising the rate raises what the BUYER pays in Stars for the same
        // USD price — the whole point of the setting.
        assert_eq!(usd_cents_to_stars(300, 50), 150);
        assert_eq!(usd_cents_to_stars(300, 75), 225);
        assert_eq!(usd_cents_to_stars(300, 100), 300);
        // A non-positive rate can never yield a chargeable amount.
        assert_eq!(usd_cents_to_stars(300, 0), 0);
        assert_eq!(usd_cents_to_stars(300, -10), 0);
    }

    #[test]
    fn stars_to_usd_at_real_price_points() {
        assert_eq!(stars_to_usd_cents(150, 50), 300);
        assert_eq!(stars_to_usd_cents(250, 50), 500);
        assert_eq!(stars_to_usd_cents(1250, 50), 2500);
        // Same star count is worth less USD at a higher rate.
        assert_eq!(stars_to_usd_cents(150, 75), 200);
    }

    #[test]
    fn conversion_round_trips_without_drift() {
        // Any price the gate will ever see must survive cents → stars → cents
        // unchanged, otherwise the amount check would reject honest payments.
        for cents in [100, 200, 300, 500, 990, 1000, 1500, 2500, 5000, 9900] {
            let stars = usd_cents_to_stars(cents, STARS_PER_USD);
            assert_eq!(
                stars_to_usd_cents(stars, STARS_PER_USD),
                cents,
                "round trip drifted for {cents} cents"
            );
        }
    }

    #[test]
    fn odd_cents_round_up_never_down() {
        // One star == 2 cents, so an odd cent amount cannot be represented
        // exactly. We must always ask for MORE, never less, than the price.
        assert_eq!(usd_cents_to_stars(301, 50), 151);
        assert!(stars_to_usd_cents(usd_cents_to_stars(301, 50), 50) >= 301);
        assert_eq!(usd_cents_to_stars(1, 50), 1);
        assert!(stars_to_usd_cents(usd_cents_to_stars(1, 50), 50) >= 1);
    }

    #[test]
    fn non_positive_amounts_convert_to_zero() {
        assert_eq!(usd_cents_to_stars(0, 50), 0);
        assert_eq!(usd_cents_to_stars(-500, 50), 0);
        assert_eq!(stars_to_usd_cents(0, 50), 0);
        assert_eq!(stars_to_usd_cents(-10, 50), 0);
    }

    // ---- session gate ------------------------------------------------------

    #[test]
    fn session_star_amount_uses_the_supplied_rate() {
        let s = session(
            300,
            "USD",
            Some(json!({"type": "plan", "duration_days": 30})),
        );
        assert_eq!(session_star_amount(&s, 50).unwrap(), 150);
        assert_eq!(session_star_amount(&s, 100).unwrap(), 300);
    }

    #[test]
    fn session_star_amount_is_case_insensitive_about_usd() {
        let s = session(500, "usd", None);
        assert_eq!(session_star_amount(&s, 50).unwrap(), 250);
    }

    #[test]
    fn session_star_amount_rejects_foreign_currency() {
        let s = session(50000, "RUB", None);
        assert!(session_star_amount(&s, 50).is_err());
    }

    #[test]
    fn session_star_amount_rejects_zero_and_absurd_amounts() {
        assert!(session_star_amount(&session(0, "USD", None), 50).is_err());
        assert!(session_star_amount(&session(-100, "USD", None), 50).is_err());
        // Just over the 1_000_000 XTR cap ($20 000 = 2 000 000 cents).
        assert!(session_star_amount(&session(2_000_100, "USD", None), 50).is_err());
        // Right at the cap is still fine.
        assert_eq!(
            session_star_amount(&session(2_000_000, "USD", None), 50).unwrap(),
            MAX_STARS_PER_INVOICE
        );
    }

    // ---- frozen expectation (rate changes mid-flight) ----------------------

    #[test]
    fn a_rate_change_cannot_invalidate_an_open_session() {
        // The regression this whole mechanism exists for. An invoice is created
        // at rate X; the operator then raises the rate to Y; the buyer pays the
        // amount the invoice quoted. The gate MUST still expect X's amount.
        let rate_at_creation = 50;
        let rate_after_change = 200;

        let priced = session(
            300,
            "USD",
            Some(json!({"type": "plan", "duration_days": 30})),
        );
        let quoted = session_star_amount(&priced, rate_at_creation).unwrap();
        assert_eq!(quoted, 150);

        // What `create_session` persists.
        let mut open = priced.clone();
        open.metadata = stamp_session_star_amount(open.metadata.take(), quoted);

        // The pre-checkout gate and the successful_payment gate, both now
        // running under the NEW rate, still expect the quoted amount.
        assert_eq!(
            expected_session_star_amount(&open, rate_after_change).unwrap(),
            quoted
        );
        // Sanity: without the frozen value the expectation WOULD have moved,
        // which is exactly the bug being pinned.
        assert_eq!(
            session_star_amount(&priced, rate_after_change).unwrap(),
            600
        );
        // …and a payment of the quoted amount is no longer an underpayment.
        assert!(quoted >= expected_session_star_amount(&open, rate_after_change).unwrap());
    }

    #[test]
    fn stamping_preserves_the_rest_of_the_metadata() {
        let stamped =
            stamp_session_star_amount(Some(json!({"type": "plan", "duration_days": 30})), 150)
                .unwrap();
        assert_eq!(stamped["type"], "plan");
        assert_eq!(stamped["duration_days"], 30);
        assert_eq!(stamped[SESSION_STARS_METADATA_KEY], 150);
    }

    #[test]
    fn stamping_creates_metadata_when_the_session_had_none() {
        let stamped = stamp_session_star_amount(None, 250).unwrap();
        assert!(stamped.is_object());
        // No `type` key — which `payment_resource_type` reads exactly like
        // absent metadata ("product"), so nothing downstream changes meaning.
        assert!(stamped.get("type").is_none());
        assert_eq!(stamped[SESSION_STARS_METADATA_KEY], 250);
    }

    #[test]
    fn sessions_without_a_frozen_amount_fall_back_to_a_fresh_computation() {
        // Sessions created before this field existed must keep working.
        let legacy = session(300, "USD", Some(json!({"type": "plan"})));
        assert_eq!(persisted_session_star_amount(&legacy), None);
        assert_eq!(expected_session_star_amount(&legacy, 50).unwrap(), 150);
        assert_eq!(expected_session_star_amount(&legacy, 100).unwrap(), 300);
    }

    #[test]
    fn garbage_frozen_amounts_are_not_trusted() {
        for bad in [json!("150"), json!(0), json!(-150), json!(null), json!(1.5)] {
            let s = session(300, "USD", Some(json!({"stars_amount": bad})));
            assert_eq!(
                persisted_session_star_amount(&s),
                None,
                "frozen value {bad:?} must not be trusted"
            );
            // …and the gate silently reverts to a fresh computation.
            assert_eq!(expected_session_star_amount(&s, 50).unwrap(), 150);
        }
        // Above the per-invoice cap: also not trustworthy.
        let absurd = session(300, "USD", Some(json!({"stars_amount": 5_000_000})));
        assert_eq!(persisted_session_star_amount(&absurd), None);
    }

    // ---- invoice text derivation -------------------------------------------

    #[test]
    fn invoice_texts_follow_the_session_kind() {
        assert_eq!(
            invoice_texts(&session(300, "USD", Some(json!({"type": "plan"})))),
            ("VPN Subscription", "Subscription")
        );
        assert_eq!(
            invoice_texts(&session(300, "USD", Some(json!({"type": "order"})))),
            ("Store Order", "Order")
        );
        assert_eq!(
            invoice_texts(&session(300, "USD", None)),
            ("Purchase", "Purchase")
        );
    }

    #[test]
    fn invoice_text_fields_fit_telegram_limits() {
        let s = session(
            2_000_000,
            "USD",
            Some(json!({"type": "plan", "duration_days": 3650})),
        );
        let (title, label) = invoice_texts(&s);
        assert!(!title.is_empty() && title.chars().count() <= 32);
        assert!(!label.is_empty() && label.chars().count() <= 32);
        let description = invoice_description(&s);
        assert!(!description.is_empty() && description.chars().count() <= 255);
    }

    #[test]
    fn invoice_description_mentions_the_purchased_duration() {
        let s = session(
            300,
            "USD",
            Some(json!({"type": "plan", "duration_days": 30})),
        );
        assert_eq!(invoice_description(&s), "30 days of VPN access — $3.00");
        let s = session(300, "USD", Some(json!({"type": "order"})));
        assert_eq!(invoice_description(&s), "Payment of $3.00");
    }
}
