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

        let (gift_id, plan_id, duration) =
            gift.ok_or_else(|| anyhow::anyhow!("Gift code already redeemed or expired"))?;

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

        // License gate (P4, contract E): this is the LIVE gift-code redemption path
        // (called via api/client.rs and bot/command.rs -> redeem_code). On a Free
        // instance (manual_approval) the new sub stays 'pending' until an admin
        // approves; Pro -> auto-'active'. Never touches existing subs.
        let limits = crate::license::effective_limits_from_pool(&self.pool).await;
        let initial_status = crate::license::initial_subscription_status(&limits);

        sqlx::query(
            "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at, is_trial) \
             VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, $6, CURRENT_TIMESTAMP, $7)",
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(initial_status)
        .bind(expires_at)
        .bind(plan_is_trial.unwrap_or(false))
        .execute(&mut *tx)
        .await?;

        // Фиксируем использование триала атомарно внутри транзакции.
        if plan_is_trial.unwrap_or(false) {
            sqlx::query("UPDATE users SET trial_used = TRUE, trial_used_at = NOW() WHERE id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(format!(
            "Gift subscription activated for {} days!",
            duration
        ))
    }

    async fn redeem_promo_code(&self, user_id: i64, promo: PromoCode) -> Result<String> {
        // Быстрая проверка срока действия до открытия транзакции
        if let Some(expiry) = promo.expires_at
            && expiry < Utc::now()
        {
            return Err(anyhow::anyhow!("Promo code has expired"));
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

                // License gate (P4, contract E): LIVE promo-code redemption path.
                // Free (manual_approval) -> 'pending' until admin approval; Pro ->
                // 'active'. Existing subs are never affected.
                let limits = crate::license::effective_limits_from_pool(&self.pool).await;
                let initial_status = crate::license::initial_subscription_status(&limits);

                sqlx::query(
                    "INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, status, activated_at, expires_at, created_at, is_trial) VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, $6, CURRENT_TIMESTAMP, $7)"
                )
                .bind(user_id)
                .bind(plan_id)
                .bind(vless_uuid)
                .bind(sub_uuid)
                .bind(initial_status)
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

    // ========================================================================
    // Partner per-source codes
    //
    // A partner mints several referral aliases (one per traffic source). Each
    // alias maps a new signup to the partner via the existing signup
    // attribution mechanism (users.referrer_id + users.signup_partner_code_id).
    // Money is never duplicated here: `conversions` and `balance_earned` are
    // derived from the referral_rewards ledger the normal referral path writes.
    // ========================================================================

    /// Whether a user holds the partner role (gate for /app/partner/*).
    pub async fn is_partner(&self, user_id: i64) -> Result<bool> {
        let flag: Option<bool> = sqlx::query_scalar("SELECT is_partner FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to read partner flag")?;
        Ok(flag.unwrap_or(false))
    }

    /// All partner codes owned by a user, with derived stats, newest first.
    ///
    /// Stats are computed by joining attributed users (signup_partner_code_id)
    /// and the referral_rewards ledger — nothing money-related is stored on
    /// partner_codes itself:
    ///   * clicks      — best-effort counter column (deep-link hits).
    ///   * signups     — COUNT(users WHERE signup_partner_code_id = code).
    ///   * conversions — those signups that have a first-purchase reward row.
    ///   * balance_earned — SUM of referral_rewards.amount_cents for them.
    pub async fn list_partner_codes(&self, partner_user_id: i64) -> Result<Vec<PartnerCodeStats>> {
        let rows = sqlx::query_as::<_, PartnerCodeStats>(
            r#"
            SELECT
                pc.code                                                   AS code,
                pc.source_label                                           AS source_label,
                pc.created_at                                             AS created_at,
                pc.clicks                                                 AS clicks,
                COUNT(DISTINCT u.id)                                      AS signups,
                COUNT(DISTINCT rr.referred_user_id)                       AS conversions,
                COALESCE(CAST(SUM(rr.amount_cents) AS BIGINT), 0)         AS balance_earned
            FROM partner_codes pc
            LEFT JOIN users u
                   ON u.signup_partner_code_id = pc.id
            LEFT JOIN referral_rewards rr
                   ON rr.referred_user_id = u.id
                  AND rr.referrer_id = pc.partner_user_id
            WHERE pc.partner_user_id = $1
            GROUP BY pc.id, pc.code, pc.source_label, pc.created_at, pc.clicks
            ORDER BY pc.created_at DESC
            "#,
        )
        .bind(partner_user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list partner codes")?;

        Ok(rows)
    }

    /// Stats for a single freshly-created code (so the create handler can return
    /// the same shape as the list). A new code has no signups yet.
    pub async fn get_partner_code(
        &self,
        partner_user_id: i64,
        code: &str,
    ) -> Result<Option<PartnerCodeStats>> {
        let row = sqlx::query_as::<_, PartnerCodeStats>(
            r#"
            SELECT
                pc.code                                                   AS code,
                pc.source_label                                           AS source_label,
                pc.created_at                                             AS created_at,
                pc.clicks                                                 AS clicks,
                COUNT(DISTINCT u.id)                                      AS signups,
                COUNT(DISTINCT rr.referred_user_id)                       AS conversions,
                COALESCE(CAST(SUM(rr.amount_cents) AS BIGINT), 0)         AS balance_earned
            FROM partner_codes pc
            LEFT JOIN users u
                   ON u.signup_partner_code_id = pc.id
            LEFT JOIN referral_rewards rr
                   ON rr.referred_user_id = u.id
                  AND rr.referrer_id = pc.partner_user_id
            WHERE pc.partner_user_id = $1 AND pc.code = $2
            GROUP BY pc.id, pc.code, pc.source_label, pc.created_at, pc.clicks
            "#,
        )
        .bind(partner_user_id)
        .bind(code)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch partner code")?;

        Ok(row)
    }

    /// Creates a partner code with a generated unique alias. Returns the stored
    /// `code`. Retries on the (extremely rare) UNIQUE collision.
    pub async fn create_partner_code(
        &self,
        partner_user_id: i64,
        source_label: &str,
    ) -> Result<String> {
        let label = source_label.trim();
        let label = if label.chars().count() > 64 {
            label.chars().take(64).collect::<String>()
        } else {
            label.to_string()
        };

        // Up to 5 attempts to dodge a UNIQUE(code) collision.
        for _ in 0..5 {
            let code = Self::generate_partner_code();
            let res = sqlx::query(
                "INSERT INTO partner_codes (code, partner_user_id, source_label) VALUES ($1, $2, $3)",
            )
            .bind(&code)
            .bind(partner_user_id)
            .bind(&label)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => return Ok(code),
                Err(e) => {
                    if let Some(db) = e.as_database_error()
                        && db.code().as_deref() == Some("23505")
                    {
                        continue; // collision — regenerate
                    }
                    return Err(anyhow::Error::from(e)).context("Failed to create partner code");
                }
            }
        }
        Err(anyhow::anyhow!("Could not allocate a unique partner code"))
    }

    /// Deletes a partner code, but only if it belongs to the given partner.
    /// Returns true if a row was removed. Attributed users keep their
    /// signup_partner_code_id NULLed (ON DELETE SET NULL) — historical earnings
    /// in referral_rewards are untouched.
    pub async fn delete_partner_code(&self, partner_user_id: i64, code: &str) -> Result<bool> {
        let affected =
            sqlx::query("DELETE FROM partner_codes WHERE code = $1 AND partner_user_id = $2")
                .bind(code)
                .bind(partner_user_id)
                .execute(&self.pool)
                .await
                .context("Failed to delete partner code")?
                .rows_affected();
        Ok(affected > 0)
    }

    /// Generates a partner alias of the form EXARO-XXXXXX (uppercase, no
    /// ambiguous chars). Matches the EXARO-* sample-code brand convention.
    fn generate_partner_code() -> String {
        use rand::Rng;
        const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        let mut rng = rand::rng();
        let suffix: String = (0..6)
            .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
            .collect();
        format!("EXARO-{}", suffix)
    }
}

/// Derived stats for one partner code. Field names match the partner contract
/// the Flutter client consumes (GET /app/partner/codes).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PartnerCodeStats {
    pub code: String,
    pub source_label: String,
    pub created_at: DateTime<Utc>,
    pub clicks: i64,
    pub signups: i64,
    pub conversions: i64,
    pub balance_earned: i64,
}
