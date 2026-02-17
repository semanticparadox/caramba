use sqlx::PgPool;
use anyhow::{Result, Context};
use chrono::{DateTime, Utc};
use caramba_db::models::promo::{PromoCode, PromoCodeUsage};

#[derive(Debug, Clone)]
pub struct PromoService {
    pool: PgPool,
}

impl PromoService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Unified redemption: Checks Gift Codes first, then Promo Codes.
    pub async fn redeem_code(&self, user_id: i64, code: &str) -> Result<String> {
        let code = code.trim().to_uppercase();
        
        // 1. Check Gift Codes (User-to-User Single Use)
        let gift: Option<(i64, i64, i32)> = sqlx::query_as::<_, (i64, i64, i32)>(
            "SELECT id, plan_id, duration_days FROM gift_codes WHERE code = $1 AND redeemed_by_user_id IS NULL"
        )
        .bind(&code)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((gift_id, plan_id, duration)) = gift {
            return self.redeem_gift_code(user_id, gift_id, plan_id, duration).await;
        }

        // 2. Check Promo Codes (Admin/Promoter Multi-Use)
        let promo = sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type as promo_type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes WHERE code = $1 AND is_active = TRUE"
        )
        .bind(&code)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(promo) = promo {
            return self.redeem_promo_code(user_id, promo).await;
        }

        Err(anyhow::anyhow!("Code not found or already used"))
    }

    async fn redeem_gift_code(&self, user_id: i64, gift_id: i64, plan_id: i64, duration: i32) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        
        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = $1, redeemed_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(user_id)
            .bind(gift_id)
            .execute(&mut *tx)
            .await?;

        let _traffic_gb: i64 = sqlx::query_scalar("SELECT traffic_limit_gb FROM plans WHERE id = $1")
            .bind(plan_id)
            .fetch_one(&mut *tx)
            .await?;

        let expires_at = Utc::now() + chrono::Duration::days(duration as i64);
        let vless_uuid = uuid::Uuid::new_v4().to_string();
        let sub_uuid = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at) VALUES ($1, $2, $3, $4, 'active', CURRENT_TIMESTAMP, $5, CURRENT_TIMESTAMP)"
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(format!("Gift subscription activated for {} days!", duration))
    }

    async fn redeem_promo_code(&self, user_id: i64, promo: PromoCode) -> Result<String> {
        if let Some(expiry) = promo.expires_at {
            if expiry < Utc::now() {
                return Err(anyhow::anyhow!("Promo code has expired"));
            }
        }

        if promo.current_uses >= promo.max_uses {
            return Err(anyhow::anyhow!("Promo code reached maximum uses"));
        }

        let usage_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM promo_code_usage WHERE promo_code_id = $1 AND user_id = $2)")
            .bind(promo.id)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;

        if usage_exists {
            return Err(anyhow::anyhow!("You have already used this promo code"));
        }

        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE promo_codes SET current_uses = current_uses + 1 WHERE id = $1")
            .bind(promo.id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("INSERT INTO promo_code_usage (promo_code_id, user_id) VALUES ($1, $2)")
            .bind(promo.id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let msg = match promo.promo_type.as_str() {
            "balance" => {
                let amount = promo.balance_amount.unwrap_or(0);
                sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                    .bind(amount)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                format!("Success! Received {} credits to balance.", amount)
            }
            "subscription" | "trial" => {
                let plan_id = promo.plan_id.ok_or_else(|| anyhow::anyhow!("Missing plan for subscription promo"))?;
                let duration = promo.duration_days.unwrap_or(7);
                
                let expires_at = Utc::now() + chrono::Duration::days(duration as i64);
                let vless_uuid = uuid::Uuid::new_v4().to_string();
                let sub_uuid = uuid::Uuid::new_v4().to_string();

                sqlx::query(
                    "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at) VALUES ($1, $2, $3, $4, 'active', CURRENT_TIMESTAMP, $5, CURRENT_TIMESTAMP)"
                )
                .bind(user_id)
                .bind(plan_id)
                .bind(vless_uuid)
                .bind(sub_uuid)
                .bind(expires_at)
                .execute(&mut *tx)
                .await?;
                format!("Promo activated! New subscription for {} days.", duration)
            }
            _ => return Err(anyhow::anyhow!("Unknown promo type")),
        };

        tx.commit().await?;
        Ok(msg)
    }

    pub async fn list_promos(&self) -> Result<Vec<PromoCode>> {
        sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type as promo_type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list promos")
    }

    pub async fn get_promo_usages(&self, promo_id: i64) -> Result<Vec<PromoCodeUsage>> {
        sqlx::query_as::<_, PromoCodeUsage>(
            "SELECT id, promo_code_id, user_id, used_at FROM promo_code_usage WHERE promo_code_id = $1 ORDER BY used_at DESC"
        )
        .bind(promo_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch promo usages")
    }

    pub async fn create_promo(&self, code: &str, p_type: &str, plan_id: Option<i64>, balance: Option<i32>, duration: Option<i32>, traffic: Option<i32>, max_uses: i32, expires_at: Option<DateTime<Utc>>, admin_id: i64) -> Result<i64> {
        let id: i64 = sqlx::query_scalar("INSERT INTO promo_codes (code, type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, expires_at, created_by_admin_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id")
            .bind(code.trim().to_uppercase())
            .bind(p_type)
            .bind(plan_id)
            .bind(balance)
            .bind(duration)
            .bind(traffic)
            .bind(max_uses)
            .bind(expires_at)
            .bind(admin_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(id)
    }
}
