use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use base64::Engine;
use caramba_db::models::payment::PaymentType;
use caramba_db::models::store::{PaymentSession, User};
use chrono::Utc;
use subtle::ConstantTimeEq;

use super::provider::{PaymentProvider, PaymentWebhookAction};

#[derive(Serialize)]
struct CryptomusInvoiceReq {
    amount: String,
    currency: String,
    order_id: String,
    url_callback: String,
    url_return: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    additional_data: String,
}

#[derive(Deserialize)]
struct CryptomusInvoiceRes {
    result: Option<CryptomusInvoiceDetail>,
}

#[derive(Deserialize)]
struct CryptomusInvoiceDetail {
    url: String,
}

#[derive(Serialize)]
struct CryptomusPaymentInfoReq {
    order_id: String,
}

#[derive(Deserialize)]
struct CryptomusPaymentInfoRes {
    result: Option<CryptomusPaymentInfoDetail>,
}

#[derive(Deserialize)]
struct CryptomusPaymentInfoDetail {
    status: Option<String>,
}

pub struct CryptomusProvider {
    pub merchant_id: String,
    pub api_key: String,
    pub api_domain: String,
    pub bot_username: String,
}

pub struct CryptomusAdapter {
    merchant_id: String,
    api_key: String,
    // Переиспользуемый HTTP-клиент — не создаётся на каждый запрос
    http_client: reqwest::Client,
}

impl CryptomusAdapter {
    pub fn new(merchant_id: String, api_key: String) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client for CryptomusAdapter");
        Self {
            merchant_id,
            api_key,
            http_client,
        }
    }

    fn generate_signature(&self, body: &str) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let to_hash = format!("{}{}", encoded, self.api_key);
        format!("{:x}", md5::compute(to_hash.as_bytes()))
    }
}

#[async_trait]
impl crate::services::payment::PaymentAdapter for CryptomusAdapter {
    async fn create_invoice(
        &self,
        user_id: i64,
        amount_usd: f64,
        payment_type: PaymentType,
        bot_username: &str,
        api_domain: &str,
    ) -> Result<String> {
        let payload_str = payment_type.to_payload_string(user_id);
        let order_id = format!("{}_{}", user_id, Utc::now().timestamp());

        let body_json = serde_json::json!({
            "amount": amount_usd.to_string(),
            "currency": "USD",
            "order_id": order_id,
            // NOTE: legacy PayService path. The production Mini App flow uses
            // MarketplaceService → POST /api/webhooks/payment/cryptomus (session-based).
            // This `/caramba-api/payments/cryptomus` callback belongs to the older
            // bot-initiated flow; confirmation there relies on check_status/polling.
            "url_callback": format!("https://{}/caramba-api/payments/cryptomus", api_domain),
            "url_return": format!("https://t.me/{}", bot_username),
            "additional_data": payload_str
        });

        let body_str = serde_json::to_string(&body_json)?;
        let sign = self.generate_signature(&body_str);

        let resp = self
            .http_client
            .post("https://api.cryptomus.com/v1/payment")
            .header("merchant", &self.merchant_id)
            .header("sign", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?;

        let resp_json: serde_json::Value = resp.json().await?;

        if let Some(result) = resp_json.get("result")
            && let Some(url) = result.get("url").and_then(|u| u.as_str())
        {
            return Ok(url.to_string());
        }

        Err(anyhow::anyhow!("Cryptomus Error: {:?}", resp_json))
    }

    fn verify_signature(&self, payload: &str, signature: Option<&str>) -> Result<()> {
        let sig = signature.ok_or_else(|| anyhow::anyhow!("Missing sign header for Cryptomus"))?;
        let expected = self.generate_signature(payload);

        if sig == expected {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Invalid Cryptomus signature"))
        }
    }

    fn name(&self) -> &str {
        "cryptomus"
    }
}

impl CryptomusProvider {
    /// Cryptomus sign = md5( base64( body ) + api_key ).
    /// `body` MUST already be in PHP `json_encode(..., JSON_UNESCAPED_UNICODE)` form
    /// (forward slashes escaped as `\/`, non-ASCII left raw). For outgoing requests we
    /// only ever sign bodies without slashes/unicode (e.g. `{"order_id":"<uuid>"}`),
    /// so serde_json output already matches PHP byte-for-byte.
    fn generate_signature(&self, body: &str) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let to_hash = format!("{}{}", encoded, self.api_key);
        format!("{:x}", md5::compute(to_hash.as_bytes()))
    }

    /// Remove the top-level `"sign"` member from a raw Cryptomus webhook body while
    /// preserving every other byte exactly as Cryptomus sent it.
    ///
    /// Why not parse → re-serialize? Cryptomus signs `json_encode($data,
    /// JSON_UNESCAPED_UNICODE)` which preserves the ORIGINAL key order and escapes
    /// `/` as `\/`. serde_json (without the `preserve_order` feature, which other
    /// providers depend on staying OFF) re-sorts object keys alphabetically and does
    /// not escape slashes, so re-serializing a parsed `Value` would produce a
    /// different byte string and the signature would never match. The incoming body
    /// already IS `json_encode(...)` output, so surgically excising just the `sign`
    /// member yields exactly what PHP's re-`json_encode($data_without_sign)` produces.
    ///
    /// Returns `(extracted_sign, body_without_sign)` or `None` if no `sign` is found.
    fn strip_sign(raw: &str) -> Option<(String, String)> {
        // Read the sign value with a tolerant parse (reading does not depend on key
        // order, so using serde_json here is safe).
        let value: Value = serde_json::from_str(raw).ok()?;
        let sign = value.get("sign")?.as_str()?.to_string();

        // Locate the `"sign"` key token in the raw bytes. The key text `"sign"` is
        // unambiguous: a JSON string value can contain `sign` but never the exact
        // sequence `"sign"` immediately followed by `:` as an unescaped key, because
        // any inner occurrence would be inside a quoted value and thus preceded by
        // content, while the member key is always `"sign":`.
        let key_pat = "\"sign\"";
        // Search from the end so a (hypothetical) string value containing `"sign"`
        // earlier in the body does not shadow the real key. The real `sign` member is
        // typically last in Cryptomus payloads.
        // JSON structural whitespace is restricted to these ASCII bytes; we only ever
        // scan whitespace in structural positions (never inside a string value), so an
        // ASCII-only check is both correct and safe against multibyte UTF-8.
        let is_ws = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r');

        let mut search_from = raw.len();
        loop {
            let key_pos = raw[..search_from].rfind(key_pat)?;
            // After the key there must be optional whitespace then a colon.
            let after_key = key_pos + key_pat.len();
            let mut cursor = after_key;
            let bytes = raw.as_bytes();
            while cursor < bytes.len() && is_ws(bytes[cursor]) {
                cursor += 1;
            }
            if cursor >= bytes.len() || bytes[cursor] != b':' {
                // Not a key (it was a value containing the text "sign"); keep looking.
                search_from = key_pos;
                continue;
            }
            // Move past the colon and any whitespace to the value (a quoted hex string).
            cursor += 1;
            while cursor < bytes.len() && is_ws(bytes[cursor]) {
                cursor += 1;
            }
            if cursor >= bytes.len() || bytes[cursor] != b'"' {
                search_from = key_pos;
                continue;
            }
            // Consume the quoted value, honouring backslash escapes.
            cursor += 1; // opening quote
            let mut escaped = false;
            while cursor < bytes.len() {
                let c = bytes[cursor];
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == b'"' {
                    break;
                }
                cursor += 1;
            }
            if cursor >= bytes.len() {
                return None; // unterminated string — malformed
            }
            let value_end = cursor + 1; // include closing quote

            // The member spans [key_pos, value_end). Remove it together with exactly
            // one adjacent comma (the one that joins it to its sibling) so the
            // remaining JSON stays well-formed and byte-identical otherwise.
            let mut start = key_pos;
            let mut end = value_end;

            // Prefer trimming a trailing comma (member is not last).
            let mut after = value_end;
            while after < bytes.len() && is_ws(bytes[after]) {
                after += 1;
            }
            if after < bytes.len() && bytes[after] == b',' {
                end = after + 1;
            } else {
                // Member is last → trim the preceding comma instead.
                let mut before = key_pos;
                while before > 0 && is_ws(bytes[before - 1]) {
                    before -= 1;
                }
                if before > 0 && bytes[before - 1] == b',' {
                    start = before - 1;
                }
            }

            let mut stripped = String::with_capacity(raw.len());
            stripped.push_str(&raw[..start]);
            stripped.push_str(&raw[end..]);
            return Some((sign, stripped));
        }
    }
}

#[async_trait]
impl PaymentProvider for CryptomusProvider {
    fn name(&self) -> &str {
        "cryptomus"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let req = CryptomusInvoiceReq {
            amount: format!("{:.2}", (session.amount as f64) / 100.0),
            currency: session.currency.to_uppercase(),
            order_id: session.id.to_string(),
            url_callback: format!("https://{}/api/webhooks/payment/cryptomus", self.api_domain),
            url_return: format!("https://t.me/{}", self.bot_username),
            additional_data: String::new(),
        };

        let body_str = serde_json::to_string(&req)?;
        let sign = self.generate_signature(&body_str);

        let res = client
            .post("https://api.cryptomus.com/v1/payment")
            .header("merchant", &self.merchant_id)
            .header("sign", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await
            .context("Failed to send request to Cryptomus")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Cryptomus API Error: {}", error_text);
        }

        let resp: CryptomusInvoiceRes = res
            .json()
            .await
            .context("Failed to parse Cryptomus response")?;
        if let Some(detail) = resp.result {
            Ok(detail.url)
        } else {
            anyhow::bail!("Cryptomus API missing invoice URL");
        }
    }

    async fn verify_webhook(&self, payload: &[u8], _signature: &str) -> Result<bool> {
        // Cryptomus delivers the signature in the JSON BODY field `sign`, NOT in a
        // header (doc.cryptomus.com/merchant-api/payments/webhook). The `_signature`
        // argument is therefore empty for this provider (see api/webhooks.rs) and is
        // intentionally ignored.
        //
        // Verification (mirrors the official PHP example):
        //   $sign = $data['sign']; unset($data['sign']);
        //   md5( base64_encode( json_encode($data, JSON_UNESCAPED_UNICODE) ) . $apiKey )
        let raw = match std::str::from_utf8(payload) {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };

        let (received_sign, body_without_sign) = match Self::strip_sign(raw) {
            Some(parts) => parts,
            // No `sign` member present → cannot be a legitimate Cryptomus webhook.
            None => return Ok(false),
        };

        let expected = self.generate_signature(&body_without_sign);

        // Constant-time comparison to avoid leaking the signature via timing.
        // ct_eq on unequal-length slices returns false without leaking content; only
        // the (non-secret, fixed 32-char) length could differ, so this is safe.
        let matches: bool = received_sign.as_bytes().ct_eq(expected.as_bytes()).into();
        Ok(matches)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;

        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_id = data.get("order_id").and_then(|v| v.as_str()).unwrap_or("");

        if order_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        // Cryptomus payment statuses (doc.cryptomus.com/merchant-api/payments/webhook):
        //   paid / paid_over            → funds received (paid_over = overpaid, still good)
        //   fail / cancel / wrong_amount / system_fail → terminal failure
        //   confirm_check / check / process / wrong_amount_waiting → still pending
        // "success" is not a real Cryptomus status but is kept as a harmless alias.
        match status {
            "paid" | "paid_over" | "success" => {
                // U18: emit the INVOICE fiat amount/currency so MarketplaceService
                // can verify the paid magnitude against the stored session before
                // granting access. Cryptomus echoes the invoice `amount` (a decimal
                // string) in `currency` (e.g. "10.00" / "USD") — directly comparable
                // to the session, which is priced in the same currency. `paid_over`
                // reports the same invoice `amount` (the surplus is in crypto), so a
                // ">=" check correctly accepts overpayment.
                //
                // If either field is missing/unparseable we degrade to the legacy
                // `Completed` (no amount check) rather than risk wrongly rejecting a
                // genuine payment — keeping behavior at least as permissive as before.
                let invoice_amount = data.get("amount").and_then(parse_decimal_to_minor);
                let invoice_currency = data
                    .get("currency")
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
            "fail" | "cancel" | "wrong_amount" | "system_fail" => {
                Ok(PaymentWebhookAction::Failed {
                    reason: status.to_string(),
                })
            }
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(
        &self,
        session: &PaymentSession,
        client: &reqwest::Client,
    ) -> Result<String> {
        // Poll the invoice state via POST /v1/payment/info.
        // Body is signed with the same algorithm as other requests; `order_id` is a
        // UUID (no slashes / non-ASCII) so serde_json output equals PHP json_encode.
        let req = CryptomusPaymentInfoReq {
            order_id: session.id.to_string(),
        };
        let body_str = serde_json::to_string(&req)?;
        let sign = self.generate_signature(&body_str);

        let res = client
            .post("https://api.cryptomus.com/v1/payment/info")
            .header("merchant", &self.merchant_id)
            .header("sign", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await
            .context("Failed to query Cryptomus payment info")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Cryptomus payment/info error: {}", error_text);
        }

        let resp: CryptomusPaymentInfoRes = res
            .json()
            .await
            .context("Failed to parse Cryptomus payment/info response")?;

        let status = resp
            .result
            .and_then(|r| r.status)
            .unwrap_or_else(|| "pending".to_string());

        // Normalize provider statuses to the internal vocabulary used elsewhere.
        let normalized = match status.as_str() {
            "paid" | "paid_over" => "paid",
            "fail" | "cancel" | "wrong_amount" | "system_fail" => "failed",
            _ => "pending",
        };
        Ok(normalized.to_string())
    }
}

/// Parse a JSON amount (string like `"10.00"` or a JSON number) into MINOR units
/// (×100, rounded to nearest cent) for the U18 amount-verification gate. Returns
/// `None` if the value is absent or unparseable so the caller can fall back to the
/// legacy non-verifying `Completed` path rather than reject a real payment.
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
