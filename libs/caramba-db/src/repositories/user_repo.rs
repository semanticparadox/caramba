use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::models::store::User;

#[derive(Debug, Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn row_to_user(row: &PgRow) -> User {
        User {
            id: row.try_get::<i64, _>("id").unwrap_or_default(),
            tg_id: row.try_get::<i64, _>("tg_id").unwrap_or_default(),
            username: row.try_get::<Option<String>, _>("username").ok().flatten(),
            full_name: row.try_get::<Option<String>, _>("full_name").ok().flatten(),
            balance: row
                .try_get::<i64, _>("balance")
                .or_else(|_| row.try_get::<i32, _>("balance").map(|v| v as i64))
                .unwrap_or_default(),
            referral_code: row
                .try_get::<Option<String>, _>("referral_code")
                .ok()
                .flatten(),
            referrer_id: row.try_get::<Option<i64>, _>("referrer_id").ok().flatten(),
            referred_by: row.try_get::<Option<i64>, _>("referred_by").ok().flatten(),
            is_banned: row.try_get::<bool, _>("is_banned").unwrap_or(false),
            language_code: row
                .try_get::<Option<String>, _>("language_code")
                .ok()
                .flatten(),
            terms_accepted_at: row
                .try_get::<Option<DateTime<Utc>>, _>("terms_accepted_at")
                .ok()
                .flatten(),
            warning_count: row
                .try_get::<i32, _>("warning_count")
                .or_else(|_| row.try_get::<i16, _>("warning_count").map(i32::from))
                .unwrap_or_default(),
            trial_used: row
                .try_get::<Option<bool>, _>("trial_used")
                .ok()
                .flatten()
                .or_else(|| {
                    row.try_get::<Option<i16>, _>("trial_used")
                        .ok()
                        .flatten()
                        .map(|v| v != 0)
                })
                .or_else(|| {
                    row.try_get::<Option<i32>, _>("trial_used")
                        .ok()
                        .flatten()
                        .map(|v| v != 0)
                }),
            trial_used_at: row
                .try_get::<Option<DateTime<Utc>>, _>("trial_used_at")
                .ok()
                .flatten(),
            last_bot_msg_id: row
                .try_get::<Option<i32>, _>("last_bot_msg_id")
                .ok()
                .flatten()
                .or_else(|| {
                    row.try_get::<Option<i64>, _>("last_bot_msg_id")
                        .ok()
                        .flatten()
                        .map(|v| v as i32)
                }),
            created_at: row
                .try_get::<DateTime<Utc>, _>("created_at")
                .unwrap_or_else(|_| Utc::now()),
            parent_id: row.try_get::<Option<i64>, _>("parent_id").ok().flatten(),
        }
    }

    pub async fn get_all(&self) -> Result<Vec<User>> {
        let rows = sqlx::query("SELECT * FROM users ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all users")?;
        Ok(rows.into_iter().map(|r| Self::row_to_user(&r)).collect())
    }

    pub async fn search(&self, query: &str) -> Result<Vec<User>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query(
            "SELECT * FROM users WHERE username ILIKE $1 OR full_name ILIKE $2 ORDER BY created_at DESC",
        )
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .context("Failed to search users")?;
        Ok(rows.into_iter().map(|r| Self::row_to_user(&r)).collect())
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<User>> {
        let row = sqlx::query("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by ID")?;
        Ok(row.map(|r| Self::row_to_user(&r)))
    }

    pub async fn get_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        let row = sqlx::query("SELECT * FROM users WHERE tg_id = $1")
            .bind(tg_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by TG ID")?;
        Ok(row.map(|r| Self::row_to_user(&r)))
    }

    pub async fn get_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        let row = sqlx::query("SELECT * FROM users WHERE referral_code = $1")
            .bind(code)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch user by referral code")?;
        Ok(row.map(|r| Self::row_to_user(&r)))
    }

    pub async fn upsert(
        &self,
        tg_id: i64,
        username: Option<&str>,
        full_name: Option<&str>,
        referrer_id: Option<i64>,
    ) -> Result<User> {
        let default_ref_code = tg_id.to_string();

        // Current schema path.
        let user_id = match sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO users (tg_id, username, full_name, referral_code, referrer_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT(tg_id) DO UPDATE SET
                username = COALESCE(excluded.username, users.username),
                full_name = COALESCE(excluded.full_name, users.full_name),
                referrer_id = COALESCE(users.referrer_id, excluded.referrer_id),
                last_seen = CURRENT_TIMESTAMP
            RETURNING id
            "#,
        )
        .bind(tg_id)
        .bind(username)
        .bind(full_name)
        .bind(&default_ref_code)
        .bind(referrer_id)
        .fetch_one(&self.pool)
        .await
        {
            Ok(id) => id,
            Err(first_err) => {
                // Legacy path: no users.last_seen.
                match sqlx::query_scalar::<_, i64>(
                    r#"
                    INSERT INTO users (tg_id, username, full_name, referral_code, referrer_id)
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT(tg_id) DO UPDATE SET
                        username = COALESCE(excluded.username, users.username),
                        full_name = COALESCE(excluded.full_name, users.full_name),
                        referrer_id = COALESCE(users.referrer_id, excluded.referrer_id)
                    RETURNING id
                    "#,
                )
                .bind(tg_id)
                .bind(username)
                .bind(full_name)
                .bind(&default_ref_code)
                .bind(referrer_id)
                .fetch_one(&self.pool)
                .await
                {
                    Ok(id) => id,
                    Err(second_err) => {
                        // Very old path: no users.referrer_id.
                        match sqlx::query_scalar::<_, i64>(
                            r#"
                            INSERT INTO users (tg_id, username, full_name, referral_code)
                            VALUES ($1, $2, $3, $4)
                            ON CONFLICT(tg_id) DO UPDATE SET
                                username = COALESCE(excluded.username, users.username),
                                full_name = COALESCE(excluded.full_name, users.full_name)
                            RETURNING id
                            "#,
                        )
                        .bind(tg_id)
                        .bind(username)
                        .bind(full_name)
                        .bind(&default_ref_code)
                        .fetch_one(&self.pool)
                        .await
                        {
                            Ok(id) => id,
                            Err(third_err) => {
                                return Err(anyhow::anyhow!(
                                    "Failed to upsert user across compatibility paths: primary={}, fallback={}, legacy={}",
                                    first_err,
                                    second_err,
                                    third_err
                                ));
                            }
                        }
                    }
                }
            }
        };

        self.get_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User {} not found after upsert", user_id))
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

    pub async fn update_profile(
        &self,
        id: i64,
        balance: i64,
        is_banned: bool,
        referral_code: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE users SET balance = $1, is_banned = $2, referral_code = $3 WHERE id = $4",
        )
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
        sqlx::query(
            "UPDATE users SET trial_used = 1, trial_used_at = CURRENT_TIMESTAMP WHERE id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_parent_id(&self, parent_id: i64) -> Result<Vec<User>> {
        let rows = sqlx::query("SELECT * FROM users WHERE parent_id = $1")
            .bind(parent_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch users by parent ID")?;
        Ok(rows.into_iter().map(|r| Self::row_to_user(&r)).collect())
    }
}
