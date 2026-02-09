use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::User;


#[derive(Debug, Clone)]
pub struct UserService {
    pool: SqlitePool,
}

impl UserService {
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
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username LIKE ? OR full_name LIKE ? ORDER BY created_at DESC")
            .bind(format!("%{}%", query))
            .bind(format!("%{}%", query))
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

    pub async fn update_profile(&self, id: i64, balance: i64, is_banned: bool, referral_code: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE users SET balance = ?, is_banned = ?, referral_code = ? WHERE id = ?")
            .bind(balance)
            .bind(is_banned)
            .bind(referral_code)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_balance(&self, id: i64, balance: i64) -> Result<()> {
        sqlx::query("UPDATE users SET balance = ? WHERE id = ?")
            .bind(balance)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
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

    pub async fn resolve_referrer_id(&self, code: &str) -> Result<Option<i64>> {
        if let Ok(tg_id) = code.parse::<i64>() {
            if let Some(user) = self.get_by_tg_id(tg_id).await? {
                return Ok(Some(user.id));
            }
        }
        
        if let Some(user) = self.get_by_referral_code(code).await? {
            return Ok(Some(user.id));
        }
        
        Ok(None)
    }

    pub async fn upsert(&self, tg_id: i64, username: Option<&str>, full_name: Option<&str>, referrer_id: Option<i64>) -> Result<User> {
        let existing = self.get_by_tg_id(tg_id).await?;
        
        let final_referrer_id = if let Some(ref u) = existing {
            u.referrer_id
        } else {
            referrer_id
        };

        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (tg_id, username, full_name, referral_code, referrer_id)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(tg_id) DO UPDATE SET
                username = COALESCE(excluded.username, users.username),
                full_name = COALESCE(excluded.full_name, users.full_name),
                referrer_id = COALESCE(users.referrer_id, excluded.referrer_id),
                last_seen = CURRENT_TIMESTAMP
            RETURNING id, tg_id, username, full_name, balance, referral_code, referrer_id, referred_by, is_banned, language_code, terms_accepted_at, warning_count, trial_used, trial_used_at, last_bot_msg_id, created_at
            "#
        )
        .bind(tg_id)
        .bind(username)
        .bind(full_name)
        .bind(tg_id.to_string())
        .bind(final_referrer_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to upsert user")?;

        if existing.is_none() {
             let _ = crate::services::analytics_service::AnalyticsService::track_new_user(&self.pool).await;
        }
        let _ = crate::services::analytics_service::AnalyticsService::track_active_user(&self.pool, user.id).await;

        Ok(user)
    }

    pub async fn increment_warning_count(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET warning_count = warning_count + 1 WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn ban(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET is_banned = 1 WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_language(&self, user_id: i64, lang: &str) -> Result<()> {
        sqlx::query("UPDATE users SET language_code = ? WHERE id = ?")
            .bind(lang)
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

    pub async fn add_bot_message_to_history(&self, user_id: i64, chat_id: i64, message_id: i32) -> Result<()> {
        sqlx::query("INSERT INTO bot_chat_history (user_id, chat_id, message_id) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(chat_id)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn cleanup_bot_history(&self, user_id: i64, keep_count: i64) -> Result<Vec<(i64, i32)>> {
        let ids_to_delete: Vec<(i64, i64, i32)> = sqlx::query_as(
            "SELECT id, chat_id, message_id FROM bot_chat_history 
             WHERE user_id = ? 
             ORDER BY created_at DESC 
             LIMIT -1 OFFSET ?"
        )
        .bind(user_id)
        .bind(keep_count)
        .fetch_all(&self.pool)
        .await?;

        if ids_to_delete.is_empty() {
            return Ok(Vec::new());
        }

        let id_list = ids_to_delete.iter().map(|(id, _, _)| id.to_string()).collect::<Vec<_>>().join(",");
        let query = format!("DELETE FROM bot_chat_history WHERE id IN ({})", id_list);
        sqlx::query(&query).execute(&self.pool).await?;

        Ok(ids_to_delete.into_iter().map(|(_, chat_id, msg_id)| (chat_id, msg_id)).collect())
    }

    pub async fn update_terms(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET terms_accepted_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_referrer(&self, user_id: i64, referrer_code: &str) -> Result<()> {
        let referrer = self.get_by_referral_code(referrer_code.trim()).await?
            .context("Referrer not found")?;

        if referrer.id == user_id {
            return Err(anyhow::anyhow!("You cannot refer yourself"));
        }

        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;

        if user.referrer_id.is_some() {
            return Err(anyhow::anyhow!("Referrer is already set and cannot be changed"));
        }

        sqlx::query("UPDATE users SET referrer_id = ? WHERE id = ?")
            .bind(referrer.id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
