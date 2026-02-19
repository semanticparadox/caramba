use sqlx::PgPool;
use serde::Serialize;
use anyhow::{Result, Context};

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
    pub async fn get_leaderboard(pool: &PgPool, limit: i64) -> Result<Vec<LeaderboardDisplayEntry>> {
        // Query to count referrals per user
        let rows: Vec<LeaderboardEntry> = sqlx::query_as(r#"
            SELECT 
                u.username,
                COUNT(r.id) as referral_count
            FROM users u
            JOIN users r ON u.id = r.referrer_id
            GROUP BY u.id, u.username
            ORDER BY referral_count DESC
            LIMIT $1
        "#)
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

    pub async fn get_user_referrals(pool: &PgPool, referrer_id: i64) -> Result<Vec<caramba_db::models::store::DetailedReferral>> {
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
                COALESCE(CAST(SUM(rb.bonus_value) AS BIGINT), 0) as total_earned
            FROM users u
            LEFT JOIN referral_bonuses rb ON u.id = rb.referred_user_id AND rb.user_id = $1
            WHERE u.referrer_id = $2
            GROUP BY u.id, u.tg_id, u.username, u.full_name, u.balance, u.referral_code, u.referrer_id, u.referred_by, u.is_banned, u.created_at
            ORDER BY u.created_at DESC
            "#
        )
        .bind(referrer_id)
        .bind(referrer_id)
        .fetch_all(pool)
        .await
        .context("Failed to fetch detailed referrals")
    }

    pub async fn get_user_referral_earnings(pool: &PgPool, referrer_id: i64) -> Result<i64> {
        let total: Option<i64> = sqlx::query_scalar("SELECT CAST(SUM(bonus_value) AS BIGINT) FROM referral_bonuses WHERE user_id = $1")
            .bind(referrer_id)
            .fetch_optional(pool)
            .await?;
        Ok(total.unwrap_or(0))
    }

    pub async fn get_referral_count(pool: &PgPool, user_id: i64) -> Result<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE referrer_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
        Ok(count.0)
    }

    pub async fn update_user_referral_code(pool: &PgPool, user_id: i64, new_code: &str) -> Result<()> {
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


    pub async fn apply_referral_bonus(pool: &mut sqlx::Transaction<'_, sqlx::Postgres>, user_id: i64, amount_cents: i64, _payment_id: Option<i64>) -> Result<Option<(i64, i64)>> {
        // 10% bonus for the referrer
        let user = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
            "SELECT referrer_id, referred_by FROM users WHERE id = $1",
        )
            .bind(user_id)
            .fetch_one(&mut **pool)
            .await?;

        let referrer_id = user.0.or(user.1);

        if let Some(r_id) = referrer_id {
            let bonus = amount_cents / 10;
            if bonus > 0 {
                // 1. Update balance
                sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                    .bind(bonus)
                    .bind(r_id)
                    .execute(&mut **pool)
                    .await?;
                
                // 2. Log to referral_bonuses
                sqlx::query("INSERT INTO referral_bonuses (user_id, referred_user_id, bonus_type, bonus_value, status, applied_at) VALUES ($1, $2, 'payment', $3, 'completed', CURRENT_TIMESTAMP)")
                    .bind(r_id)
                    .bind(user_id)
                    .bind(bonus as f64)
                    .execute(&mut **pool)
                    .await?;

                tracing::info!("Applied referral bonus of {} to user {} (from user {})", bonus, r_id, user_id);

                // Fetch referrer tg_id for notification
                let referrer_tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                    .bind(r_id)
                    .fetch_optional(&mut **pool)
                    .await?;
                
                if let Some(tg_id) = referrer_tg_id {
                    return Ok(Some((tg_id, bonus)));
                }
            }
        }
        Ok(None)
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
