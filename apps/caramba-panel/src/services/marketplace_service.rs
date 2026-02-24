use anyhow::{Context, Result};
use caramba_db::models::store::{PaymentSession, User};
use caramba_db::repositories::payment_session_repo::PaymentSessionRepository;
use chrono::Utc;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::payment::provider::{PaymentProvider, PaymentWebhookAction};
use super::payment::balance::BalanceProvider;
use super::payment::cryptobot::CryptoBotProvider;
use super::payment::manual::ManualProvider;
use super::payment::nowpayments::NowPaymentsProvider;
use super::payment::telegram_stars::StarsProvider;
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
}

impl MarketplaceService {
    pub fn new(
        pool: PgPool,
        nowpayments_key: String,
        nowpayments_ipn_secret: String,
        cryptobot_token: String,
        store_service: StoreService,
        sub_service: SubscriptionService,
    ) -> Self {
        let session_repo = PaymentSessionRepository::new(pool.clone());
        let http_client = reqwest::Client::new();
        let mut providers: HashMap<String, Box<dyn PaymentProvider>> = HashMap::new();

        providers.insert("stars".to_string(), Box::new(StarsProvider));
        providers.insert("manual".to_string(), Box::new(ManualProvider));

        if !nowpayments_key.is_empty() && !nowpayments_ipn_secret.is_empty() {
            providers.insert(
                "nowpayments".to_string(),
                Box::new(NowPaymentsProvider {
                    api_key: nowpayments_key,
                    ipn_secret: nowpayments_ipn_secret,
                }),
            );
        }

        if !cryptobot_token.is_empty() {
            providers.insert(
                "cryptobot".to_string(),
                Box::new(CryptoBotProvider {
                    token: cryptobot_token,
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
        }
    }

    pub fn get_provider(&self, name: &str) -> Option<&Box<dyn PaymentProvider>> {
        self.providers.get(name)
    }

    pub async fn create_session(
        &self,
        user: &User,
        product_id: i64,
        provider_name: &str,
        amount: i64,
        currency: &str,
    ) -> Result<(PaymentSession, String)> {
        let provider = self
            .get_provider(provider_name)
            .context("Payment provider not found or disabled")?;

        let session = PaymentSession {
            id: Uuid::new_v4(),
            user_id: user.id,
            product_id,
            provider: provider_name.to_string(),
            external_id: None,
            amount,
            currency: currency.to_string(),
            status: "pending".to_string(),
            metadata: None,
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
        let invoice_payload = provider.create_invoice(&session, user, &self.http_client).await?;

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
                // Find session by external_id and complete
                if let Ok(Some(session)) = self.session_repo.get_by_external_id(&external_id).await {
                    self.fulfill_payment(session.id).await?;
                }
            }
            PaymentWebhookAction::Failed { reason: _ } => {
                // Find session by external_id and fail
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn fulfill_payment(&self, session_id: Uuid) -> Result<()> {
        // 1. Fetch session
        let session = self.session_repo.get_by_id(session_id).await?.context("Session not found")?;

        // 2. Prevent double fulfillment
        if session.status == "completed" {
            tracing::warn!("Session {} is already completed, ignoring.", session_id);
            return Ok(());
        }

        // 3. Mark as completed
        self.session_repo.update_status(session_id, "completed").await?;

        // 4. Provision Product
        let products = self.store_service.get_all_products().await?;
        let product = products.into_iter().find(|p| p.id == session.product_id).context("Product not found")?;

        if product.product_type == "plan" {
            // Determine days to add
            let mut days_to_add = 30; // Default
            
            if let Some(meta) = &session.metadata {
                if let Some(days) = meta.get("duration_days").and_then(|v| v.as_i64()) {
                    days_to_add = days as i32;
                }
            } else {
                // Fallback: search plan_durations for this plan_id with this price
                let duration_row: Option<(i32,)> = sqlx::query_as("SELECT duration_days FROM plan_durations WHERE plan_id = $1 AND price = $2 LIMIT 1")
                    .bind(session.product_id)
                    .bind(session.amount)
                    .fetch_optional(&self.pool)
                    .await
                    .unwrap_or(None);
                
                if let Some((d,)) = duration_row {
                    days_to_add = d;
                }
            }

            // Find the active subscription or create one
            let subs = self.sub_service.get_user_subscriptions(session.user_id).await?;

            tracing::info!("Fulfilling VPN Plan for user {}: extending by {} days", session.user_id, days_to_add);

            if let Some(active_sub) = subs.first() {
                self.sub_service.admin_extend(active_sub.sub.id, days_to_add).await?;
            } else {
                // Create new subscription
                let _ = self.store_service.admin_gift_subscription(session.user_id, session.product_id, days_to_add).await?;
            }
        }

        Ok(())
    }
}
