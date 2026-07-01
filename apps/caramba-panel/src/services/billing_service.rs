use anyhow::{Context, Result};
use caramba_db::models::promo::PromoCode;
use chrono::Utc;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct BillingService {
    pool: PgPool,
}

impl BillingService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Записывает платёж в таблицу payments.
    /// Возвращает `true` если запись была создана (новый платёж),
    /// `false` если запись уже существует — идемпотентный конфликт по (method, external_id).
    pub async fn log_payment(
        &self,
        user_id: i64,
        method: &str,
        amount_cents: i64,
        external_id: Option<&str>,
        status: &str,
    ) -> Result<bool> {
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO payments (user_id, method, amount, external_id, status) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (method, external_id) WHERE external_id IS NOT NULL \
             DO NOTHING RETURNING id",
        )
        .bind(user_id)
        .bind(method)
        .bind(amount_cents)
        .bind(external_id)
        .bind(status)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to log payment")?;
        Ok(inserted.is_some())
    }

    /// DEPRECATED / DEAD — divergent copy of the old per-payment referral
    /// granting (percent of every payment -> referrer balance, logged as
    /// referral_bonuses 'payment', ignoring user_referral_rates). It is not on
    /// the live payment path and is superseded by the money model
    /// (ReferralService::apply_first_purchase_reward, granted at fulfillment in
    /// MarketplaceService). Kept as a no-op shim to preserve the signature for
    /// any stale caller without re-introducing the old traffic-replaced reward
    /// or tripping the dropped referral_bonuses uniqueness on repeat payments.
    #[allow(dead_code)]
    pub async fn apply_referral_bonus(
        &self,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _user_id: i64,
        _amount_cents: i64,
        _payment_id: Option<i64>,
    ) -> Result<Option<(i64, i64)>> {
        Ok(None)
    }

    pub async fn validate_promo(&self, code: &str) -> Result<Option<PromoCode>> {
        sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes WHERE code = $1 AND (expires_at IS NULL OR expires_at > $2) AND current_uses < max_uses AND is_active = TRUE"
        )
        .bind(code)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await
        .context("Failed to validate promo code")
    }

    pub async fn add_balance(&self, user_id: i64, amount_cents: i64) -> Result<()> {
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(amount_cents)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get recent orders for dashboard (limit 10)
    pub async fn get_recent_orders(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::handlers::admin::dashboard::OrderWithUser>> {
        use crate::handlers::admin::dashboard::OrderWithUser;
        let orders = sqlx::query_as::<_, OrderWithUser>(
            r#"
            SELECT o.id, COALESCE(u.username, u.full_name, 'Unknown') as username, 
                   to_char(o.total_amount::numeric / 100.0, 'FM999999990.00') as total_amount,
                   o.status, o.created_at
            FROM orders o
            LEFT JOIN users u ON o.user_id = u.id
            ORDER BY o.created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent orders")?;

        Ok(orders)
    }

    /// Get all orders for transactions page (capped at 1000)
    pub async fn get_all_orders(
        &self,
    ) -> Result<Vec<crate::handlers::admin::dashboard::OrderWithUser>> {
        use crate::handlers::admin::dashboard::OrderWithUser;
        let orders = sqlx::query_as::<_, OrderWithUser>(
            r#"
            SELECT o.id, COALESCE(u.username, u.full_name, 'Unknown') as username,
                   to_char(o.total_amount::numeric / 100.0, 'FM999999990.00') as total_amount,
                   o.status, o.created_at
            FROM orders o
            LEFT JOIN users u ON o.user_id = u.id
            ORDER BY o.created_at DESC
            LIMIT 1000
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch all orders")?;

        Ok(orders)
    }

    pub async fn get_user_orders(
        &self,
        user_id: i64,
    ) -> Result<Vec<caramba_db::models::store::Order>> {
        use caramba_db::models::store::Order;
        let orders = sqlx::query_as::<_, Order>(
            "SELECT id, user_id, total_amount, status, created_at, paid_at FROM orders WHERE user_id = $1 ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user orders")?;
        Ok(orders)
    }
}
