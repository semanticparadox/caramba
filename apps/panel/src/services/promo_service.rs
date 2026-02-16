use sqlx::SqlitePool;
use anyhow::{Result, Context};
use tracing::{info, error};
use chrono::Utc;
use crate::models::promo::{PromoCode, PromoCodeUsage};

#[derive(Debug, Clone)]
pub struct PromoService {
    pool: SqlitePool,
}

impl PromoService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Unified redemption: Checks Gift Codes first, then Promo Codes.
    pub async fn redeem_code(&self, user_id: i64, code: &str) -> Result<String> {
        let code = code.trim().to_uppercase();
        
        // 1. Check Gift Codes (User-to-User Single Use)
        let gift: Option<(i64, i64, i32)> = sqlx::query_as::<(i64, i64, i32)>(
            "SELECT id, plan_id, duration_days FROM gift_codes WHERE code = ? AND redeemed_by_user_id IS NULL"
        )
        .bind(&code)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((gift_id, plan_id, duration)) = gift {
            return self.redeem_gift_code(user_id, gift_id, plan_id, duration).await;
        }

        // 2. Check Promo Codes (Admin/Promoter Multi-Use)
        let promo = sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type as promo_type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes WHERE code = ? AND is_active = 1"
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
        
        // Mark as used
        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = ?, redeemed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .bind(gift_id)
            .execute(&mut *tx)
            .await?;

        // Apply subscription (using SubscriptionService logic or direct SQL)
        // For simplicity and to avoid circular dependency if any, we do it here or call a method
        // But better use SQL to ensure atomicity in this transaction
        
        // 1. Get Plan info
        let traffic_gb: i64 = sqlx::query_scalar("SELECT traffic_limit_gb FROM plans WHERE id = ?")
            .bind(plan_id)
            .fetch_one(&mut *tx)
            .await?;

        let expires_at = Utc::now() + chrono::Duration::days(duration as i64);
        let vless_uuid = uuid::Uuid::new_v4().to_string();
        let sub_uuid = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at) VALUES (?, ?, ?, ?, 'active', CURRENT_TIMESTAMP, ?, CURRENT_TIMESTAMP)"
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
        // 1. Expiration check
        if let Some(expiry) = promo.expires_at {
            if expiry < Utc::now() {
                return Err(anyhow::anyhow!("Promo code has expired"));
            }
        }

        // 2. Max Uses check
        if promo.current_uses >= promo.max_uses {
            return Err(anyhow::anyhow!("Promo code reached maximum uses"));
        }

        // 3. User already used check (Prevent double-dipping)
        let usage_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM promo_code_usage WHERE promo_code_id = ? AND user_id = ?)")
            .bind(promo.id)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;

        if usage_exists {
            return Err(anyhow::anyhow!("You have already used this promo code"));
        }

        let mut tx = self.pool.begin().await?;

        // 4. Update uses
        sqlx::query("UPDATE promo_codes SET current_uses = current_uses + 1 WHERE id = ?")
            .bind(promo.id)
            .execute(&mut *tx)
            .await?;

        // 5. Log usage
        sqlx::query("INSERT INTO promo_code_usage (promo_code_id, user_id) VALUES (?, ?)")
            .bind(promo.id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // 6. Apply effect
        let msg = match promo.promo_type.as_str() {
            "balance" => {
                let amount = promo.balance_amount.unwrap_or(0);
                sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
                    .bind(amount)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                format!("Success! Received {} credits to balance.", amount)
            }
            "subscription" | "trial" => {
                let plan_id = promo.plan_id.ok_or_else(|| anyhow::anyhow!("Missing plan for subscription promo"))?;
                let duration = promo.duration_days.unwrap_or(7);
                
                // Creation logic (similar to gift)
                let expires_at = Utc::now() + chrono::Duration::days(duration as i64);
                let vless_uuid = uuid::Uuid::new_v4().to_string();
                let sub_uuid = uuid::Uuid::new_v4().to_string();

                sqlx::query(
                    "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at) VALUES (?, ?, ?, ?, 'active', CURRENT_TIMESTAMP, ?, CURRENT_TIMESTAMP)"
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

    pub async fn create_promo(&self, code: &str, p_type: &str, plan_id: Option<i64>, balance: Option<i32>, duration: Option<i32>, traffic: Option<i32>, max_uses: i32, expires_at: Option<DateTime<Utc>>, admin_id: i64) -> Result<i64> {
        let id = sqlx::query("INSERT INTO promo_codes (code, type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, expires_at, created_by_admin_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(code.trim().to_uppercase())
            .bind(p_type)
            .bind(plan_id)
            .bind(balance)
            .bind(duration)
            .bind(traffic)
            .bind(max_uses)
            .bind(expires_at)
            .bind(admin_id)
            .execute(&self.pool)
            .await?
            .last_insert_rowid();
        Ok(id)
    }
}
