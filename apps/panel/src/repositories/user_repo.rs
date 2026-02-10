use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::User;

#[derive(Clone)]
pub struct UserRepository {
    pool: SqlitePool,
}

impl UserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_all(&self) -> Result<Vec<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all users")
    }

    pub async fn search(&self, query: &str) -> Result<Vec<User>> {
        let pattern = format!("%{}%", query);
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username LIKE ? OR full_name LIKE ? ORDER BY created_at DESC")
            .bind(&pattern)
            .bind(&pattern)
            .fetch_all(&self.pool)
            .await
            .context("Failed to search users")
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by ID")
    }

    pub async fn get_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE tg_id = ?")
            .bind(tg_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by TG ID")
    }

    pub async fn get_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE referral_code = ?")
            .bind(code)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by referral code")
    }

    pub async fn upsert(&self, tg_id: i64, username: Option<&str>, full_name: Option<&str>, referrer_id: Option<i64>) -> Result<User> {
        let default_ref_code = tg_id.to_string();
        
        // We do a read-first to preserve existing referrer_id if not provided,
        // although the ON CONFLICT clause in SQL can handle COALESCE logics too.
        // But doing it like store_service allows us to be precise.
        // However, purely for Repo, we should probably trust the SQL.
        
        // Let's stick to the SQL logic from store_service which seems robust enough
        // "referrer_id = COALESCE(users.referrer_id, excluded.referrer_id)"
        
        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (tg_id, username, full_name, referral_code, referrer_id)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(tg_id) DO UPDATE SET
                username = COALESCE(excluded.username, users.username),
                full_name = COALESCE(excluded.full_name, users.full_name),
                referrer_id = COALESCE(users.referrer_id, excluded.referrer_id),
                last_seen = CURRENT_TIMESTAMP
            RETURNING id, tg_id, username, full_name, balance, referral_code, referrer_id, referred_by, is_banned, language_code, terms_accepted_at, warning_count, trial_used, trial_used_at, last_bot_msg_id, created_at, parent_id
            "#
        )
        .bind(tg_id)
        .bind(username)
        .bind(full_name)
        .bind(default_ref_code)
        .bind(referrer_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to upsert user")?;

        Ok(user)
    }

    pub async fn update_balance(&self, id: i64, balance: i64) -> Result<()> {
        sqlx::query("UPDATE users SET balance = ? WHERE id = ?")
            .bind(balance)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update user balance")?;
        Ok(())
    }

    pub async fn update_profile(&self, id: i64, balance: i64, is_banned: bool, referral_code: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE users SET balance = ?, is_banned = ?, referral_code = ? WHERE id = ?")
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
        sqlx::query("UPDATE users SET language_code = ? WHERE id = ?")
            .bind(lang)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update language")?;
        Ok(())
    }
    
    pub async fn update_warning_count(&self, id: i64, count: i32) -> Result<()> {
         // Using increment could be a business logic method, raw update is safer for Repo
         // But "increment" is a very common atomic database op.
         // Let's keep it generic "set" or specific "increment"?
         // Let's implement set for now.
         sqlx::query("UPDATE users SET warning_count = ? WHERE id = ?")
            .bind(count)
            .bind(id)
            .execute(&self.pool)
            .await?;
         Ok(())
    }
    
    pub async fn increment_warning_count(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET warning_count = warning_count + 1 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn set_referrer_id(&self, user_id: i64, referrer_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET referrer_id = ? WHERE id = ?")
            .bind(referrer_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        sqlx::query("UPDATE users SET last_bot_msg_id = ? WHERE id = ?")
            .bind(msg_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn set_parent_id(&self, user_id: i64, parent_id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE users SET parent_id = ? WHERE id = ?")
             .bind(parent_id)
             .bind(user_id)
             .execute(&self.pool)
             .await?;
        Ok(())
    }

    pub async fn update_terms_accepted(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET terms_accepted_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn adjust_balance(&self, user_id: i64, amount: i64) -> Result<()> {
        // Amount can be positive (credit) or negative (debit)
        sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
            .bind(amount)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_trial_used(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET trial_used = 1, trial_used_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
