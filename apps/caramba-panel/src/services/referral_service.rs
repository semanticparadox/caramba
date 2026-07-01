use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize, sqlx::FromRow)]
pub struct LeaderboardEntry {
    pub username: Option<String>,
    pub referral_count: i64,
}

#[derive(Serialize)]
pub struct LeaderboardDisplayEntry {
    pub rank: usize,
    pub username: String,
    pub referral_count: i64,
    pub medal: Option<String>,
}

pub struct ReferralService;

impl ReferralService {
    /// Get top referrers
    pub async fn get_leaderboard(
        pool: &PgPool,
        limit: i64,
    ) -> Result<Vec<LeaderboardDisplayEntry>> {
        // Query to count referrals per user
        let rows: Vec<LeaderboardEntry> = sqlx::query_as(
            r#"
            SELECT
                u.username,
                COUNT(r.id) as referral_count
            FROM users u
            JOIN users r ON COALESCE(r.referrer_id, r.referred_by) = u.id
            GROUP BY u.id, u.username
            ORDER BY referral_count DESC
            LIMIT $1
        "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let mut display_rows = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let rank = index + 1;
            let medal = match rank {
                1 => Some("🥇".to_string()),
                2 => Some("🥈".to_string()),
                3 => Some("🥉".to_string()),
                _ => None,
            };

            let safe_username = row.username.unwrap_or_else(|| "Anonymous".to_string());
            let masked_username = Self::mask_username(&safe_username);

            display_rows.push(LeaderboardDisplayEntry {
                rank,
                username: masked_username,
                referral_count: row.referral_count,
                medal,
            });
        }

        Ok(display_rows)
    }

    pub async fn get_user_referrals(
        pool: &PgPool,
        referrer_id: i64,
    ) -> Result<Vec<caramba_db::models::store::DetailedReferral>> {
        // `total_earned` per referee is the money-model first-purchase payout
        // (referral_rewards), falling back to legacy referral_bonuses rows so
        // pre-migration earnings keep showing. Both are minor units (cents).
        sqlx::query_as::<_, caramba_db::models::store::DetailedReferral>(
            r#"
            SELECT
                u.id,
                u.tg_id,
                u.username,
                u.full_name,
                u.balance::BIGINT AS balance,
                u.referral_code,
                u.referrer_id,
                u.referred_by,
                u.is_banned,
                u.created_at,
                COALESCE(
                    (SELECT rr.amount_cents FROM referral_rewards rr
                     WHERE rr.referred_user_id = u.id AND rr.referrer_id = $1),
                    (SELECT CAST(SUM(rb.bonus_value) AS BIGINT) FROM referral_bonuses rb
                     WHERE rb.referred_user_id = u.id AND rb.user_id = $1),
                    0
                ) AS total_earned
            FROM users u
            WHERE COALESCE(u.referrer_id, u.referred_by) = $2
            ORDER BY u.created_at DESC
            "#
        )
        .bind(referrer_id)
        .bind(referrer_id)
        .fetch_all(pool)
        .await
        .context("Failed to fetch detailed referrals")
    }

    /// Lifetime balance credited to a referrer from referrals (minor units).
    /// Sums the money-model first-purchase payouts plus any legacy
    /// referral_bonuses rows so pre-migration earnings are not lost.
    pub async fn get_user_referral_earnings(pool: &PgPool, referrer_id: i64) -> Result<i64> {
        let rewards: i64 = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT CAST(SUM(amount_cents) AS BIGINT) FROM referral_rewards WHERE referrer_id = $1",
        )
        .bind(referrer_id)
        .fetch_one(pool)
        .await?
        .unwrap_or(0);

        let legacy: i64 = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT CAST(SUM(bonus_value) AS BIGINT) FROM referral_bonuses WHERE user_id = $1",
        )
        .bind(referrer_id)
        .fetch_optional(pool)
        .await?
        .flatten()
        .unwrap_or(0);

        Ok(rewards + legacy)
    }

    /// Effective referrer reward percent: per-user override
    /// (user_referral_rates.bonus_percent) -> global `referral_reward_percent`
    /// setting -> contract default 20.
    pub async fn reward_percent(
        executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        referrer_id: i64,
    ) -> Result<i64> {
        let custom: Option<i64> = sqlx::query_scalar::<_, i32>(
            "SELECT bonus_percent FROM user_referral_rates WHERE user_id = $1",
        )
        .bind(referrer_id)
        .fetch_optional(&mut **executor)
        .await?
        .map(|v| v as i64);

        if let Some(pct) = custom {
            return Ok(pct);
        }

        let global: i64 = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'referral_reward_percent'",
        )
        .fetch_optional(&mut **executor)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

        Ok(global)
    }

    /// Discount percent applied to a referee's FIRST paid purchase. Reads the
    /// global `referral_referee_discount_percent` setting, defaulting to the
    /// contract value 15. Returns 0 when the user has no referrer or has
    /// already made a paid purchase (so the discount applies once only).
    pub async fn referee_first_purchase_discount(pool: &PgPool, user_id: i64) -> Result<i64> {
        // Must have a referrer (new field or legacy column).
        let referrer: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT referrer_id, referred_by FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
        let has_referrer = referrer.and_then(|(r, rb)| r.or(rb)).is_some();
        if !has_referrer {
            return Ok(0);
        }

        // "First paid purchase only" must hold under concurrency and multiple open
        // checkouts. A completed purchase obviously consumes the discount, but so
        // does any still-open (pending) checkout: without this, a referee opening
        // two sessions before either completes would get the discount on BOTH, and
        // both could later be fulfilled. Unlike the referrer reward, the discount
        // has no UNIQUE ledger, so we treat an existing live ('pending') or
        // 'completed' session — and the reward ledger row — as a durable claim.
        // 'expired'/'failed' sessions are abandoned and do NOT consume the discount.
        let active_or_completed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_sessions \
             WHERE user_id = $1 AND status IN ('pending', 'completed')",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
        if active_or_completed > 0 {
            return Ok(0);
        }

        if Self::has_prior_paid_purchase(pool, user_id).await? {
            return Ok(0);
        }

        let pct: i64 = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'referral_referee_discount_percent'",
        )
        .fetch_optional(pool)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);

        Ok(pct.clamp(0, 100))
    }

    /// Whether the user already completed a paid purchase. Uses completed
    /// payment_sessions as the most complete signal (every plan/order/product/
    /// balance checkout flips its session to 'completed' on fulfillment), and
    /// also honours the first-purchase payout ledger as a belt-and-braces guard.
    pub async fn has_prior_paid_purchase(pool: &PgPool, user_id: i64) -> Result<bool> {
        let sessions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_sessions WHERE user_id = $1 AND status = 'completed'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
        if sessions > 0 {
            return Ok(true);
        }

        let rewarded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM referral_rewards WHERE referred_user_id = $1",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
        Ok(rewarded > 0)
    }

    /// Credits the referrer's balance on the referee's FIRST fulfilled paid
    /// purchase. Money model: referrer gets `reward_percent` of `amount_cents`
    /// as internal balance. Idempotent — the UNIQUE(referred_user_id) on
    /// referral_rewards ensures a given referee credits the referrer at most
    /// once; a duplicate attempt is a silent no-op. Runs inside the caller's
    /// transaction. Returns Some((referrer_tg_id, bonus_cents)) for the DM.
    pub async fn apply_first_purchase_reward(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        amount_cents: i64,
    ) -> Result<Option<(i64, i64)>> {
        if amount_cents <= 0 {
            return Ok(None);
        }

        let user = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
            "SELECT referrer_id, referred_by FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;

        let Some(referrer_id) = user.0.or(user.1) else {
            return Ok(None);
        };

        // Self-referral guard (apply_referral_bonus historically lacked this).
        if referrer_id == user_id {
            return Ok(None);
        }

        let reward_pct = Self::reward_percent(tx, referrer_id).await?;
        let bonus = amount_cents * reward_pct / 100;
        if bonus <= 0 {
            return Ok(None);
        }

        // Idempotency gate: claim this referee's payout. ON CONFLICT DO NOTHING
        // against UNIQUE(referred_user_id) makes the second fulfillment a no-op.
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO referral_rewards \
             (referrer_id, referred_user_id, base_amount_cents, amount_cents, reward_percent) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (referred_user_id) DO NOTHING RETURNING id",
        )
        .bind(referrer_id)
        .bind(user_id)
        .bind(amount_cents)
        .bind(bonus)
        .bind(reward_pct as i32)
        .fetch_optional(&mut **tx)
        .await?;

        if inserted.is_none() {
            // Already rewarded for this referee — do not credit again.
            return Ok(None);
        }

        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(bonus)
            .bind(referrer_id)
            .execute(&mut **tx)
            .await?;

        tracing::info!(
            referrer_id,
            referred_user_id = user_id,
            bonus_cents = bonus,
            reward_percent = reward_pct,
            "Applied first-purchase referral reward"
        );

        let referrer_tg_id: Option<i64> =
            sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                .bind(referrer_id)
                .fetch_optional(&mut **tx)
                .await?;

        Ok(referrer_tg_id.map(|tg| (tg, bonus)))
    }

    pub async fn get_referral_count(pool: &PgPool, user_id: i64) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM users WHERE COALESCE(referrer_id, referred_by) = $1",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
        Ok(count.0)
    }

    #[allow(dead_code)]
    pub async fn update_user_referral_code(
        pool: &PgPool,
        user_id: i64,
        new_code: &str,
    ) -> Result<()> {
        let clean_code = new_code.trim();
        if clean_code.is_empty() {
            return Err(anyhow::anyhow!("Referral code cannot be empty"));
        }

        sqlx::query("UPDATE users SET referral_code = $1 WHERE id = $2")
            .bind(clean_code)
            .bind(user_id)
            .execute(pool)
            .await
            .context("Failed to update referral code. It might already be taken.")?;

        Ok(())
    }

    /// DEPRECATED — old per-payment referral granting (percent of every balance
    /// top-up credited to the referrer, logged as referral_bonuses 'payment').
    ///
    /// The money model replaces this with a single first-purchase reward granted
    /// at fulfillment time (see `apply_first_purchase_reward`, hooked in
    /// MarketplaceService::fulfill_payment). This shim is intentionally a no-op
    /// so the legacy call site (PayService::process_balance_topup) no longer
    /// double-rewards on every top-up and no longer trips the dropped
    /// UNIQUE(user_id, referred_user_id) constraint on the second payment.
    /// Kept (not deleted) to avoid touching the bot/pay_service signature.
    pub async fn apply_referral_bonus(
        _pool: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _user_id: i64,
        _amount_cents: i64,
        _payment_id: Option<i64>,
    ) -> Result<Option<(i64, i64)>> {
        Ok(None)
    }

    /// Начисляет бонусы за регистрацию по реферальной ссылке.
    /// Вызывается при создании нового пользователя с referrer_id.
    /// Идемпотентен: повторный вызов с теми же аргументами пропускается.
    pub async fn apply_signup_bonus(
        pool: &sqlx::PgPool,
        referrer_id: i64,
        referred_user_id: i64,
    ) -> Result<(i64, i64)> {
        // Проверяем идемпотентность — не начисляем дважды
        let already: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM referral_bonuses WHERE user_id = $1 AND referred_user_id = $2 AND bonus_type = 'signup' LIMIT 1",
        )
        .bind(referrer_id)
        .bind(referred_user_id)
        .fetch_optional(pool)
        .await?;

        if already.is_some() {
            return Ok((0, 0));
        }

        // Загружаем индивидуальные настройки реферрера (приоритет над глобальными)
        let custom: Option<(Option<i32>, Option<i32>)> = sqlx::query_as(
            "SELECT referrer_signup_bonus_cents, referred_signup_bonus_cents FROM user_referral_rates WHERE user_id = $1",
        )
        .bind(referrer_id)
        .fetch_optional(pool)
        .await?;

        // Глобальные настройки — фолбэк
        let global_referrer: i64 = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'referral_referrer_signup_bonus_cents'",
        )
        .fetch_optional(pool)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

        let global_referred: i64 = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'referral_referred_signup_bonus_cents'",
        )
        .fetch_optional(pool)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

        let referrer_bonus: i64 = custom
            .as_ref()
            .and_then(|(r, _)| r.map(|v| v as i64))
            .unwrap_or(global_referrer);

        let referred_bonus: i64 = custom
            .as_ref()
            .and_then(|(_, r)| r.map(|v| v as i64))
            .unwrap_or(global_referred);

        // Если оба нулевые — ничего не делаем, не засоряем таблицу
        if referrer_bonus == 0 && referred_bonus == 0 {
            return Ok((0, 0));
        }

        let mut tx = pool.begin().await?;

        // Бонус рефереру
        if referrer_bonus > 0 {
            sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                .bind(referrer_bonus)
                .bind(referrer_id)
                .execute(&mut *tx)
                .await?;

            sqlx::query(
                "INSERT INTO referral_bonuses (user_id, referred_user_id, bonus_type, bonus_value, status, applied_at) \
                 VALUES ($1, $2, 'signup', $3, 'completed', CURRENT_TIMESTAMP)",
            )
            .bind(referrer_id)
            .bind(referred_user_id)
            .bind(referrer_bonus as f64)
            .execute(&mut *tx)
            .await?;
        }

        // Бонус новому пользователю
        if referred_bonus > 0 {
            sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                .bind(referred_bonus)
                .bind(referred_user_id)
                .execute(&mut *tx)
                .await?;

            // Записываем от имени referred_user_id (user_id = referred) с bonus_type = 'signup_welcome'
            // чтобы не конфликтовать с идемпотентностью referrer-записи
            sqlx::query(
                "INSERT INTO referral_bonuses (user_id, referred_user_id, bonus_type, bonus_value, status, applied_at) \
                 VALUES ($1, $2, 'signup_welcome', $3, 'completed', CURRENT_TIMESTAMP)",
            )
            .bind(referred_user_id)
            .bind(referred_user_id)
            .bind(referred_bonus as f64)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        tracing::info!(
            referrer_id,
            referred_user_id,
            referrer_bonus,
            referred_bonus,
            "Signup bonuses applied"
        );

        Ok((referrer_bonus, referred_bonus))
    }

    fn mask_username(username: &str) -> String {
        if username.len() <= 3 {
            return "***".to_string();
        }
        let len = username.len();
        let visible = if len > 6 { 3 } else { 1 };
        format!("{}***", &username[0..visible])
    }
}
