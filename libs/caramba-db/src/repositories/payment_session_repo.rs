use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::store::PaymentSession;

#[derive(Debug, Clone)]
pub struct PaymentSessionRepository {
    pool: PgPool,
}

impl PaymentSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, session: &PaymentSession) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO payment_sessions
                (id, user_id, product_id, provider, external_id, amount, currency, status, metadata, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(session.id)
        .bind(session.user_id)
        .bind(session.product_id)
        .bind(&session.provider)
        .bind(&session.external_id)
        .bind(session.amount)
        .bind(&session.currency)
        .bind(&session.status)
        .bind(&session.metadata)
        .bind(session.created_at)
        .bind(session.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_id(&self, id: Uuid) -> Result<Option<PaymentSession>> {
        let session = sqlx::query_as::<_, PaymentSession>(
            "SELECT * FROM payment_sessions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(session)
    }

    pub async fn get_by_external_id(&self, external_id: &str) -> Result<Option<PaymentSession>> {
        let session = sqlx::query_as::<_, PaymentSession>(
            "SELECT * FROM payment_sessions WHERE external_id = $1",
        )
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(session)
    }

    pub async fn update_status(&self, id: Uuid, status: &str) -> Result<()> {
        sqlx::query("UPDATE payment_sessions SET status = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_external_id(&self, id: Uuid, external_id: &str) -> Result<()> {
        sqlx::query("UPDATE payment_sessions SET external_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(external_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_metadata(&self, id: Uuid, metadata: &serde_json::Value) -> Result<()> {
        sqlx::query("UPDATE payment_sessions SET metadata = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(metadata)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_pending_by_provider(&self, provider: &str) -> Result<Vec<PaymentSession>> {
        let sessions = sqlx::query_as::<_, PaymentSession>(
            "SELECT * FROM payment_sessions WHERE provider = $1 AND status = 'pending'",
        )
        .bind(provider)
        .fetch_all(&self.pool)
        .await?;
        Ok(sessions)
    }
}
