use sqlx::PgPool;
use anyhow::{Context, Result};
use crate::models::store::User;

const USER_SELECT_SQL: &str = r#"
SELECT
    id,
    tg_id,
    username,
    full_name,
    balance::BIGINT AS balance,
    referral_code,
    referrer_id,
    referred_by,
    is_banned,
    language_code,
    terms_accepted_at,
    warning_count,
    trial_used,
    trial_used_at,
    last_bot_msg_id,
    created_at,
    parent_id
FROM users
"#;

#[derive(Debug, Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_all(&self) -> Result<Vec<User>> {
        sqlx::query_as::<_, User>(&format!("{USER_SELECT_SQL} ORDER BY created_at DESC"))
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all users")
    }

    pub async fn search(&self, query: &str) -> Result<Vec<User>> {
        let pattern = format!("%{}%", query);
        sqlx::query_as::<_, User>(&format!("{USER_SELECT_SQL} WHERE username ILIKE $1 OR full_name ILIKE $2 ORDER BY created_at DESC"))
            .bind(&pattern)
            .bind(&pattern)
            .fetch_all(&self.pool)
            .await
            .context("Failed to search users")
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<User>> {
        sqlx::query_as::<_, User>(&format!("{USER_SELECT_SQL} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by ID")
    }

    pub async fn get_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        sqlx::query_as::<_, User>(&format!("{USER_SELECT_SQL} WHERE tg_id = $1"))
            .bind(tg_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by TG ID")
    }

    pub async fn get_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        sqlx::query_as::<_, User>(&format!("{USER_SELECT_SQL} WHERE referral_code = $1"))
            .bind(code)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by referral code")
    }

    pub async fn upsert(&self, tg_id: i64, username: Option<&str>, full_name: Option<&str>, referrer_id: Option<i64>) -> Result<User> {
        let default_ref_code = tg_id.to_string();

        let primary = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (tg_id, username, full_name, referral_code, referrer_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT(tg_id) DO UPDATE SET
                username = COALESCE(excluded.username, users.username),
                full_name = COALESCE(excluded.full_name, users.full_name),
                referrer_id = COALESCE(users.referrer_id, excluded.referrer_id),
                last_seen = CURRENT_TIMESTAMP
            RETURNING id, tg_id, username, full_name, balance::BIGINT AS balance, referral_code, referrer_id, referred_by, is_banned, language_code, terms_accepted_at, warning_count, trial_used, trial_used_at, last_bot_msg_id, created_at, parent_id
            "#
        )
        .bind(tg_id)
        .bind(username)
        .bind(full_name)
        .bind(&default_ref_code)
        .bind(referrer_id)
        .fetch_one(&self.pool)
        .await;

        match primary {
            Ok(user) => Ok(user),
            Err(e) => {
                // Legacy fallback: very old schemas can miss users.last_seen.
                let msg = e.to_string();
                if msg.contains("last_seen") && msg.contains("does not exist") {
                    let user = sqlx::query_as::<_, User>(
                        r#"
                        INSERT INTO users (tg_id, username, full_name, referral_code, referrer_id)
                        VALUES ($1, $2, $3, $4, $5)
                        ON CONFLICT(tg_id) DO UPDATE SET
                            username = COALESCE(excluded.username, users.username),
                            full_name = COALESCE(excluded.full_name, users.full_name),
                            referrer_id = COALESCE(users.referrer_id, excluded.referrer_id)
                        RETURNING id, tg_id, username, full_name, balance::BIGINT AS balance, referral_code, referrer_id, referred_by, is_banned, language_code, terms_accepted_at, warning_count, trial_used, trial_used_at, last_bot_msg_id, created_at, parent_id
                        "#,
                    )
                    .bind(tg_id)
                    .bind(username)
                    .bind(full_name)
                    .bind(&default_ref_code)
                    .bind(referrer_id)
                    .fetch_one(&self.pool)
                    .await
                    .context("Failed to upsert user")?;
                    Ok(user)
                } else {
                    Err(e).context("Failed to upsert user")
                }
            }
        }
    }

    pub async fn update_balance(&self, id: i64, balance: i64) -> Result<()> {
        sqlx::query("UPDATE users SET balance = $1 WHERE id = $2")
            .bind(balance)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update user balance")?;
        Ok(())
    }

    pub async fn update_profile(&self, id: i64, balance: i64, is_banned: bool, referral_code: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE users SET balance = $1, is_banned = $2, referral_code = $3 WHERE id = $4")
            .bind(balance)
            .bind(is_banned)
            .bind(referral_code)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update user profile")?;
        Ok(())
    }

    pub async fn update_language(&self, id: i64, lang: &str) -> Result<()> {
        sqlx::query("UPDATE users SET language_code = $1 WHERE id = $2")
            .bind(lang)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update language")?;
        Ok(())
    }
    
    pub async fn update_warning_count(&self, id: i64, count: i32) -> Result<()> {
         sqlx::query("UPDATE users SET warning_count = $1 WHERE id = $2")
            .bind(count)
            .bind(id)
            .execute(&self.pool)
            .await?;
         Ok(())
    }
    
    pub async fn increment_warning_count(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET warning_count = warning_count + 1 WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn set_referrer_id(&self, user_id: i64, referrer_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET referrer_id = $1 WHERE id = $2")
            .bind(referrer_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        sqlx::query("UPDATE users SET last_bot_msg_id = $1 WHERE id = $2")
            .bind(msg_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn set_parent_id(&self, user_id: i64, parent_id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE users SET parent_id = $1 WHERE id = $2")
             .bind(parent_id)
             .bind(user_id)
             .execute(&self.pool)
             .await?;
        Ok(())
    }

    pub async fn update_terms_accepted(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET terms_accepted_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn adjust_balance(&self, user_id: i64, amount: i64) -> Result<()> {
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_trial_used(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET trial_used = 1, trial_used_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_by_parent_id(&self, parent_id: i64) -> Result<Vec<User>> {
        sqlx::query_as::<_, User>(&format!("{USER_SELECT_SQL} WHERE parent_id = $1"))
            .bind(parent_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch users by parent ID")
    }
}
