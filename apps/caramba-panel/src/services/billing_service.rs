use sqlx::PgPool;
use anyhow::{Context, Result};
use caramba_db::models::store::User;
use caramba_db::models::promo::PromoCode;
use chrono::Utc;


#[derive(Debug, Clone)]
pub struct BillingService {
    pool: PgPool,
}

impl BillingService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn log_payment(&self, user_id: i64, method: &str, amount_cents: i64, external_id: Option<&str>, status: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO payments (user_id, method, amount, external_id, status) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(user_id)
        .bind(method)
        .bind(amount_cents)
        .bind(external_id)
        .bind(status)
        .execute(&self.pool)
        .await
        .context("Failed to log payment")?;
        Ok(())
    }

    pub async fn apply_referral_bonus(&self, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, user_id: i64, amount_cents: i64, payment_id: Option<i64>) -> Result<Option<(i64, i64)>> {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await?;
        
        if let Some(referrer_id) = user.referrer_id {
            let bonus = amount_cents / 10; // 10%
            if bonus > 0 {
                sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                    .bind(bonus)
                    .bind(referrer_id)
                    .execute(&mut **tx)
                    .await?;
                
                sqlx::query("INSERT INTO referral_bonuses (referrer_id, referred_id, amount, payment_id) VALUES ($1, $2, $3, $4)")
                    .bind(referrer_id)
                    .bind(user_id)
                    .bind(bonus)
                    .bind(payment_id)
                    .execute(&mut **tx)
                    .await?;

                let referrer_tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                    .bind(referrer_id)
                    .fetch_optional(&mut **tx)
                    .await?;
                
                if let Some(tg_id) = referrer_tg_id {
                    return Ok(Some((tg_id, bonus)));
                }
            }
        }
        Ok(None)
    }

    pub async fn validate_promo(&self, code: &str) -> Result<Option<PromoCode>> {
        sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type as promo_type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes WHERE code = $1 AND (expires_at IS NULL OR expires_at > $2) AND current_uses < max_uses AND is_active = TRUE"
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
    pub async fn get_recent_orders(&self, limit: i64) -> Result<Vec<crate::handlers::admin::dashboard::OrderWithUser>> {
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
            "#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent orders")?;
        
        Ok(orders)
    }

    /// Get all orders for transactions page
    pub async fn get_all_orders(&self) -> Result<Vec<crate::handlers::admin::dashboard::OrderWithUser>> {
        use crate::handlers::admin::dashboard::OrderWithUser;
        let orders = sqlx::query_as::<_, OrderWithUser>(
            r#"
            SELECT o.id, COALESCE(u.username, u.full_name, 'Unknown') as username, 
                   to_char(o.total_amount::numeric / 100.0, 'FM999999990.00') as total_amount,
                   o.status, o.created_at
            FROM orders o
            LEFT JOIN users u ON o.user_id = u.id
            ORDER BY o.created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch all orders")?;
        
        Ok(orders)
    }

    pub async fn get_user_orders(&self, user_id: i64) -> Result<Vec<caramba_db::models::store::Order>> {
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
