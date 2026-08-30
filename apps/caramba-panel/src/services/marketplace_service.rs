use anyhow::{Context, Result};
use caramba_db::models::store::{PaymentSession, User};
use caramba_db::repositories::payment_session_repo::PaymentSessionRepository;
use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::payment::aaio::AaioProvider;
use super::payment::balance::BalanceProvider;
use super::payment::btcpay::BtcPayProvider;
use super::payment::coinbase_commerce::CoinbaseCommerceProvider;
use super::payment::cryptobot::CryptoBotProvider;
use super::payment::cryptomus::CryptomusProvider;
use super::payment::crystalpay::CrystalPayProvider;
use super::payment::lava::LavaProvider;
use super::payment::manual::ManualProvider;
use super::payment::nowpayments::NowPaymentsProvider;
use super::payment::oxapay::OxaPayProvider;
use super::payment::paypalych::PaypalychProvider;
use super::payment::plisio::PlisioProvider;
use super::payment::provider::{PaymentProvider, PaymentWebhookAction};
use super::payment::stripe::StripeProvider;
use super::payment::tribute::TributeProvider;
use super::payment::wata::WataProvider;
// StarsProvider намеренно исключён из MarketplaceService: интерфейс PaymentProvider
// не имеет доступа к bot_token и tg_id, которые требуются Bot API для createInvoiceLink.
// Рабочий путь для Stars — PayService::create_stars_invoice (вызывается из бота).
use super::store_service::StoreService;
use super::subscription_service::SubscriptionService;

#[derive(Clone)]
pub struct MarketplaceService {
    pool: PgPool,
    pub session_repo: PaymentSessionRepository,
    providers: Arc<HashMap<String, Box<dyn PaymentProvider>>>,
    pub http_client: reqwest::Client,
    pub store_service: StoreService,
    pub sub_service: SubscriptionService,
    /// DM-канал для платёжных уведомлений (счёт создан / оплата получена).
    /// Все отправки — fire-and-forget: сбой доставки не должен ломать checkout.
    bot_manager: Arc<crate::bot_manager::BotManager>,
    /// Снимок `bot_username` на момент старта — фолбэк, если у BotManager ещё
    /// нет живого username (бот не запущен). Может быть пустым.
    bot_username: String,
}

/// Returns the settings key controlling whether a payment provider is offered in
/// the Mini App, plus its default state when the key has never been set.
///
/// This is the single source of truth shared between the admin toggle UI and the
/// `/payments/providers` endpoint, so flipping a switch in the panel takes effect
/// immediately. Opt-in RU providers default **off** (the operator must enable them
/// explicitly); legacy global providers default **on** once credentials exist.
/// `coinbase_commerce` maps to the historical `coinbase_enabled` key already used
/// by the settings form.
pub fn provider_enable_setting(name: &str) -> (String, bool) {
    match name {
        "coinbase_commerce" => ("coinbase_enabled".to_string(), false),
        "wata" | "crystalpay" | "tribute" | "btcpay" | "oxapay" | "plisio" | "paypalych"
        | "manual" => (format!("{name}_enabled"), false),
        // nowpayments / cryptobot / cryptomus / lava / aaio / stripe: shown when
        // configured unless the admin explicitly turns them off.
        _ => (format!("{name}_enabled"), true),
    }
}

impl MarketplaceService {
    pub fn new(
        pool: PgPool,
        nowpayments_key: String,
        nowpayments_ipn_secret: String,
        cryptobot_token: String,
        cryptomus_merchant_id: String,
        cryptomus_api_key: String,
        lava_project_id: String,
        lava_secret_key: String,
        aaio_merchant_id: String,
        aaio_secret_1: String,
        aaio_secret_2: String,
        stripe_secret_key: String,
        stripe_webhook_secret: String,
        // WATA (RU СБП/карты)
        wata_jwt_token: String,
        wata_webhook_secret: String,
        // CrystalPay (RU СБП + крипта)
        crystalpay_login: String,
        crystalpay_secret: String,
        crystalpay_salt: String,
        // Tribute (Telegram-native)
        tribute_api_key: String,
        tribute_webhook_secret: String,
        // BTCPay Server (самохостинг, без KYC)
        btcpay_url: String,
        btcpay_api_key: String,
        btcpay_store_id: String,
        btcpay_webhook_secret: String,
        // OxaPay (крипта, RU-friendly)
        oxapay_merchant_key: String,
        // Coinbase Commerce (мировая крипта)
        coinbase_api_key: String,
        coinbase_webhook_secret: String,
        // Plisio (мировая крипта)
        plisio_api_key: String,
        // Paypalych (RU: SBP + USDT TRC20, pal24.pro / pally.info)
        paypalych_api_token: String,
        paypalych_shop_id: String,
        // Accepted for back-compat with v0.9.52 installations that still set
        // it in the env / DB. Ignored by `PaypalychProvider` since v0.9.53 —
        // the API token itself signs the webhook (no separate secret).
        _paypalych_webhook_secret: String,
        api_domain: String,
        bot_username: String,
        bot_manager: Arc<crate::bot_manager::BotManager>,
        store_service: StoreService,
        sub_service: SubscriptionService,
    ) -> Self {
        let session_repo = PaymentSessionRepository::new(pool.clone());
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client for MarketplaceService");
        let mut providers: HashMap<String, Box<dyn PaymentProvider>> = HashMap::new();

        // "stars" намеренно отсутствует — см. комментарий к импортам выше.
        providers.insert("manual".to_string(), Box::new(ManualProvider));

        if !nowpayments_key.is_empty() && !nowpayments_ipn_secret.is_empty() {
            providers.insert(
                "nowpayments".to_string(),
                Box::new(NowPaymentsProvider {
                    api_key: nowpayments_key,
                    ipn_secret: nowpayments_ipn_secret,
                    api_domain: api_domain.clone(),
                    bot_username: bot_username.clone(),
                }),
            );
        }

        if !cryptobot_token.is_empty() {
            providers.insert(
                "cryptobot".to_string(),
                Box::new(CryptoBotProvider {
                    token: cryptobot_token,
                    bot_username: bot_username.clone(),
                }),
            );
        }

        if !cryptomus_merchant_id.is_empty() && !cryptomus_api_key.is_empty() {
            providers.insert(
                "cryptomus".to_string(),
                Box::new(CryptomusProvider {
                    merchant_id: cryptomus_merchant_id,
                    api_key: cryptomus_api_key,
                    api_domain: api_domain.clone(),
                    bot_username: bot_username.clone(),
                }),
            );
        }

        if !lava_project_id.is_empty() && !lava_secret_key.is_empty() {
            providers.insert(
                "lava".to_string(),
                Box::new(LavaProvider {
                    project_id: lava_project_id,
                    secret_key: lava_secret_key,
                    api_domain: api_domain.clone(),
                }),
            );
        }

        if !aaio_merchant_id.is_empty() && !aaio_secret_1.is_empty() {
            providers.insert(
                "aaio".to_string(),
                Box::new(AaioProvider {
                    merchant_id: aaio_merchant_id,
                    secret_1: aaio_secret_1,
                    secret_2: aaio_secret_2,
                }),
            );
        }

        // Stripe: регистрируем если задан секретный ключ.
        // webhook_secret нужен для проверки подписи вебхука (whsec_...).
        if !stripe_secret_key.is_empty() {
            providers.insert(
                "stripe".to_string(),
                Box::new(StripeProvider {
                    secret_key: stripe_secret_key,
                    webhook_secret: stripe_webhook_secret,
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // WATA: достаточно JWT-токена; webhook_secret может быть пустым (с предупреждением).
        if !wata_jwt_token.is_empty() {
            providers.insert(
                "wata".to_string(),
                Box::new(WataProvider {
                    jwt_token: wata_jwt_token,
                    webhook_secret: wata_webhook_secret,
                    api_domain: api_domain.clone(),
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // CrystalPay: нужны login + secret; salt опционален (предупреждение).
        if !crystalpay_login.is_empty() && !crystalpay_secret.is_empty() {
            providers.insert(
                "crystalpay".to_string(),
                Box::new(CrystalPayProvider {
                    login: crystalpay_login,
                    secret: crystalpay_secret,
                    salt: crystalpay_salt,
                    api_domain: api_domain.clone(),
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // Tribute: нужен API-ключ; webhook_secret обязателен для верификации подписи.
        if !tribute_api_key.is_empty() {
            providers.insert(
                "tribute".to_string(),
                Box::new(TributeProvider {
                    api_key: tribute_api_key,
                    webhook_secret: tribute_webhook_secret,
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // BTCPay Server: нужны url + api_key + store_id.
        if !btcpay_url.is_empty() && !btcpay_api_key.is_empty() && !btcpay_store_id.is_empty() {
            providers.insert(
                "btcpay".to_string(),
                Box::new(BtcPayProvider {
                    btcpay_url,
                    api_key: btcpay_api_key,
                    store_id: btcpay_store_id,
                    webhook_secret: btcpay_webhook_secret,
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // OxaPay: нужен только merchant_key.
        if !oxapay_merchant_key.is_empty() {
            providers.insert(
                "oxapay".to_string(),
                Box::new(OxaPayProvider {
                    merchant_key: oxapay_merchant_key,
                    api_domain: api_domain.clone(),
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // Coinbase Commerce: нужны api_key + webhook_secret.
        if !coinbase_api_key.is_empty() {
            providers.insert(
                "coinbase_commerce".to_string(),
                Box::new(CoinbaseCommerceProvider {
                    api_key: coinbase_api_key,
                    webhook_secret: coinbase_webhook_secret,
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // Plisio: нужен только api_key.
        if !plisio_api_key.is_empty() {
            providers.insert(
                "plisio".to_string(),
                Box::new(PlisioProvider {
                    api_key: plisio_api_key,
                    api_domain: api_domain.clone(),
                    bot_username: bot_username.clone(),
                }),
            );
        }

        // Paypalych (pal24.pro): SBP + USDT TRC20 для RU-аудитории.
        // `api_token` doubles as the webhook signing key (no separate secret);
        // `shop_id` is required by the dashboard-configured Success/Fail/Result
        // URLs to be honoured. The `_paypalych_webhook_secret` parameter is
        // accepted for backward-compat (some installations still have it set
        // from v0.9.52) but is ignored by the provider since v0.9.53.
        if !paypalych_api_token.is_empty() {
            providers.insert(
                "paypalych".to_string(),
                Box::new(PaypalychProvider {
                    api_token: paypalych_api_token,
                    shop_id: paypalych_shop_id,
                }),
            );
        }

        providers.insert("balance".to_string(), Box::new(BalanceProvider));

        Self {
            pool,
            session_repo,
            providers: Arc::new(providers),
            http_client,
            store_service,
            sub_service,
            bot_manager,
            bot_username,
        }
    }

    pub fn get_provider(&self, name: &str) -> Option<&dyn PaymentProvider> {
        self.providers.get(name).map(|p| p.as_ref())
    }

    /// Returns the names of all registered (configured) payment providers, sorted
    /// for stable ordering. A provider is registered only when its credentials were
    /// present at startup, so this is the source of truth for "which methods exist".
    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }

    pub async fn create_session(
        &self,
        user: &User,
        product_id: i64,
        provider_name: &str,
        amount: i64,
        currency: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<(PaymentSession, String)> {
        let provider = self
            .get_provider(provider_name)
            .context("Payment provider not found or disabled")?;

        let metadata = normalize_payment_metadata(metadata)?;
        self.validate_session_resource(product_id, metadata.as_ref())
            .await?;

        // Referral money model — REFEREE side: a one-time percentage discount on
        // the invited user's FIRST paid purchase. `referee_first_purchase_discount`
        // returns 0 unless the user has a referrer AND no prior claimed/completed
        // purchase, so the discount can never apply twice (a discount claim is
        // recorded below in the same path that produces the discounted session).
        // The discount lands on session.amount — the figure every provider invoice
        // is built from here, the figure the referrer reward is later computed
        // against, and the figure the balance-wallet handlers (api/client.rs,
        // app_billing.rs) debit and refund.
        let discount_pct =
            crate::services::referral_service::ReferralService::referee_first_purchase_discount(
                &self.pool, user.id,
            )
            .await
            .unwrap_or(0);
        let amount = if discount_pct > 0 {
            let discounted = amount - (amount * discount_pct / 100);
            // Never charge below zero; keep at least 1 minor unit so providers
            // that reject zero-amount invoices still work.
            discounted.max(1)
        } else {
            amount
        };

        let session = PaymentSession {
            id: Uuid::new_v4(),
            user_id: user.id,
            product_id,
            provider: provider_name.to_string(),
            external_id: None,
            amount,
            currency: currency.to_string(),
            status: "pending".to_string(),
            metadata,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // If amount is for a specific plan duration, try to store duration_days in metadata
        // In this architecture, product_id is plan_id.
        // We might want to pass metadata from the caller.
        // For now, let's keep create_session signature but add an internal enhancement
        // OR better: change create_session to accept metadata.

        self.session_repo.create(&session).await?;

        // Generate the invoice URL/Payload from the provider
        let invoice_payload = provider
            .create_invoice(&session, user, &self.http_client)
            .await?;

        Ok((session, invoice_payload))
    }

    pub async fn handle_webhook(
        &self,
        provider_name: &str,
        payload: &[u8],
        signature: &str,
    ) -> Result<()> {
        let provider = self
            .get_provider(provider_name)
            .context("Payment provider not found")?;

        if !provider.verify_webhook(payload, signature).await? {
            anyhow::bail!("Invalid webhook signature");
        }

        let action = provider.handle_webhook(payload).await?;

        match action {
            PaymentWebhookAction::Completed { external_id } => {
                // Провайдеры возвращают session UUID в поле external_id.
                // Пробуем сначала найти сессию по UUID (основной путь),
                // затем — по полю external_id в базе (резервный путь).
                let session_opt = self.lookup_session_by_external(&external_id).await;

                if let Some(session) = session_opt {
                    self.fulfill_payment(session.id).await?;
                } else {
                    tracing::warn!(
                        provider = provider_name,
                        external_id = %external_id,
                        "Received Completed webhook but no matching payment session found"
                    );
                }
            }
            PaymentWebhookAction::CompletedWithAmount {
                external_id,
                paid_amount_minor,
                paid_currency,
            } => {
                // U18: amount/currency verification gate. A provider that knows the
                // actually-paid (fiat) amount routes through here so we can refuse
                // to fulfill an under-paid or wrong-currency invoice — defeating the
                // "pay $1 for a $10 plan" / replayed-webhook abuse class.
                let session_opt = self.lookup_session_by_external(&external_id).await;

                let Some(session) = session_opt else {
                    tracing::warn!(
                        provider = provider_name,
                        external_id = %external_id,
                        "Received CompletedWithAmount webhook but no matching payment session found"
                    );
                    return Ok(());
                };

                // Currency must match (case-insensitive). A mismatch means we cannot
                // even compare the magnitudes meaningfully, so it's an automatic
                // reject. Providers emitting this variant are contractually obliged
                // to echo back the INVOICE currency, which always equals the session
                // currency on the happy path.
                let currency_ok = paid_currency.eq_ignore_ascii_case(&session.currency);
                // Allow exact-or-over payment; only block genuine underpayment.
                let amount_ok = paid_amount_minor >= session.amount;

                if currency_ok && amount_ok {
                    self.fulfill_payment(session.id).await?;
                } else {
                    // Read-only refusal: do NOT mutate the session here. Leaving it
                    // 'pending' lets the operator inspect/refund and keeps webhook
                    // retries idempotent. We log at ERROR so it surfaces immediately;
                    // the daily reconciliation pass (U21) additionally re-detects this
                    // divergence and pages admins via notify_admins.
                    tracing::error!(
                        provider = provider_name,
                        session_id = %session.id,
                        expected_amount = session.amount,
                        expected_currency = %session.currency,
                        paid_amount = paid_amount_minor,
                        paid_currency = %paid_currency,
                        currency_ok,
                        amount_ok,
                        "Rejecting payment fulfillment: amount/currency mismatch (possible under/over-pay abuse)"
                    );
                }
            }
            PaymentWebhookAction::Failed { reason } => {
                tracing::warn!(
                    provider = provider_name,
                    reason = %reason,
                    "Payment webhook reported failure"
                );
            }
            _ => {}
        }
        Ok(())
    }

    /// Resolve a session from a provider's `external_id` token. Providers return
    /// the session UUID there (primary path); we fall back to matching the stored
    /// `external_id` column for providers that key by their own invoice id.
    async fn lookup_session_by_external(&self, external_id: &str) -> Option<PaymentSession> {
        if let Ok(uuid) = external_id.parse::<uuid::Uuid>()
            && let Some(session) = self.session_repo.get_by_id(uuid).await.ok().flatten()
        {
            return Some(session);
        }
        self.session_repo
            .get_by_external_id(external_id)
            .await
            .ok()
            .flatten()
    }

    /// Webhook-loss polling fallback (U20/U13). Walks recent still-`pending`
    /// sessions and asks each provider's `check_status`; fulfills the ones the
    /// provider now reports as paid. Returns the number of sessions fulfilled.
    ///
    /// Designed to be cheap and safe to run every minute or two:
    ///   - only sessions created in the last `max_age_hours` are polled (older
    ///     ones are swept to 'expired' by `expire_stale_sessions`);
    ///   - providers without a real status endpoint return "pending" and are a
    ///     no-op (see `PaymentProvider::check_status` contract);
    ///   - providers advertising `supports_polling() == false` (off-chain methods
    ///     like manual/balance/stars) are skipped entirely;
    ///   - fulfillment goes through `fulfill_payment`, which is idempotent and
    ///     guards against double-grant.
    pub async fn poll_pending_sessions(&self, max_age_hours: i64, limit: i64) -> Result<u64> {
        let pending = self
            .session_repo
            .list_pending_recent(max_age_hours, limit)
            .await
            .context("Failed to list recent pending sessions for polling")?;

        if pending.is_empty() {
            return Ok(0);
        }

        let mut fulfilled = 0u64;
        for session in pending {
            let Some(provider) = self.get_provider(&session.provider) else {
                // Provider was de-configured since the session was created — nothing
                // we can poll; leave it for the stale-expiry sweep.
                continue;
            };

            if !provider.supports_polling() {
                continue;
            }

            // A poll failure for one session must not abort the whole batch.
            let status = match provider.check_status(&session, &self.http_client).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!(
                        provider = %session.provider,
                        session_id = %session.id,
                        error = %e,
                        "check_status poll failed; will retry next tick"
                    );
                    continue;
                }
            };

            match status.as_str() {
                "paid" | "completed" | "success" => match self.fulfill_payment(session.id).await {
                    Ok(()) => {
                        fulfilled += 1;
                        tracing::info!(
                            provider = %session.provider,
                            session_id = %session.id,
                            "Fulfilled payment via polling fallback (webhook likely lost)"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            provider = %session.provider,
                            session_id = %session.id,
                            error = %e,
                            "Polling found a paid session but fulfillment failed"
                        );
                    }
                },
                // "failed" / "pending" / anything else: leave the session as-is.
                _ => {}
            }
        }

        Ok(fulfilled)
    }

    /// Daily reconciliation audit (U21). READ-ONLY: scans sessions from the last
    /// `lookback_hours` for divergence and returns a list of human-readable
    /// findings for the caller (monitoring) to forward to admins via
    /// `notify_admins`. Never mutates state.
    ///
    /// Flags, conservatively (false positives are noisy, not dangerous):
    ///   - sessions whose `provider` is no longer configured (orphaned);
    ///   - non-positive amounts on a non-failed session (data integrity);
    ///   - an unusually high count of still-`pending` recent sessions, which can
    ///     indicate a provider/webhook outage worth a human look.
    pub async fn reconcile_recent(&self, lookback_hours: i64) -> Result<Vec<String>> {
        let rows = self
            .session_repo
            .list_recent_for_audit(lookback_hours)
            .await
            .context("Failed to load sessions for reconciliation")?;

        let mut findings: Vec<String> = Vec::new();
        let mut pending_count = 0usize;

        for (id, provider, status, amount, currency, _external_id, _created_at) in &rows {
            if status == "pending" {
                pending_count += 1;
            }

            // Orphaned provider: a completed/pending session referencing a provider
            // that is no longer registered cannot be polled or refunded cleanly.
            if self.get_provider(provider).is_none() && status != "expired" && status != "failed" {
                findings.push(format!(
                    "session {} uses unconfigured provider '{}' (status={}, {} {})",
                    id,
                    provider,
                    status,
                    *amount as f64 / 100.0,
                    currency
                ));
            }

            // Non-positive amount on a session that was meant to collect money.
            if *amount <= 0 && status != "failed" && status != "expired" {
                findings.push(format!(
                    "session {} has non-positive amount {} {} (status={}, provider={})",
                    id, amount, currency, status, provider
                ));
            }
        }

        // A large pending backlog over the audit window often means a webhook
        // endpoint or provider is down. Threshold kept conservative.
        if pending_count >= 25 {
            findings.push(format!(
                "{} sessions still pending over the last {}h — possible webhook/provider outage",
                pending_count, lookback_hours
            ));
        }

        Ok(findings)
    }

    /// Marks payment_sessions older than `max_age_hours` and still in 'pending'
    /// state as 'expired'. Returns the count of rows updated.
    ///
    /// Used by a daily background task — without it, abandoned checkouts (user
    /// closed browser before paying, provider never sent a webhook, etc.)
    /// stay 'pending' forever, polluting the dashboard's pending count and
    /// making real stuck payments harder to spot.
    pub async fn expire_stale_sessions(&self, max_age_hours: i64) -> Result<u64> {
        let res = sqlx::query(
            "UPDATE payment_sessions
             SET status = 'expired', updated_at = CURRENT_TIMESTAMP
             WHERE status = 'pending'
               AND created_at < CURRENT_TIMESTAMP - ($1 || ' hours')::INTERVAL",
        )
        .bind(max_age_hours.to_string())
        .execute(&self.pool)
        .await
        .context("Failed to expire stale payment sessions")?;
        let count = res.rows_affected();
        if count > 0 {
            tracing::info!(
                "Expired {} stale payment sessions older than {}h",
                count,
                max_age_hours
            );
        }
        Ok(count)
    }

    /// Mark a payment session as failed (used when the charge step fails before
    /// fulfillment, e.g. an insufficient-balance wallet payment).
    pub async fn mark_session_failed(&self, session_id: Uuid) -> Result<()> {
        self.session_repo.update_status(session_id, "failed").await
    }

    pub async fn fulfill_payment(&self, session_id: Uuid) -> Result<()> {
        // 1. Fetch session
        let session = self
            .session_repo
            .get_by_id(session_id)
            .await?
            .context("Session not found")?;

        // 2. Validate the resource BEFORE claiming (read-only; a bad session
        //    must not consume the atomic claim).
        self.validate_session_resource(session.product_id, session.metadata.as_ref())
            .await?;

        // 3. Atomically claim the `pending` -> `completed` transition. A
        //    duplicate provider webhook, a webhook retry, or a race with the
        //    lost-webhook poller loses the claim (0 rows) and bails BEFORE any
        //    side effect — so a subscription can never be extended twice.
        if !self.session_repo.claim_for_fulfillment(session_id).await? {
            tracing::warn!(
                "Session {} already claimed/completed, ignoring duplicate fulfillment.",
                session_id
            );
            return Ok(());
        }

        // 4. Fulfill. Status is already `completed` from the claim above; on
        //    failure we mark it `failed` (matches prior behavior).
        let fulfillment_result = self.fulfill_session_resource(&session).await;
        match fulfillment_result {
            Ok(()) => {
                // Referral money model — REFERRER side: credit the inviter's
                // internal balance with reward_percent of this payment when the
                // referee's FIRST paid purchase is fulfilled. Idempotent: the
                // referral_rewards UNIQUE(referred_user_id) makes any later
                // fulfillment a no-op, so a given referee credits the referrer at
                // most once. A failure here must NOT undo the (already committed)
                // fulfillment — log and move on.
                if let Err(e) = self
                    .grant_referrer_reward(session.user_id, session.amount)
                    .await
                {
                    tracing::error!(
                        session_id = %session_id,
                        user_id = session.user_id,
                        error = %e,
                        "Referral reward granting failed after fulfillment (non-fatal)"
                    );
                }

                // Успешное подтверждение оплаты — единственное на сессию (атомарный
                // claim выше гарантирует ровно один проход этой ветки), поэтому DM
                // «оплата получена» не задублируется даже при повторных вебхуках.
                self.notify_payment_success(&session);

                Ok(())
            }
            Err(error) => {
                let _ = self.session_repo.update_status(session_id, "failed").await;
                Err(error)
            }
        }
    }

    /// Человекочитаемая метка оплачиваемого ресурса для DM со счётом:
    /// план — его имя из БД (фолбэк — «подписка»), заказ — «заказ №N».
    async fn payment_product_label(&self, session: &PaymentSession, lang: Option<&str>) -> String {
        use crate::bot::translations::{t, tf};

        match payment_resource_type(session.metadata.as_ref()) {
            "order" => tf(lang, "label_order", &[&session.product_id.to_string()]),
            _ => {
                let name: Option<String> =
                    sqlx::query_scalar("SELECT name FROM plans WHERE id = $1")
                        .bind(session.product_id)
                        .fetch_optional(&self.pool)
                        .await
                        .unwrap_or(None);
                name.unwrap_or_else(|| t(lang, "label_subscription").to_string())
            }
        }
    }

    /// Username бота для кнопок в DM: живое значение из BotManager, иначе
    /// снимок из настроек на момент старта. Пустая строка = username неизвестен.
    async fn resolve_bot_username(&self) -> String {
        let name = match self.bot_manager.get_username().await {
            Some(u) if !u.trim().is_empty() => u,
            _ => self.bot_username.clone(),
        };
        name.trim().trim_start_matches('@').to_string()
    }

    /// Fire-and-forget DM пользователю со ссылкой на оплату созданного счёта.
    /// Отправляется ТОЛЬКО для внешних http(s)-чекаутов — stars/manual/balance
    /// имеют собственный UX внутри приложения/бота. Ошибки доставки логируются
    /// и не влияют на ответ API (Mini App уже получил invoice_url).
    pub fn notify_invoice_created(&self, user: &User, session: &PaymentSession, invoice_url: &str) {
        use crate::bot::translations::{t, tf};

        if !invoice_url.starts_with("http://") && !invoice_url.starts_with("https://") {
            return;
        }
        // Stars-инвойсы (t.me/invoice/...) открываются нативно внутри Telegram —
        // дублировать их в чат бессмысленно (защитная проверка: StarsProvider в
        // MarketplaceService не зарегистрирован, но ссылка могла прийти извне).
        if invoice_url.contains("t.me/invoice") {
            return;
        }

        let svc = self.clone();
        let session = session.clone();
        let tg_id = user.tg_id;
        let lang = user.language_code.clone();
        let invoice_url = invoice_url.to_string();
        tokio::spawn(async move {
            let lang = lang.as_deref();
            let label = svc.payment_product_label(&session, lang).await;
            let amount = format!("{:.2}", session.amount as f64 / 100.0);
            let text = tf(
                lang,
                "invoice_created",
                &[&label, &amount, &session.currency],
            );
            let mut payload = crate::bot_manager::NotificationPayload::plain(text);
            payload
                .buttons
                .push((t(lang, "pay_button").to_string(), invoice_url));
            if let Err(e) = svc.bot_manager.send_rich_notification(tg_id, payload).await {
                tracing::warn!(
                    session_id = %session.id,
                    tg_id,
                    error = %e,
                    "Invoice DM delivery failed (non-fatal)"
                );
            }
        });
    }

    /// Fire-and-forget DM «оплата получена» после успешного fulfillment'а.
    /// Вызывается ровно один раз на сессию (см. atomic claim в fulfill_payment).
    fn notify_payment_success(&self, session: &PaymentSession) {
        use crate::bot::translations::t;

        let svc = self.clone();
        let session = session.clone();
        tokio::spawn(async move {
            // session.user_id — внутренний id БД; для DM нужен tg_id чата.
            // (тот же паттерн, что в pay_service.rs при активационных DM)
            let row: Option<(i64, Option<String>)> =
                sqlx::query_as("SELECT tg_id, language_code FROM users WHERE id = $1")
                    .bind(session.user_id)
                    .fetch_optional(&svc.pool)
                    .await
                    .unwrap_or(None);
            let Some((tg_id, lang)) = row else {
                tracing::warn!(
                    session_id = %session.id,
                    user_id = session.user_id,
                    "Payment success DM skipped: user not found"
                );
                return;
            };
            let lang = lang.as_deref();

            let key = match payment_resource_type(session.metadata.as_ref()) {
                "order" => "payment_success_order",
                // "plan" и каталожные продукты, продлевающие подписку
                _ => "payment_success_sub",
            };
            let mut payload = crate::bot_manager::NotificationPayload::plain(t(lang, key));

            // Кнопка возврата в Mini App. `?startapp` открывает главный Mini App
            // бота (deep link Telegram); без username кнопку не показываем.
            let username = svc.resolve_bot_username().await;
            if !username.is_empty() {
                payload.buttons.push((
                    t(lang, "open_app_button").to_string(),
                    format!("https://t.me/{}?startapp", username),
                ));
            }

            if let Err(e) = svc.bot_manager.send_rich_notification(tg_id, payload).await {
                tracing::warn!(
                    session_id = %session.id,
                    tg_id,
                    error = %e,
                    "Payment success DM delivery failed (non-fatal)"
                );
            }
        });
    }

    /// Credits the referrer of `user_id` with the first-purchase money reward.
    /// Runs in its own transaction; the referral_rewards ledger makes it safe to
    /// call on every fulfillment (only the first paid purchase per referee pays
    /// out). The referrer sees the credit in the app's referral summary and
    /// balance; this shared fulfillment path has no DM channel, so no push here.
    async fn grant_referrer_reward(&self, user_id: i64, amount_cents: i64) -> Result<()> {
        use crate::services::referral_service::ReferralService;

        let mut tx = self.pool.begin().await?;
        let reward =
            ReferralService::apply_first_purchase_reward(&mut tx, user_id, amount_cents).await?;
        tx.commit().await?;

        if let Some((referrer_tg_id, bonus_cents)) = reward {
            tracing::info!(
                referrer_tg_id,
                bonus_cents,
                referred_user_id = user_id,
                "Referral first-purchase reward credited to referrer balance"
            );
        }

        Ok(())
    }

    async fn fulfill_session_resource(&self, session: &PaymentSession) -> Result<()> {
        let resource_type = payment_resource_type(session.metadata.as_ref());

        if resource_type == "plan" {
            let days_to_add = self.resolve_plan_duration_days(session).await?;
            let subs = self
                .sub_service
                .get_user_subscriptions(session.user_id)
                .await?;

            tracing::info!(
                "Fulfilling VPN Plan for user {}: extending by {} days",
                session.user_id,
                days_to_add
            );

            if let Some(active_sub) = subs.first() {
                self.sub_service
                    .admin_extend(active_sub.sub.id, days_to_add)
                    .await?;
            } else {
                let _ = self
                    .store_service
                    .admin_gift_subscription(session.user_id, session.product_id, days_to_add)
                    .await?;
            }

            return Ok(());
        }

        if resource_type == "order" {
            // For store orders, `product_id` carries the order id. Mark it paid so the
            // user's purchase is recorded; physical/digital delivery (file, gift code,
            // etc.) is handled by the admin or an out-of-band mechanism. The guard on
            // status keeps webhook retries idempotent.
            let order_id = session.product_id;
            let updated = sqlx::query(
                "UPDATE orders SET status = 'paid', paid_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status <> 'paid'",
            )
            .bind(order_id)
            .execute(&self.pool)
            .await
            .context("Failed to mark order as paid")?;

            tracing::info!(
                "Fulfilled store order {} for user {} (rows_affected={})",
                order_id,
                session.user_id,
                updated.rows_affected()
            );

            return Ok(());
        }

        // Для физических/цифровых товаров из каталога — списываем баланс и создаём заказ.
        // Фактическая доставка (email, gift code и т.п.) производится вне этого слоя.
        let products = self.store_service.get_all_products().await?;
        let product = products
            .into_iter()
            .find(|p| p.id == session.product_id)
            .context("Product not found")?;

        tracing::info!(
            "Fulfilling Product '{}' (type='{}') for user {} (amount={})",
            product.name,
            product.product_type,
            session.user_id,
            session.amount,
        );

        // TODO(product-type-dispatch): При появлении новых типов продуктов добавить ветки.
        // Известные типы на 2026-04-28: "file", "text", "subscription".
        // Неизвестные типы возвращают ошибку — безопаснее отказать, чем доставить неправильно.
        match product.product_type.as_str() {
            "file" | "text" => {
                // Продукты типа file/text: доставка производится вручную администратором
                // или через отдельный внешний механизм. Здесь фиксируем факт оплаты в orders.
                sqlx::query(
                    "INSERT INTO orders (user_id, total_amount, status, paid_at) \
                     VALUES ($1, $2, 'paid', CURRENT_TIMESTAMP)",
                )
                .bind(session.user_id)
                .bind(session.amount)
                .execute(&self.pool)
                .await
                .context("Failed to create order record for fulfilled product session")?;
            }
            other => {
                // TODO(product-type-dispatch): Тип '{}' не обрабатывается MarketplaceService.
                // Реализуйте явную ветку перед добавлением такого продукта в каталог.
                anyhow::bail!(
                    "Unhandled product_type '{}' for product {} — fulfillment not implemented. \
                     Add an explicit dispatch branch in fulfill_session_resource.",
                    other,
                    product.id
                );
            }
        }

        Ok(())
    }

    async fn resolve_plan_duration_days(&self, session: &PaymentSession) -> Result<i32> {
        if let Some(days) = session
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("duration_days"))
            .and_then(|value| value.as_i64())
        {
            return Ok(days as i32);
        }

        let duration_row: Option<(i32,)> = sqlx::query_as(
            "SELECT duration_days FROM plan_durations WHERE plan_id = $1 AND price = $2 LIMIT 1",
        )
        .bind(session.product_id)
        .bind(session.amount)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        Ok(duration_row.map(|(days,)| days).unwrap_or(30))
    }

    async fn validate_session_resource(
        &self,
        product_id: i64,
        metadata: Option<&Value>,
    ) -> Result<()> {
        match payment_resource_type(metadata) {
            "plan" => {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plans WHERE id = $1)")
                        .bind(product_id)
                        .fetch_one(&self.pool)
                        .await?;
                if !exists {
                    anyhow::bail!("Plan {} does not exist", product_id);
                }
            }
            "order" => {
                // For store orders, `product_id` carries the order id.
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM orders WHERE id = $1)")
                        .bind(product_id)
                        .fetch_one(&self.pool)
                        .await?;
                if !exists {
                    anyhow::bail!("Order {} does not exist", product_id);
                }
            }
            _ => {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1)")
                        .bind(product_id)
                        .fetch_one(&self.pool)
                        .await?;
                if !exists {
                    anyhow::bail!("Product {} does not exist", product_id);
                }
            }
        }

        Ok(())
    }
}

fn normalize_payment_metadata(metadata: Option<Value>) -> Result<Option<Value>> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };

    if !metadata.is_object() {
        anyhow::bail!("Payment metadata must be a JSON object");
    }

    let resource_type = payment_resource_type(Some(&metadata));
    if resource_type != "plan" && resource_type != "product" && resource_type != "order" {
        anyhow::bail!("Unsupported payment resource type: {}", resource_type);
    }

    if resource_type == "plan"
        && metadata
            .get("duration_days")
            .and_then(|value| value.as_i64())
            .is_some_and(|days| days <= 0)
    {
        anyhow::bail!("duration_days must be a positive integer");
    }

    Ok(Some(metadata))
}

fn payment_resource_type(metadata: Option<&Value>) -> &str {
    metadata
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("product")
}

#[cfg(test)]
mod tests {
    use super::{normalize_payment_metadata, payment_resource_type};
    use serde_json::json;

    #[test]
    fn defaults_missing_payment_type_to_product() {
        assert_eq!(payment_resource_type(None), "product");
        assert_eq!(payment_resource_type(Some(&json!({}))), "product");
    }

    #[test]
    fn rejects_non_object_metadata() {
        let result = normalize_payment_metadata(Some(json!(["plan"])));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_plan_duration() {
        let result = normalize_payment_metadata(Some(json!({
            "type": "plan",
            "duration_days": 0
        })));
        assert!(result.is_err());
    }
}
