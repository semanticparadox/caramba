use anyhow::{Context, Result};
use caramba_db::models::promo::{PromoCode, PromoCodeUsage};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

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
        // Предварительная проверка без блокировки — только чтобы решить, идти ли дальше.
        // Фактическая блокировка FOR UPDATE выполняется внутри redeem_gift_code.
        let gift_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM gift_codes
                WHERE code = $1
                  AND redeemed_by_user_id IS NULL
                  AND COALESCE(status, 'active') = 'active'
                  AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
             )",
        )
        .bind(&code)
        .fetch_one(&self.pool)
        .await?;

        if gift_exists {
            return self.redeem_gift_code(user_id, &code).await;
        }

        // 2. Check Promo Codes (Admin/Promoter Multi-Use)
        let promo = sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes WHERE code = $1 AND is_active = TRUE"
        )
        .bind(&code)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(promo) = promo {
            return self.redeem_promo_code(user_id, promo).await;
        }

        Err(anyhow::anyhow!("Code not found or already used"))
    }

    async fn redeem_gift_code(&self, user_id: i64, code: &str) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        // Блокируем строку FOR UPDATE внутри транзакции, чтобы предотвратить
        // двойное погашение при конкурентных запросах (race condition)
        let gift: Option<(i64, i64, i32)> = sqlx::query_as::<_, (i64, i64, i32)>(
            "SELECT id, plan_id, duration_days
             FROM gift_codes
             WHERE code = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
             FOR UPDATE",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let (gift_id, plan_id, duration) = gift
            .ok_or_else(|| anyhow::anyhow!("Gift code already redeemed or expired"))?;

        // Проверяем, является ли план, привязанный к подарочному коду, пробным.
        // Подарочный код на пробный тариф тоже нельзя активировать дважды.
        let plan_is_trial: Option<bool> =
            sqlx::query_scalar("SELECT is_trial FROM plans WHERE id = $1")
                .bind(plan_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();

        if plan_is_trial.unwrap_or(false) {
            let trial_used: Option<bool> =
                sqlx::query_scalar("SELECT trial_used FROM users WHERE id = $1")
                    .bind(user_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .flatten();

            if trial_used.unwrap_or(false) {
                return Err(anyhow::anyhow!(
                    "Trial already used. You can only activate the trial period once."
                ));
            }
        }

        sqlx::query(
            "UPDATE gift_codes SET redeemed_by_user_id = $1, redeemed_at = CURRENT_TIMESTAMP WHERE id = $2",
        )
        .bind(user_id)
        .bind(gift_id)
        .execute(&mut *tx)
        .await?;

        let expires_at = Utc::now() + chrono::Duration::days(duration as i64);
        let vless_uuid = uuid::Uuid::new_v4().to_string();
        let sub_uuid = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at, is_trial) \
             VALUES ($1, $2, $3, $4, 'active', CURRENT_TIMESTAMP, $5, CURRENT_TIMESTAMP, $6)",
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(expires_at)
        .bind(plan_is_trial.unwrap_or(false))
        .execute(&mut *tx)
        .await?;

        // Фиксируем использование триала атомарно внутри транзакции.
        if plan_is_trial.unwrap_or(false) {
            sqlx::query(
                "UPDATE users SET trial_used = TRUE, trial_used_at = NOW() WHERE id = $1",
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(format!("Gift subscription activated for {} days!", duration))
    }

    async fn redeem_promo_code(&self, user_id: i64, promo: PromoCode) -> Result<String> {
        // Быстрая проверка срока действия до открытия транзакции
        if let Some(expiry) = promo.expires_at {
            if expiry < Utc::now() {
                return Err(anyhow::anyhow!("Promo code has expired"));
            }
        }

        let mut tx = self.pool.begin().await?;

        // Перечитываем строку с блокировкой FOR UPDATE, чтобы предотвратить
        // конкурентные списания сверх max_uses (race condition)
        let locked = sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active \
             FROM promo_codes WHERE id = $1 AND is_active = TRUE FOR UPDATE"
        )
        .bind(promo.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Promo code no longer active"))?;

        if locked.current_uses >= locked.max_uses {
            return Err(anyhow::anyhow!("Promo code reached maximum uses"));
        }

        let usage_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM promo_code_usage WHERE promo_code_id = $1 AND user_id = $2)"
        )
        .bind(locked.id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        if usage_exists {
            return Err(anyhow::anyhow!("You have already used this promo code"));
        }

        sqlx::query("UPDATE promo_codes SET current_uses = current_uses + 1 WHERE id = $1")
            .bind(locked.id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("INSERT INTO promo_code_usage (promo_code_id, user_id) VALUES ($1, $2)")
            .bind(locked.id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let msg = match locked.promo_type.as_str() {
            "balance" => {
                let amount = locked.balance_amount.unwrap_or(0);
                sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                    .bind(amount)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                format!("Success! Received {} credits to balance.", amount)
            }
            "subscription" => {
                let plan_id = locked
                    .plan_id
                    .ok_or_else(|| anyhow::anyhow!("Missing plan for subscription promo"))?;
                let duration = locked.duration_days.unwrap_or(7);

                // Проверяем, является ли план пробным — промо-подписку на пробный тариф
                // нельзя активировать дважды (защита от повторного получения триала).
                let plan_is_trial: Option<bool> =
                    sqlx::query_scalar("SELECT is_trial FROM plans WHERE id = $1")
                        .bind(plan_id)
                        .fetch_optional(&mut *tx)
                        .await?
                        .flatten();

                if plan_is_trial.unwrap_or(false) {
                    let trial_used: Option<bool> =
                        sqlx::query_scalar("SELECT trial_used FROM users WHERE id = $1")
                            .bind(user_id)
                            .fetch_optional(&mut *tx)
                            .await?
                            .flatten();

                    if trial_used.unwrap_or(false) {
                        return Err(anyhow::anyhow!(
                            "Trial already used. You can only activate the trial period once."
                        ));
                    }
                }

                let expires_at = Utc::now() + chrono::Duration::days(duration as i64);
                let vless_uuid = uuid::Uuid::new_v4().to_string();
                let sub_uuid = uuid::Uuid::new_v4().to_string();

                sqlx::query(
                    "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at, is_trial) VALUES ($1, $2, $3, $4, 'active', CURRENT_TIMESTAMP, $5, CURRENT_TIMESTAMP, $6)"
                )
                .bind(user_id)
                .bind(plan_id)
                .bind(vless_uuid)
                .bind(sub_uuid)
                .bind(expires_at)
                .bind(plan_is_trial.unwrap_or(false))
                .execute(&mut *tx)
                .await?;

                // Фиксируем факт использования триала внутри транзакции,
                // чтобы флаг был установлен атомарно с созданием подписки.
                if plan_is_trial.unwrap_or(false) {
                    sqlx::query(
                        "UPDATE users SET trial_used = TRUE, trial_used_at = NOW() WHERE id = $1",
                    )
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                }

                format!("Promo activated! New subscription for {} days.", duration)
            }
            _ => return Err(anyhow::anyhow!("Unknown promo type")),
        };

        tx.commit().await?;
        Ok(msg)
    }

    pub async fn list_promos(&self) -> Result<Vec<PromoCode>> {
        sqlx::query_as::<_, PromoCode>(
            "SELECT id, code, type, plan_id, balance_amount, duration_days, traffic_gb, max_uses, current_uses, expires_at, created_at, created_by_admin_id, promoter_user_id, is_active FROM promo_codes ORDER BY created_at DESC"
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

    pub async fn create_promo(
        &self,
        code: &str,
        p_type: &str,
        plan_id: Option<i64>,
        balance: Option<i32>,
        duration: Option<i32>,
        traffic: Option<i32>,
        max_uses: i32,
        expires_at: Option<DateTime<Utc>>,
        admin_id: i64,
    ) -> Result<i64> {
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
