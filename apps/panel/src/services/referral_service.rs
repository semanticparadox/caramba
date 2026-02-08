use sqlx::SqlitePool;
use serde::Serialize;
use anyhow::Result;

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
    pub async fn get_leaderboard(pool: &SqlitePool, limit: i64) -> Result<Vec<LeaderboardDisplayEntry>> {
        // Query to count referrals per user
        let rows: Vec<LeaderboardEntry> = sqlx::query_as(r#"
            SELECT 
                u.username,
                COUNT(r.id) as referral_count
            FROM users u
            JOIN users r ON u.id = r.referred_by
            GROUP BY u.id
            ORDER BY referral_count DESC
            LIMIT ?
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

    pub async fn get_user_referrals(pool: &sqlx::SqlitePool, referrer_id: i64) -> Result<Vec<crate::models::store::DetailedReferral>> {
        sqlx::query_as::<_, crate::models::store::DetailedReferral>(
            r#"
            SELECT 
                u.id,
                u.tg_id,
                u.username,
                u.full_name,
                u.balance,
                u.referral_code,
                u.referrer_id,
                u.is_banned,
                u.created_at,
                COALESCE(CAST(SUM(rb.amount) AS INTEGER), 0) as total_earned
            FROM users u
            LEFT JOIN referral_bonuses rb ON u.id = rb.referred_id AND rb.referrer_id = ?
            WHERE u.referrer_id = ?
            GROUP BY u.id
            ORDER BY u.created_at DESC
            "#
        )
        .bind(referrer_id)
        .bind(referrer_id)
        .fetch_all(pool)
        .await
        .context("Failed to fetch detailed referrals")
    }

    pub async fn get_user_referral_earnings(pool: &sqlx::SqlitePool, referrer_id: i64) -> Result<i64> {
        let total: Option<i64> = sqlx::query_scalar("SELECT CAST(SUM(amount) AS INTEGER) FROM referral_bonuses WHERE referrer_id = ?")
            .bind(referrer_id)
            .fetch_one(pool)
            .await
            .ok()
            .flatten();
        Ok(total.unwrap_or(0))
    }

    pub async fn get_referral_count(pool: &sqlx::SqlitePool, user_id: i64) -> Result<i64> {
        let count = sqlx::query_scalar!("SELECT COUNT(*) FROM users WHERE referrer_id = ?", user_id)
            .fetch_one(pool)
            .await?;
        Ok(count as i64)
    }

    pub async fn update_user_referral_code(pool: &sqlx::SqlitePool, user_id: i64, new_code: &str) -> Result<()> {
        let clean_code = new_code.trim();
        if clean_code.is_empty() {
            return Err(anyhow::anyhow!("Referral code cannot be empty"));
        }

        sqlx::query("UPDATE users SET referral_code = ? WHERE id = ?")
            .bind(clean_code)
            .bind(user_id)
            .execute(pool)
            .await
            .context("Failed to update referral code. It might already be taken.")?;

        Ok(())
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
