use crate::services::activity_service::ActivityService;
use crate::services::referral_service::ReferralService;
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use caramba_db::models::store::{CartItem, GiftCode, PlanDuration, Subscription, User};

use caramba_db::repositories::api_key_repo::ApiKeyRepository;
use caramba_db::repositories::node_repo::NodeRepository;
use caramba_db::repositories::subscription_repo::SubscriptionRepository;
use caramba_db::repositories::user_repo::UserRepository;

#[derive(Debug, Clone)]
pub struct StoreService {
    pool: PgPool,
    user_repo: UserRepository,
    pub sub_repo: SubscriptionRepository,
    pub node_repo: NodeRepository,
    pub api_key_repo: ApiKeyRepository,
}

impl StoreService {
    pub fn new(pool: PgPool) -> Self {
        let user_repo = UserRepository::new(pool.clone());
        let sub_repo = SubscriptionRepository::new(pool.clone());
        let node_repo = NodeRepository::new(pool.clone());
        let api_key_repo = ApiKeyRepository::new(pool.clone());
        Self {
            pool,
            user_repo,
            sub_repo,
            node_repo,
            api_key_repo,
        }
    }

    pub fn get_pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub async fn get_products_by_category(
        &self,
        category_id: i64,
    ) -> Result<Vec<caramba_db::models::store::Product>> {
        sqlx::query_as::<_, caramba_db::models::store::Product>(
            "SELECT id, category_id, name, description, price, product_type, content, is_active, created_at FROM products WHERE category_id = $1 AND is_active = TRUE"
        )
        .bind(category_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch products")
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<caramba_db::models::node::Node>> {
        self.node_repo.get_active_nodes().await
    }

    pub async fn get_api_keys(&self) -> Result<Vec<caramba_db::models::api_key::ApiKey>> {
        self.api_key_repo.get_all().await
    }

    pub async fn create_api_key(
        &self,
        name: &str,
        key: &str,
        max_uses: Option<i64>,
    ) -> Result<caramba_db::models::api_key::ApiKey> {
        self.api_key_repo.create(name, key, max_uses).await
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<()> {
        self.api_key_repo.delete(id).await
    }

    pub async fn get_active_subs_by_plans(
        &self,
        plan_ids: &[i64],
    ) -> Result<Vec<(i64, Option<String>, i64, Option<String>)>> {
        self.sub_repo.get_active_subs_by_plans(plan_ids).await
    }

    pub async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Option<Subscription>> {
        self.sub_repo.get_by_uuid(uuid).await
    }

    pub async fn update_subscription_status(&self, sub_id: i64, status: &str) -> Result<()> {
        self.sub_repo.update_status(sub_id, status).await
    }

    pub async fn reset_warning_count(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_warning_count(user_id, 0).await
    }

    pub async fn get_user_nodes(
        &self,
        user_id: i64,
    ) -> Result<Vec<caramba_db::models::node::Node>> {
        let plan_id = self.sub_repo.get_active_plan_id_by_user(user_id).await?;
        match plan_id {
            Some(id) => self.node_repo.get_nodes_for_plan(id).await,
            None => Ok(vec![]),
        }
    }

    pub async fn get_user_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        self.user_repo.get_by_tg_id(tg_id).await
    }

    pub async fn get_user_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        self.user_repo.get_by_referral_code(code).await
    }

    pub async fn resolve_referrer_id(&self, code: &str) -> Result<Option<i64>> {
        if let Ok(tg_id) = code.parse::<i64>() {
            if let Some(user) = self.get_user_by_tg_id(tg_id).await? {
                return Ok(Some(user.id));
            }
        }

        if let Some(user) = self.get_user_by_referral_code(code).await? {
            return Ok(Some(user.id));
        }

        Ok(None)
    }

    pub async fn upsert_user(
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

    pub async fn create_family_invite(
        &self,
        parent_id: i64,
        max_uses: i32,
        duration_days: i32,
    ) -> Result<caramba_db::models::store::FamilyInvite> {
        let random_part = Uuid::new_v4()
            .to_string()
            .replace("-", "")
            .chars()
            .take(6)
            .collect::<String>()
            .to_uppercase();
        let code = format!("FAMILY-{}", random_part);
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        let invite = sqlx::query_as::<_, caramba_db::models::store::FamilyInvite>(
            "INSERT INTO family_invites (code, parent_id, max_uses, expires_at) VALUES ($1, $2, $3, $4) RETURNING *"
        )
        .bind(code)
        .bind(parent_id)
        .bind(max_uses)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create family invite")?;

        Ok(invite)
    }

    pub async fn get_valid_invite(
        &self,
        code: &str,
    ) -> Result<Option<caramba_db::models::store::FamilyInvite>> {
        let invite = sqlx::query_as::<_, caramba_db::models::store::FamilyInvite>(
            "SELECT * FROM family_invites WHERE code = $1 AND expires_at > CURRENT_TIMESTAMP AND used_count < max_uses"
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;

        Ok(invite)
    }

    pub async fn redeem_family_invite(&self, user_id: i64, code: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let invite = sqlx::query_as::<_, caramba_db::models::store::FamilyInvite>(
            "SELECT * FROM family_invites WHERE code = $1 AND expires_at > CURRENT_TIMESTAMP AND used_count < max_uses FOR UPDATE"
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let invite = match invite {
            Some(i) => i,
            None => return Err(anyhow::anyhow!("Invalid or expired invite code")),
        };

        if invite.parent_id == user_id {
            return Err(anyhow::anyhow!("You cannot invite yourself"));
        }

        let current_user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        if let Some(pid) = current_user.parent_id {
            if pid == invite.parent_id {
                return Err(anyhow::anyhow!("You are already in this family"));
            }
            return Err(anyhow::anyhow!(
                "You are already a member of another family"
            ));
        }

        sqlx::query("UPDATE users SET parent_id = $1 WHERE id = $2")
            .bind(invite.parent_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE family_invites SET used_count = used_count + 1 WHERE id = $1")
            .bind(invite.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        self.sync_family_subscriptions(invite.parent_id).await?;
        Ok(())
    }

    pub async fn get_family_members(&self, parent_id: i64) -> Result<Vec<User>> {
        self.user_repo.get_by_parent_id(parent_id).await
    }

    pub async fn set_user_parent(&self, user_id: i64, parent_id: Option<i64>) -> Result<()> {
        self.user_repo.set_parent_id(user_id, parent_id).await?;
        if let Some(pid) = parent_id {
            self.sync_family_subscriptions(pid).await?;
        }
        Ok(())
    }

    pub async fn sync_family_subscriptions(&self, parent_id: i64) -> Result<()> {
        let parent_sub = self.sub_repo.get_active_by_user(parent_id).await?;
        let children = self.get_family_members(parent_id).await?;
        if children.is_empty() {
            return Ok(());
        }

        let tx = self.pool.begin().await?;

        if let Some(psub) = parent_sub {
            for child in children {
                let child_sub = self.sub_repo.get_active_by_user(child.id).await?;

                if let Some(csub) = child_sub {
                    if csub.note.as_deref() == Some("Family") || csub.plan_id == psub.plan_id {
                        self.sub_repo
                            .update_family_sub(csub.id, psub.expires_at, psub.plan_id, psub.node_id)
                            .await?;
                    }
                } else {
                    let vless_uuid = Uuid::new_v4().to_string();
                    let sub_uuid = Uuid::new_v4().to_string();
                    self.sub_repo
                        .create(
                            child.id,
                            psub.plan_id,
                            &vless_uuid,
                            &sub_uuid,
                            psub.expires_at,
                            "active",
                            Some("Family"),
                            false,
                        )
                        .await?;
                }
            }
        } else {
            for child in children {
                self.sub_repo.expire_family_subs(child.id).await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn increment_warning_count(&self, user_id: i64) -> Result<()> {
        self.user_repo.increment_warning_count(user_id).await?;
        Ok(())
    }

    pub async fn ban_user(&self, user_id: i64) -> Result<()> {
        let user = self
            .user_repo
            .get_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;
        self.user_repo
            .update_profile(user_id, user.balance, true, user.referral_code.as_deref())
            .await?;
        Ok(())
    }

    pub async fn update_user_language(&self, user_id: i64, lang: &str) -> Result<()> {
        self.user_repo.update_language(user_id, lang).await?;
        Ok(())
    }

    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        self.user_repo
            .update_last_bot_msg_id(user_id, msg_id)
            .await?;
        Ok(())
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

    pub async fn update_user_terms(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_terms_accepted(user_id).await?;
        Ok(())
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let res = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(res)
    }

    pub async fn update_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn purchase_plan(&self, user_id: i64, duration_id: i64) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        let duration = sqlx::query_as::<_, caramba_db::models::store::PlanDuration>(
            "SELECT * FROM plan_durations WHERE id = $1",
        )
        .bind(duration_id)
        .fetch_one(&mut *tx)
        .await?;

        if user.balance < duration.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(duration.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
        let vless_uuid = Uuid::new_v4().to_string();
        let sub_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(user_id)
        .bind(duration.plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        let _ = crate::services::analytics_service::AnalyticsService::track_order(&self.pool).await;
        let _ = ActivityService::log_tx(
            &self.pool,
            Some(user_id),
            "Plan Purchase",
            &format!("Purchased plan (Duration ID: {})", duration_id),
        )
        .await;
        Ok(sub)
    }

    pub async fn purchase_product_with_balance(
        &self,
        user_id: i64,
        product_id: i64,
    ) -> Result<caramba_db::models::store::Product> {
        let mut tx = self.pool.begin().await?;
        let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        let product: caramba_db::models::store::Product =
            sqlx::query_as("SELECT * FROM products WHERE id = $1")
                .bind(product_id)
                .fetch_one(&mut *tx)
                .await?;

        if user.balance < product.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(product.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Product Purchase",
            &format!("Purchased product: {}", product.name),
        )
        .await;

        tx.commit().await?;
        Ok(product)
    }

    pub async fn activate_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let _tx = self.pool.begin().await?;

        let sub = self.get_subscription(sub_id, user_id).await?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Subscription is not pending"));
        }

        let duration = sub.expires_at - sub.created_at;
        let new_expires_at = Utc::now() + duration;

        self.sub_repo
            .update_status_and_expiry(sub_id, "active", new_expires_at)
            .await?;
        let updated_sub = self.sub_repo.get_by_id(sub_id).await?.unwrap();

        let _ = ActivityService::log(
            &self.pool,
            "Subscription",
            &format!("User {} activated sub {}", user_id, sub_id),
        )
        .await;

        Ok(updated_sub)
    }

    pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let sub = self
            .sub_repo
            .get_by_id(sub_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        if sub.user_id != user_id {
            return Err(anyhow::anyhow!("Unauthorized access to subscription"));
        }

        Ok(sub)
    }

    pub async fn convert_subscription_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(sub_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!(
                "Only pending subscriptions can be converted to gifts"
            ));
        }

        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        let code = format!(
            "CARAMBA-GIFT-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("CODE")
                .to_uppercase()
        );

        sqlx::query(
            "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES ($1, $2, $3, $4)"
        )
        .bind(&code)
        .bind(sub.plan_id)
        .bind(duration_days)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(code)
    }

    pub async fn redeem_gift_code(&self, user_id: i64, code: &str) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let gift_code_opt = sqlx::query_as::<_, caramba_db::models::store::GiftCode>(
            "SELECT * FROM gift_codes
             WHERE code = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
             FOR UPDATE",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let gift_code =
            gift_code_opt.ok_or_else(|| anyhow::anyhow!("Invalid or already redeemed code"))?;

        let days = gift_code
            .duration_days
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no duration)"))?;
        let plan_id = gift_code
            .plan_id
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no plan)"))?;

        let expires_at = Utc::now() + Duration::days(days as i64);
        let vless_uuid = Uuid::new_v4().to_string();
        let subscription_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(subscription_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = $1, redeemed_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(user_id)
            .bind(gift_code.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn transfer_subscription(
        &self,
        sub_id: i64,
        current_user_id: i64,
        target_username: &str,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(sub_id)
        .bind(current_user_id)
        .fetch_one(&mut *tx)
        .await?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!(
                "Only pending subscriptions can be transferred"
            ));
        }

        let target_user = sqlx::query_as::<_, caramba_db::models::store::User>(
            "SELECT * FROM users WHERE username = $1",
        )
        .bind(target_username.trim_start_matches('@'))
        .fetch_optional(&mut *tx)
        .await?;

        let target_user = target_user.ok_or_else(|| {
            anyhow::anyhow!("Target user not found. They must start the bot first.")
        })?;

        if target_user.id == current_user_id {
            return Err(anyhow::anyhow!("Cannot transfer to yourself"));
        }

        let updated_sub = sqlx::query_as::<_, Subscription>(
            r#"
            UPDATE subscriptions 
            SET user_id = $1 
            WHERE id = $2 
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(target_user.id)
        .bind(sub_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(updated_sub)
    }

    pub async fn admin_delete_subscription(&self, sub_id: i64) -> Result<()> {
        self.sub_repo.delete(sub_id).await?;
        Ok(())
    }

    pub async fn delete_subscription(&self, sub_id: i64, user_id: i64) -> Result<()> {
        let _sub = self.get_subscription(sub_id, user_id).await?;
        self.sub_repo.delete(sub_id).await?;
        Ok(())
    }

    pub async fn admin_refund_subscription(&self, sub_id: i64, amount: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let sub = self
            .sub_repo
            .get_by_id(sub_id)
            .await?
            .context("Subscription not found")?;
        self.sub_repo.delete(sub_id).await?;
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(sub.user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(sub.user_id),
            "Refund",
            &format!("Refunded sub {} (Amt: {})", sub_id, amount),
        )
        .await;

        tx.commit().await?;
        Ok(())
    }

    pub async fn admin_extend_subscription(&self, sub_id: i64, days: i32) -> Result<()> {
        let user_id: i64 = sqlx::query_scalar("UPDATE subscriptions SET expires_at = expires_at + ($1 * interval '1 day') WHERE id = $2 RETURNING user_id")
            .bind(days)
            .bind(sub_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to extend subscription")?;

        let _ = self.sync_family_subscriptions(user_id).await;
        Ok(())
    }

    pub async fn admin_gift_subscription(
        &self,
        user_id: i64,
        plan_id: i64,
        duration_days: i32,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        let active_nodes = self.node_repo.get_active_node_ids().await?;
        let node_id = active_nodes
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No active nodes available"))?;

        let vless_uuid = Uuid::new_v4().to_string();
        let sub_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, subscription_uuid, created_at)
            VALUES ($1, $2, $3, $4, $5, 'active', $6, CURRENT_TIMESTAMP)
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(user_id).bind(plan_id).bind(node_id).bind(vless_uuid).bind(expires_at).bind(sub_uuid)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        let _ = self.sync_family_subscriptions(user_id).await;

        Ok(sub)
    }

    pub async fn extend_subscription(
        &self,
        user_id: i64,
        duration_id: i64,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        let duration =
            sqlx::query_as::<_, PlanDuration>("SELECT * FROM plan_durations WHERE id = $1")
                .bind(duration_id)
                .fetch_one(&mut *tx)
                .await?;

        if user.balance < duration.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(duration.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let sub = self
            .extend_subscription_with_duration_internal(user_id, &duration, &mut tx)
            .await?;
        tx.commit().await?;
        Ok(sub)
    }

    async fn extend_subscription_with_duration_internal(
        &self,
        user_id: i64,
        duration: &caramba_db::models::store::PlanDuration,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Subscription> {
        let existing_sub = self.sub_repo.get_active_by_user(user_id).await?;

        let sub = if let Some(active_sub) = existing_sub {
            if active_sub.plan_id != duration.plan_id {
                let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
                let vless_uuid = Uuid::new_v4().to_string();
                let sub_uuid = Uuid::new_v4().to_string();
                let id = self
                    .sub_repo
                    .create(
                        user_id,
                        duration.plan_id,
                        &vless_uuid,
                        &sub_uuid,
                        expires_at,
                        "active",
                        None,
                        false,
                    )
                    .await?;
                self.sub_repo.get_by_id(id).await?.unwrap()
            } else {
                let new_expires_at = if active_sub.expires_at > Utc::now() {
                    active_sub.expires_at + Duration::days(duration.duration_days as i64)
                } else {
                    Utc::now() + Duration::days(duration.duration_days as i64)
                };
                self.sub_repo
                    .update_expiry(active_sub.id, new_expires_at)
                    .await?;
                self.sub_repo.get_by_id(active_sub.id).await?.unwrap()
            }
        } else {
            let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
            let vless_uuid = Uuid::new_v4().to_string();
            let sub_uuid = Uuid::new_v4().to_string();
            let id = self
                .sub_repo
                .create(
                    user_id,
                    duration.plan_id,
                    &vless_uuid,
                    &sub_uuid,
                    expires_at,
                    "active",
                    None,
                    false,
                )
                .await?;
            self.sub_repo.get_by_id(id).await?.unwrap()
        };

        let _ = self.sync_family_subscriptions(user_id).await;
        Ok(sub)
    }

    pub async fn get_user_gift_codes(&self, user_id: i64) -> Result<Vec<GiftCode>> {
        sqlx::query_as::<_, GiftCode>(
            "SELECT * FROM gift_codes
             WHERE created_by_user_id = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user gift codes")
    }

    pub async fn update_subscription_note(&self, sub_id: i64, note: String) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET note = $1 WHERE id = $2")
            .bind(note)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to update subscription note")?;
        Ok(())
    }

    pub async fn log_payment(
        &self,
        user_id: i64,
        method: &str,
        amount_cents: i64,
        external_id: Option<&str>,
        status: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO payments (user_id, method, amount, external_id, status) VALUES ($1, $2, $3, $4, $5)").bind(user_id).bind(method).bind(amount_cents).bind(external_id).bind(status).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn apply_referral_bonus(
        &self,
        pool: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        amount_cents: i64,
        payment_id: Option<i64>,
    ) -> Result<Option<(i64, i64)>> {
        ReferralService::apply_referral_bonus(pool, user_id, amount_cents, payment_id).await
    }

    pub async fn create_category(
        &self,
        name: &str,
        description: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO categories (name, description, sort_order) VALUES ($1, $2, $3)")
            .bind(name)
            .bind(description)
            .bind(sort_order)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_category(&self, id: i64) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE category_id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        if count > 0 {
            return Err(anyhow::anyhow!(
                "Cannot delete category with existing products"
            ));
        }
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_products(&self) -> Result<Vec<caramba_db::models::store::Product>> {
        sqlx::query_as::<_, caramba_db::models::store::Product>(
            "SELECT * FROM products ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch all products")
    }

    pub async fn create_product(
        &self,
        category_id: i64,
        name: &str,
        description: Option<&str>,
        price: i64,
        product_type: &str,
        content: Option<&str>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO products (category_id, name, description, price, product_type, content) VALUES ($1, $2, $3, $4, $5, $6)").bind(category_id).bind(name).bind(description).bind(price).bind(product_type).bind(content).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn delete_product(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM products WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_active_node_ids(&self) -> Result<Vec<i64>> {
        self.node_repo.get_active_node_ids().await
    }

    pub async fn update_user_referral_code(&self, user_id: i64, new_code: &str) -> Result<()> {
        ReferralService::update_user_referral_code(&self.pool, user_id, new_code).await
    }

    pub async fn validate_promo(
        &self,
        code: &str,
    ) -> Result<Option<caramba_db::models::promo::PromoCode>> {
        sqlx::query_as::<_, caramba_db::models::promo::PromoCode>(
            "SELECT * FROM promo_codes WHERE code = $1 AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) AND current_uses < max_uses AND is_active = TRUE"
        ).bind(code).fetch_optional(&self.pool).await.context("Failed to validate promo code")
    }

    pub async fn checkout_cart(&self, user_id: i64) -> Result<()> {
        let cart = self.get_user_cart(user_id).await?;
        if cart.is_empty() {
            return Err(anyhow::anyhow!("Cart is empty"));
        }
        let total_price: i64 = cart.iter().map(|item| item.price * item.quantity).sum();
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        if user.balance < total_price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }
        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(total_price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        let order_id: i64 = sqlx::query_scalar("INSERT INTO orders (user_id, total_amount, status, paid_at) VALUES ($1, $2, 'paid', CURRENT_TIMESTAMP) RETURNING id")
            .bind(user_id).bind(total_price).fetch_one(&mut *tx).await?;
        for item in cart {
            sqlx::query("INSERT INTO order_items (order_id, product_id, quantity, price) VALUES ($1, $2, $3, $4)").bind(order_id).bind(item.product_id).bind(item.quantity).bind(item.price).execute(&mut *tx).await?;
        }
        sqlx::query("DELETE FROM cart_items WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Checkout",
            &format!("Checkout complete. Total: {}", total_price),
        )
        .await;

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_user_cart(&self, user_id: i64) -> Result<Vec<CartItem>> {
        sqlx::query_as::<_, CartItem>(
            "SELECT c.id, c.user_id, c.product_id, c.quantity, p.name as product_name, p.price FROM cart_items c JOIN products p ON c.product_id = p.id WHERE c.user_id = $1"
        ).bind(user_id).fetch_all(&self.pool).await.context("Failed to fetch cart")
    }

    pub async fn add_to_cart(&self, user_id: i64, product_id: i64, quantity: i64) -> Result<()> {
        sqlx::query("INSERT INTO cart_items (user_id, product_id, quantity) VALUES ($1, $2, $3) ON CONFLICT(user_id, product_id) DO UPDATE SET quantity = cart_items.quantity + EXCLUDED.quantity")
            .bind(user_id).bind(product_id).bind(quantity).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn clear_cart(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
