use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

#[derive(Serialize)]
struct CryptoBotInvoiceReq {
    /// "crypto" or "fiat". Crypto Pay API defaults to crypto when omitted, but we
    /// always send it explicitly so the asset/fiat pairing is unambiguous.
    currency_type: &'static str,
    /// Required when currency_type == "crypto" (e.g. USDT, TON, BTC...).
    #[serde(skip_serializing_if = "Option::is_none")]
    asset: Option<String>,
    /// Required when currency_type == "fiat" (e.g. RUB, USD, EUR...).
    #[serde(skip_serializing_if = "Option::is_none")]
    fiat: Option<String>,
    amount: String,
    description: String,
    hidden_message: String,
    paid_btn_name: String,
    paid_btn_url: String,
    payload: String,
}

#[derive(Deserialize)]
struct CryptoBotInvoiceRes {
    ok: bool,
    result: Option<CryptoBotInvoiceDetail>,
}

#[derive(Deserialize)]
struct CryptoBotInvoiceDetail {
    /// Preferred in-Telegram payment URL. `pay_url` is deprecated by Crypto Pay
    /// in favor of `bot_invoice_url`, so prefer it when present.
    #[serde(default)]
    bot_invoice_url: Option<String>,
    #[serde(default)]
    pay_url: Option<String>,
}

pub struct CryptoBotProvider {
    pub token: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for CryptoBotProvider {
    fn name(&self) -> &str {
        "cryptobot"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        // Amount must be formatted as a string for CryptoBot
        let amount_str = format!("{:.2}", (session.amount as f64) / 100.0);

        // Crypto Pay supports two invoice modes:
        //  - currency_type="crypto" with an `asset` (settled in crypto)
        //  - currency_type="fiat"   with a `fiat` code (amount denominated in fiat,
        //    payable in crypto). Required so fiat sessions (e.g. RUB) don't silently
        //    get coerced into a crypto asset and charge the wrong amount.
        let currency = session.currency.trim().to_uppercase();
        const SUPPORTED_CRYPTO: [&str; 8] =
            ["USDT", "TON", "BTC", "ETH", "LTC", "BNB", "TRX", "USDC"];
        const SUPPORTED_FIAT: [&str; 19] = [
            "USD", "EUR", "RUB", "BYN", "UAH", "GBP", "CNY", "KZT", "UZS", "GEL", "TRY", "AMD",
            "THB", "INR", "BRL", "IDR", "AZN", "AED", "PLN",
        ];

        let (currency_type, asset, fiat) = if SUPPORTED_CRYPTO.contains(&currency.as_str()) {
            ("crypto", Some(currency), None)
        } else if SUPPORTED_FIAT.contains(&currency.as_str()) {
            ("fiat", None, Some(currency))
        } else {
            // Unknown/unset currency: fall back to the previous default behavior
            // (settle in USDT) so existing crypto methods keep working.
            ("crypto", Some("USDT".to_string()), None)
        };

        let req_body = CryptoBotInvoiceReq {
            currency_type,
            asset,
            fiat,
            amount: amount_str,
            description: format!("VPN Subscription (Product: {})", session.product_id),
            hidden_message: "Thank you for your purchase!".to_string(),
            paid_btn_name: "callback".to_string(),
            paid_btn_url: format!("https://t.me/{}", self.bot_username),
            payload: session.id.to_string(), // Internal reference
        };

        let mut headers = HeaderMap::new();
        headers.insert("Crypto-Pay-API-Token", HeaderValue::from_str(&self.token)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let res = client
            .post("https://pay.crypt.bot/api/createInvoice")
            .headers(headers)
            .json(&req_body)
            .send()
            .await
            .context("Failed to send request to CryptoBot")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("CryptoBot API Error: {}", error_text);
        }

        let invoice: CryptoBotInvoiceRes = res
            .json()
            .await
            .context("Failed to parse CryptoBot response")?;

        if !invoice.ok {
            anyhow::bail!("CryptoBot API returned not ok");
        }

        if let Some(detail) = invoice.result {
            // Prefer the current `bot_invoice_url` (in-Telegram payment); fall back
            // to the deprecated `pay_url` for compatibility.
            detail
                .bot_invoice_url
                .or(detail.pay_url)
                .context("CryptoBot API result missing payment URL")
        } else {
            anyhow::bail!("CryptoBot API missing result detail");
        }
    }

    async fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<bool> {
        // Crypto Pay signs the webhook in the `crypto-pay-api-signature` HTTP header
        // (passed here as `signature`), not in the body — so we use the header value.
        // Secret = SHA256(app token); data = the raw unparsed request body, HMAC-SHA256,
        // hex-encoded.
        let mut hasher = Sha256::new();
        hasher.update(self.token.as_bytes());
        let secret_hash = hasher.finalize();

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(&secret_hash).context("Invalid HMAC key")?;

        mac.update(payload);
        let result = mac.finalize().into_bytes();
        let computed_sig = hex::encode(result);

        // Constant-time comparison to avoid leaking the signature via timing.
        let matches: bool = computed_sig.as_bytes().ct_eq(signature.as_bytes()).into();
        Ok(matches)
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value = serde_json::from_slice(payload).context("Invalid JSON")?;

        let update_type = data
            .get("update_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if update_type != "invoice_paid" {
            return Ok(PaymentWebhookAction::Ignored);
        }

        let payload_id = data
            .get("payload")
            .and_then(|v| v.get("payload"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if payload_id.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        Ok(PaymentWebhookAction::Completed {
            external_id: payload_id.to_string(),
        })
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}
