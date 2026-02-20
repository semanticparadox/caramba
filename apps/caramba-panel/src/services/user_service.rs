use anyhow::{Context, Result};
use caramba_db::models::store::User;
use caramba_db::repositories::user_repo::UserRepository;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct UserService {
    pool: PgPool,
    user_repo: UserRepository,
}

impl UserService {
    pub fn new(pool: PgPool) -> Self {
        let user_repo = UserRepository::new(pool.clone());
        Self { pool, user_repo }
    }

    pub async fn get_all(&self) -> Result<Vec<User>> {
        self.user_repo.get_all().await
    }

    pub async fn search(&self, query: &str) -> Result<Vec<User>> {
        self.user_repo.search(query).await
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<User>> {
        self.user_repo.get_by_id(id).await
    }

    pub async fn update_profile(
        &self,
        id: i64,
        balance: i64,
        is_banned: bool,
        referral_code: Option<&str>,
    ) -> Result<()> {
        self.user_repo
            .update_profile(id, balance, is_banned, referral_code)
            .await
    }

    pub async fn set_balance(&self, id: i64, balance: i64) -> Result<()> {
        self.user_repo.update_balance(id, balance).await
    }

    pub async fn get_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        self.user_repo.get_by_tg_id(tg_id).await
    }

    pub async fn get_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        self.user_repo.get_by_referral_code(code).await
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

    pub async fn upsert(
        &self,
        tg_id: i64,
        username: Option<&str>,
        full_name: Option<&str>,
        referrer_id: Option<i64>,
    ) -> Result<User> {
        let existing = self.user_repo.get_by_tg_id(tg_id).await?;

        let user = self
            .user_repo
            .upsert(tg_id, username, full_name, referrer_id)
            .await?;

        if existing.is_none() {
            let _ =
                crate::services::analytics_service::AnalyticsService::track_new_user(&self.pool)
                    .await;
        }
        let _ = crate::services::analytics_service::AnalyticsService::track_active_user(
            &self.pool, user.id,
        )
        .await;

        Ok(user)
    }

    pub async fn increment_warning_count(&self, user_id: i64) -> Result<()> {
        self.user_repo.increment_warning_count(user_id).await?;
        Ok(())
    }

    pub async fn ban(&self, user_id: i64) -> Result<()> {
        // We reuse update_profile but we need current profile info...
        // Ah, this is where Repo shines. Maybe Repo should have specific `ban` method?
        // Let's use get_by_id first
        if let Some(user) = self.user_repo.get_by_id(user_id).await? {
            self.user_repo
                .update_profile(user_id, user.balance, true, user.referral_code.as_deref())
                .await?;
        }
        Ok(())
    }

    pub async fn update_language(&self, user_id: i64, lang: &str) -> Result<()> {
        self.user_repo.update_language(user_id, lang).await
    }

    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        self.user_repo.update_last_bot_msg_id(user_id, msg_id).await
    }

    pub async fn add_bot_message_to_history(
        &self,
        user_id: i64,
        chat_id: i64,
        message_id: i32,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO bot_chat_history (user_id, chat_id, message_id) VALUES ($1, $2, $3)",
        )
        .bind(user_id)
        .bind(chat_id)
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cleanup_bot_history(
        &self,
        user_id: i64,
        keep_count: i64,
    ) -> Result<Vec<(i64, i32)>> {
        let ids_to_delete: Vec<(i64, i64, i32)> = sqlx::query_as(
            "SELECT id, chat_id, message_id FROM bot_chat_history 
             WHERE user_id = $1 
             ORDER BY created_at DESC 
             OFFSET $2",
        )
        .bind(user_id)
        .bind(keep_count)
        .fetch_all(&self.pool)
        .await?;

        if ids_to_delete.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = ids_to_delete.iter().map(|(id, _, _)| *id).collect();
        sqlx::query("DELETE FROM bot_chat_history WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&self.pool)
            .await?;

        Ok(ids_to_delete
            .into_iter()
            .map(|(_, chat_id, msg_id)| (chat_id, msg_id))
            .collect())
    }

    pub async fn update_terms(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_terms_accepted(user_id).await
    }

    pub async fn set_referrer(&self, user_id: i64, referrer_code: &str) -> Result<()> {
        let resolved_referrer_id = self
            .resolve_referrer_id(referrer_code.trim())
            .await?
            .context("Referrer not found")?;

        if resolved_referrer_id == user_id {
            return Err(anyhow::anyhow!("You cannot refer yourself"));
        }

        let user = self
            .user_repo
            .get_by_id(user_id)
            .await?
            .context("User not found")?;

        if user.referrer_id.is_some() || user.referred_by.is_some() {
            return Err(anyhow::anyhow!(
                "Referrer is already set and cannot be changed"
            ));
        }

        sqlx::query(
            "UPDATE users SET referrer_id = $1, referred_by = COALESCE(referred_by, $1) WHERE id = $2",
        )
            .bind(resolved_referrer_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
