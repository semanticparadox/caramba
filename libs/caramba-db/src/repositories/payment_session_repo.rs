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
        let session =
            sqlx::query_as::<_, PaymentSession>("SELECT * FROM payment_sessions WHERE id = $1")
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
        sqlx::query(
            "UPDATE payment_sessions SET status = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
        )
        .bind(status)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically claim a session for fulfillment. Exactly one caller wins the
    /// transition to `completed`; concurrent callers (a duplicate provider
    /// webhook, a webhook retry, or a race with the lost-webhook poller) get
    /// `rows_affected == 0` and must bail WITHOUT any side effect. Returns
    /// `true` only for the caller that won the claim.
    ///
    /// `expired` is claimable on purpose. That status is OUR bookkeeping guess
    /// (a daily sweep ages out abandoned checkouts after 24h), while crypto
    /// providers keep an invoice payable far longer — NOWPayments tracks a
    /// payment for 7 days. A customer who pays on day two produces a genuine
    /// paid webhook against a session we already swept; refusing it would take
    /// the money and grant nothing. Only `completed` is final, which is what
    /// keeps fulfillment idempotent.
    pub async fn claim_for_fulfillment(&self, id: Uuid) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE payment_sessions SET status = 'completed', updated_at = CURRENT_TIMESTAMP \
             WHERE id = $1 AND status IN ('pending', 'expired')",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
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

    /// List still-`pending` sessions created within the last `max_age_hours`
    /// (i.e. not yet stale-expired) for the webhook-loss polling fallback (U20/U13).
    ///
    /// Bounded by recency so the poller never hammers provider APIs for ancient
    /// abandoned checkouts (those are swept to `expired` by the daily job), and
    /// `LIMIT`ed so a backlog can't blow up a single tick. Oldest-first so the
    /// sessions closest to expiry — the ones most at risk of a permanently lost
    /// webhook — are reconciled before the cap is hit.
    pub async fn list_pending_recent(
        &self,
        max_age_hours: i64,
        limit: i64,
    ) -> Result<Vec<PaymentSession>> {
        let sessions = sqlx::query_as::<_, PaymentSession>(
            "SELECT * FROM payment_sessions \
             WHERE status = 'pending' \
               AND created_at > CURRENT_TIMESTAMP - ($1 || ' hours')::INTERVAL \
             ORDER BY created_at ASC \
             LIMIT $2",
        )
        .bind(max_age_hours.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(sessions)
    }

    /// Read-only audit feed for the daily reconciliation pass (U21).
    ///
    /// Returns `(id, provider, status, amount, currency, external_id, created_at)`
    /// for sessions touched in the last `lookback_hours`, across the terminal and
    /// open states the auditor cares about. Deliberately a thin projection (no
    /// model dependency) so the monitoring layer can scan for divergence without
    /// mutating anything.
    #[allow(clippy::type_complexity)]
    pub async fn list_recent_for_audit(
        &self,
        lookback_hours: i64,
    ) -> Result<
        Vec<(
            Uuid,
            String,
            String,
            i64,
            String,
            Option<String>,
            chrono::DateTime<chrono::Utc>,
        )>,
    > {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                i64,
                String,
                Option<String>,
                chrono::DateTime<chrono::Utc>,
            ),
        >(
            "SELECT id, provider, status, amount, currency, external_id, created_at \
             FROM payment_sessions \
             WHERE created_at > CURRENT_TIMESTAMP - ($1 || ' hours')::INTERVAL \
             ORDER BY created_at DESC",
        )
        .bind(lookback_hours.to_string())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
