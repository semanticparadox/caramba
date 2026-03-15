use anyhow::{Context, Result};
use caramba_db::models::store::{PaymentSession, User};
use caramba_db::repositories::payment_session_repo::PaymentSessionRepository;
use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::payment::provider::{PaymentProvider, PaymentWebhookAction};
use super::payment::cryptomus::CryptomusProvider;
use super::payment::lava::LavaProvider;
use super::payment::aaio::AaioProvider;
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
        cryptomus_merchant_id: String,
        cryptomus_api_key: String,
        lava_project_id: String,
        lava_secret_key: String,
        aaio_merchant_id: String,
        aaio_secret_1: String,
        aaio_secret_2: String,
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

        if !cryptomus_merchant_id.is_empty() && !cryptomus_api_key.is_empty() {
            providers.insert(
                "cryptomus".to_string(),
                Box::new(CryptomusProvider {
                    merchant_id: cryptomus_merchant_id,
                    api_key: cryptomus_api_key,
                }),
            );
        }

        if !lava_project_id.is_empty() && !lava_secret_key.is_empty() {
            providers.insert(
                "lava".to_string(),
                Box::new(LavaProvider {
                    project_id: lava_project_id,
                    secret_key: lava_secret_key,
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
        metadata: Option<serde_json::Value>,
    ) -> Result<(PaymentSession, String)> {
        let provider = self
            .get_provider(provider_name)
            .context("Payment provider not found or disabled")?;

        let metadata = normalize_payment_metadata(metadata)?;
        self.validate_session_resource(product_id, metadata.as_ref()).await?;

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

        self.validate_session_resource(session.product_id, session.metadata.as_ref())
            .await?;

        let fulfillment_result = self.fulfill_session_resource(&session).await;
        match fulfillment_result {
            Ok(()) => {
                self.session_repo.update_status(session_id, "completed").await?;
                Ok(())
            }
            Err(error) => {
                let _ = self.session_repo.update_status(session_id, "failed").await;
                Err(error)
            }
        }
    }

    async fn fulfill_session_resource(&self, session: &PaymentSession) -> Result<()> {
        let resource_type = payment_resource_type(session.metadata.as_ref());

        if resource_type == "plan" {
            let days_to_add = self.resolve_plan_duration_days(session).await?;
            let subs = self.sub_service.get_user_subscriptions(session.user_id).await?;

            tracing::info!(
                "Fulfilling VPN Plan for user {}: extending by {} days",
                session.user_id,
                days_to_add
            );

            if let Some(active_sub) = subs.first() {
                self.sub_service.admin_extend(active_sub.sub.id, days_to_add).await?;
            } else {
                let _ = self
                    .store_service
                    .admin_gift_subscription(session.user_id, session.product_id, days_to_add)
                    .await?;
            }

            return Ok(());
        }

        let products = self.store_service.get_all_products().await?;
        let product = products
            .into_iter()
            .find(|p| p.id == session.product_id)
            .context("Product not found")?;

        tracing::info!(
            "Fulfilling Product {} for user {}",
            product.name,
            session.user_id
        );
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
                let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plans WHERE id = $1)")
                    .bind(product_id)
                    .fetch_one(&self.pool)
                    .await?;
                if !exists {
                    anyhow::bail!("Plan {} does not exist", product_id);
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
    if resource_type != "plan" && resource_type != "product" {
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
