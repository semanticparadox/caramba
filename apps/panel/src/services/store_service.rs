use sqlx::{SqlitePool, Row};
use anyhow::{Context, Result};
use chrono::{Utc, Duration};
use tracing::{info, error};
use uuid::Uuid;
use crate::services::referral_service::ReferralService;

use crate::models::store::{
    User, Plan, Subscription, GiftCode, PlanDuration, 
    RenewalResult, AlertType, DetailedReferral, SubscriptionWithDetails, CartItem
};
use crate::models::network::InboundType;
use crate::repositories::user_repo::UserRepository;
use crate::repositories::subscription_repo::SubscriptionRepository;
use crate::repositories::node_repo::NodeRepository;
use crate::repositories::api_key_repo::ApiKeyRepository;

#[derive(Debug, Clone)]
pub struct StoreService {
    pool: SqlitePool,
    user_repo: UserRepository,
    pub sub_repo: SubscriptionRepository,
    pub node_repo: NodeRepository,
    pub api_key_repo: ApiKeyRepository,
}

impl StoreService {
    pub fn new(pool: SqlitePool) -> Self {
        let user_repo = UserRepository::new(pool.clone());
        let sub_repo = SubscriptionRepository::new(pool.clone());
        let node_repo = NodeRepository::new(pool.clone());
        let api_key_repo = ApiKeyRepository::new(pool.clone());
        Self { pool, user_repo, sub_repo, node_repo, api_key_repo }
    }

    pub fn get_pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    pub async fn get_products_by_category(&self, category_id: i64) -> Result<Vec<crate::models::store::Product>> {
        sqlx::query_as::<_, crate::models::store::Product>(
            "SELECT id, category_id, name, description, price, product_type, content, is_active, created_at FROM products WHERE category_id = ? AND is_active = 1"
        )
        .bind(category_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch products")
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<crate::models::node::Node>> {
        self.node_repo.get_active_nodes().await
    }

    pub async fn get_api_keys(&self) -> Result<Vec<crate::models::api_key::ApiKey>> {
        self.api_key_repo.get_all().await
    }

    pub async fn create_api_key(&self, name: &str, key: &str, max_uses: Option<i64>) -> Result<crate::models::api_key::ApiKey> {
        self.api_key_repo.create(name, key, max_uses).await
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<()> {
        self.api_key_repo.delete(id).await
    }

    /// Public wrapper for SubscriptionRepository::get_active_subs_by_plans
    pub async fn get_active_subs_by_plans(&self, plan_ids: &[i64]) -> Result<Vec<(Option<String>, i64, Option<String>)>> {
        self.sub_repo.get_active_subs_by_plans(plan_ids).await
    }

    /// Public wrapper for SubscriptionRepository::get_by_uuid
    pub async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Option<crate::models::store::Subscription>> {
        self.sub_repo.get_by_uuid(uuid).await
    }

    /// Public wrapper for SubscriptionRepository::update_status
    pub async fn update_subscription_status(&self, sub_id: i64, status: &str) -> Result<()> {
        self.sub_repo.update_status(sub_id, status).await
    }

    /// Reset warning count for a user (admin action)
    pub async fn reset_warning_count(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_warning_count(user_id, 0).await
    }

    /// Get nodes available to a user based on their active subscription plan's groups
    pub async fn get_user_nodes(&self, user_id: i64) -> Result<Vec<crate::models::node::Node>> {
        // 1. Get active plan_id for user
        let plan_id: Option<i64> = self.sub_repo.get_active_plan_id_by_user(user_id).await?;

        let plan_id = match plan_id {
            Some(id) => id,
            None => return Ok(vec![]), // No active plan = no nodes
        };

        // 2. Get nodes via Plan -> Groups -> Nodes
        // The repository handles the logic: Groups -> Nodes, Fallback -> All Active
        let nodes = self.node_repo.get_nodes_for_plan(plan_id).await?;
        
        Ok(nodes)
    }

    pub async fn get_user_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        self.user_repo.get_by_tg_id(tg_id).await
    }

    pub async fn get_user_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        self.user_repo.get_by_referral_code(code).await
    }

    pub async fn resolve_referrer_id(&self, code: &str) -> Result<Option<i64>> {
        // Try parsing as i64 (TG ID)
        if let Ok(tg_id) = code.parse::<i64>() {
            if let Some(user) = self.get_user_by_tg_id(tg_id).await? {
                return Ok(Some(user.id));
            }
        }
        
        // Try as alias
        if let Some(user) = self.get_user_by_referral_code(code).await? {
            return Ok(Some(user.id));
        }
        
        Ok(None)
    }

    pub async fn upsert_user(&self, tg_id: i64, username: Option<&str>, full_name: Option<&str>, referrer_id: Option<i64>) -> Result<User> {
        let existing = self.user_repo.get_by_tg_id(tg_id).await?;
        
        let user = self.user_repo.upsert(tg_id, username, full_name, referrer_id).await?;

        // Analytics Hooks
        if existing.is_none() {
             let _ = crate::services::analytics_service::AnalyticsService::track_new_user(&self.pool).await;
        }
        let _ = crate::services::analytics_service::AnalyticsService::track_active_user(&self.pool, user.id).await;

        // Sync family if this user is a child (referrer logic might be separate, but if parent_id was set...)
        // Note: upsert_user doesn't set parent_id yet. We'll need a separate method for that.
        
        Ok(user)
    }

    // ========================================================================
    // Family Invites
    // ========================================================================

    pub async fn create_family_invite(&self, parent_id: i64, max_uses: i32, duration_days: i32) -> Result<crate::models::store::FamilyInvite> {
        // Generate a random code. Format: FAMILY-XXXXXX (6 chars)
        let random_part = Uuid::new_v4().to_string().replace("-", "").chars().take(6).collect::<String>().to_uppercase();
        let code = format!("FAMILY-{}", random_part);
        
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        let invite = sqlx::query_as::<_, crate::models::store::FamilyInvite>(
            r#"
            INSERT INTO family_invites (code, parent_id, max_uses, expires_at)
            VALUES (?, ?, ?, ?)
            RETURNING *
            "#
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

    pub async fn get_valid_invite(&self, code: &str) -> Result<Option<crate::models::store::FamilyInvite>> {
        let invite = sqlx::query_as::<_, crate::models::store::FamilyInvite>(
            r#"
            SELECT * FROM family_invites 
            WHERE code = ? 
            AND expires_at > datetime('now')
            AND used_count < max_uses
            "#
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        
        Ok(invite)
    }

    pub async fn redeem_family_invite(&self, user_id: i64, code: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // 1. Get and Lock Invite
        let invite = sqlx::query_as::<_, crate::models::store::FamilyInvite>(
            "SELECT * FROM family_invites WHERE code = ? AND expires_at > datetime('now') AND used_count < max_uses LIMIT 1"
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let invite = match invite {
            Some(i) => i,
            None => return Err(anyhow::anyhow!("Invalid or expired invite code")),
        };

        // 2. Initial Checks
        if invite.parent_id == user_id {
            return Err(anyhow::anyhow!("You cannot invite yourself"));
        }

        // Check if user is already a child of someone? 
        // Policy: Overwrite parent? Or fail? Let's fail if they already have a parent to prevent abuse/accidents.
        let current_user: User = sqlx::query_as("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
            
        if let Some(pid) = current_user.parent_id {
            if pid == invite.parent_id {
                return Err(anyhow::anyhow!("You are already in this family"));
            }
            return Err(anyhow::anyhow!("You are already a member of another family"));
        }

        // 3. Link User to Parent
        sqlx::query("UPDATE users SET parent_id = ? WHERE id = ?")
            .bind(invite.parent_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // 4. Increment usage
        sqlx::query("UPDATE family_invites SET used_count = used_count + 1 WHERE id = ?")
            .bind(invite.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        // 5. Sync Subscriptions (Auto-grant access)
        // We do this outside the tx to reuse the existing method logic which might start its own tx or be complex
        self.sync_family_subscriptions(invite.parent_id).await?;

        Ok(())
    }

    pub async fn get_family_members(&self, parent_id: i64) -> Result<Vec<User>> {
        self.user_repo.get_by_parent_id(parent_id).await
    }

    pub async fn set_user_parent(&self, user_id: i64, parent_id: Option<i64>) -> Result<()> {
        // Update parent_id for this user
        self.user_repo.set_parent_id(user_id, parent_id).await?;
            
        // If parent set, sync immediately
        if let Some(pid) = parent_id {
            self.sync_family_subscriptions(pid).await?;
        }
        Ok(())
    }

    pub async fn sync_family_subscriptions(&self, parent_id: i64) -> Result<()> {
        // 1. Get Parent's Active Subscription
        let parent_sub = self.sub_repo.get_active_by_user(parent_id).await?;

        // 2. Get Children
        let children = self.get_family_members(parent_id).await?;
        if children.is_empty() { return Ok(()); }

        let tx = self.pool.begin().await?;

        if let Some(psub) = parent_sub {
            // Parent has active sub -> Grant/Update for children
            for child in children {
                // Check if child has their OWN paid subscription (not a family one)
                // We assume 'note' = 'Family Plan' implies it's a synced sub.
                // If they have a sub with different plan or no note, maybe skip?
                // For simplified Phase 2: Overwrite/Extend child's active sub if it exists, or create new.
                // Better: Check for ANY active sub.
                let child_sub = self.sub_repo.get_active_by_user(child.id).await?;

                if let Some(csub) = child_sub {
                    // Update existing
                    // Only update if it looks like a family sub or if we force it (Policy: Family overrides? Or Child Paid overrides?)
                    // Let's say: If child sub plan_id != parent plan_id, assume independent.
                    // But if plan_id is same, sync expiry.
                    // Actually, let's mark family subs with note="Family".
                    if csub.note.as_deref() == Some("Family") || csub.plan_id == psub.plan_id {
                        self.sub_repo.update_family_sub(csub.id, psub.expires_at, psub.plan_id, psub.node_id).await?;
                    }
                } else {
                    // Create new Family Sub
                    let vless_uuid = Uuid::new_v4().to_string();
                    let sub_uuid = Uuid::new_v4().to_string();
                    self.sub_repo.create(
                        child.id,
                        psub.plan_id,
                        &vless_uuid,
                        &sub_uuid,
                        psub.expires_at,
                        "active",
                        Some("Family"),
                        false
                    ).await?;
                }
            }
        } else {
            // Parent has NO active sub -> Expire family subs for children
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
        let user = self.user_repo.get_by_id(user_id).await?
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;
        self.user_repo.update_profile(user_id, user.balance, true, user.referral_code.as_deref()).await?;
        Ok(())
    }

    pub async fn update_user_language(&self, user_id: i64, lang: &str) -> Result<()> {
        self.user_repo.update_language(user_id, lang).await?;
        Ok(())
    }

    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        // Legacy: kept for compatibility but superseded by history tracking
        self.user_repo.update_last_bot_msg_id(user_id, msg_id).await?;
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
        // 1. Get IDs to delete (keeping latest N)
        // Note: LIMIT -1 OFFSET N is SQLite syntax for "All after N"
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

        // 2. Delete from DB
        let id_list = ids_to_delete.iter().map(|(id, _, _)| id.to_string()).collect::<Vec<_>>().join(",");
        let query = format!("DELETE FROM bot_chat_history WHERE id IN ({})", id_list);
        sqlx::query(&query).execute(&self.pool).await?;

        // 3. Return chat_id, message_id for Telegram deletion
        Ok(ids_to_delete.into_iter().map(|(_, chat_id, msg_id)| (chat_id, msg_id)).collect())
    }

    pub async fn update_user_terms(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_terms_accepted(user_id).await?;
        Ok(())
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let res = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(res)
    }

    pub async fn update_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        let mut plans = match sqlx::query_as::<_, Plan>(
        "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_active = 1"
    )
    .fetch_all(&self.pool)
    .await {
        Ok(p) => {
            info!("Successfully fetched {} active plans from DB", p.len());
            p
        },
        Err(e) => {
            error!("Failed to fetch active plans: {}", e);
            return Err(e.into());
        }
    };

        if plans.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch all durations for these plans in one go
        let plan_ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        let plan_ids_str = plan_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let query = format!("SELECT * FROM plan_durations WHERE plan_id IN ({}) ORDER BY duration_days ASC", plan_ids_str);
        
        let all_durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(&query)
            .fetch_all(&self.pool)
            .await?;

        for plan in &mut plans {
            plan.durations = all_durations.iter()
                .filter(|d| d.plan_id == plan.id)
                .cloned()
                .collect();
        }

        Ok(plans)
    }

    pub async fn purchase_plan(&self, user_id: i64, duration_id: i64) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        // 1. Get user and duration
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        
        let duration = sqlx::query_as::<_, crate::models::store::PlanDuration>("SELECT * FROM plan_durations WHERE id = ?")
            .bind(duration_id)
            .fetch_one(&mut *tx)
            .await?;

        // 2. Check balance
        if user.balance < duration.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        // 3. Deduct balance
        sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
            .bind(duration.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // 4. Create NEW subscription (Pending)
        // We store the intended expiration duration by setting expires_at = now + duration.
        // When activated, we will recalculate: duration = expires_at - created_at, then NewExpiry = Now + duration.
        let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
        let vless_uuid = uuid::Uuid::new_v4().to_string();
        let sub_uuid = uuid::Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status)
            VALUES (?, ?, ?, ?, ?, 'pending')
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

        // Analytics
        let _ = crate::services::analytics_service::AnalyticsService::track_order(&self.pool).await;

        Ok(sub)
    }

    pub async fn activate_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let tx = self.pool.begin().await?;

        let sub = self.get_subscription(sub_id, user_id).await?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Subscription is not pending"));
        }

        // Calculate duration from original intent (expires_at - created_at)
        let duration = sub.expires_at - sub.created_at;
        let new_expires_at = Utc::now() + duration;

        self.sub_repo.update_status_and_expiry(sub_id, "active", new_expires_at).await?;
        let updated_sub = self.sub_repo.get_by_id(sub_id).await?.unwrap();

        tx.commit().await?;

        // Sync family members
        let _ = self.sync_family_subscriptions(user_id).await.map_err(|e| error!("Failed to sync family subs: {}", e));
        Ok(updated_sub)
    }

    pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let sub = self.sub_repo.get_by_id(sub_id).await?
            .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;
            
        if sub.user_id != user_id {
            return Err(anyhow::anyhow!("Unauthorized access to subscription"));
        }
        
        Ok(sub)
    }

    pub async fn convert_subscription_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        // 1. Fetch Subscription
        let sub = self.get_subscription(sub_id, user_id).await?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Only pending subscriptions can be converted to gifts"));
        }

        // 2. Calculate duration
        // For pending subs, duration is stored in (expires_at - created_at)
        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        // 3. Delete Subscription
        self.sub_repo.delete(sub_id).await?;

        // 4. Generate Code
        let code = format!("EXA-GIFT-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("CODE").to_uppercase());

        // 5. Create Gift Code Record
        sqlx::query(
            "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES (?, ?, ?, ?)"
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

        // 1. Verify Code matches and is not redeemed
        let gift_code_opt = sqlx::query_as::<_, crate::models::store::GiftCode>(
            "SELECT * FROM gift_codes WHERE code = ? AND redeemed_by_user_id IS NULL"
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let gift_code = gift_code_opt.ok_or_else(|| anyhow::anyhow!("Invalid or already redeemed code"))?;

        // 2. Create Subscription (Pending)
        // expires_at = now + duration_days (same logic as purchase_plan)
        let days = gift_code.duration_days.ok_or_else(|| anyhow::anyhow!("Gift code invalid (no duration)"))?;
        let plan_id = gift_code.plan_id.ok_or_else(|| anyhow::anyhow!("Gift code invalid (no plan)"))?;
        
        let expires_at = Utc::now() + Duration::days(days as i64);
        let vless_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, expires_at, status)
            VALUES (?, ?, ?, ?, 'pending')
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        // 3. Mark Code as Redeemed
        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = ?, redeemed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .bind(gift_code.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn transfer_subscription(&self, sub_id: i64, current_user_id: i64, target_username: &str) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ? AND user_id = ?")
            .bind(sub_id)
            .bind(current_user_id)
            .fetch_one(&mut *tx)
            .await?;

        if sub.status != "pending" {
            // return Err(anyhow::anyhow!("Only pending subscriptions can be transferred")); 
            // Actually, requirements often allow active transfers, but for simplicity let's keep it pending?
            // "Transfer" logic usually implies moving the ownership.
            // If active, we might need to regenerate UUIDs or update nodes. 
            // For now, let's stick to Pending only as per initial requirement, or relax it if needed.
            // But wait, the user request was about "converting pending to gift".
            // Direct transfer might be deprecated? The code exists though.
            // Let's leave it as is.
             return Err(anyhow::anyhow!("Only pending subscriptions can be transferred"));
        }

        // ... existing transfer logic ...




        // Find target user
        let target_user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
            .bind(target_username.trim_start_matches('@'))
            .fetch_optional(&mut *tx)
            .await?;

        let target_user = target_user.ok_or_else(|| anyhow::anyhow!("Target user not found. They must start the bot first."))?;

        if target_user.id == current_user_id {
            return Err(anyhow::anyhow!("Cannot transfer to yourself"));
        }

        let updated_sub = sqlx::query_as::<_, Subscription>(
            r#"
            UPDATE subscriptions 
            SET user_id = ? 
            WHERE id = ? 
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

        // 1. Get Sub to find user
        let sub = self.sub_repo.get_by_id(sub_id).await?
            .context("Subscription not found")?;

        // 2. Delete Sub
        self.sub_repo.delete(sub_id).await?;

        // 3. Credit User
        sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
            .bind(amount)
            .bind(sub.user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn admin_extend_subscription(&self, sub_id: i64, days: i32) -> Result<()> {
        let user_id: i64 = sqlx::query_scalar("UPDATE subscriptions SET expires_at = datetime(expires_at, '+' || ? || ' days') WHERE id = ? RETURNING user_id")
            .bind(days)
            .bind(sub_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to extend subscription")?;

        let _ = self.sync_family_subscriptions(user_id).await.map_err(|e| error!("Failed to sync family subs: {}", e));
        Ok(())
    }

    pub async fn admin_gift_subscription(&self, user_id: i64, plan_id: i64, duration_days: i32) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        // 1. Select an active node
        // Simple strategy: pick the first available active node. 
        let active_nodes = self.node_repo.get_active_node_ids().await?;
        let node_id = active_nodes.first().cloned()
            .ok_or_else(|| anyhow::anyhow!("No active nodes available to assign"))?;

        // 2. Prepare subscription data
        let vless_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        // 3. Create Active Subscription
        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, created_at)
            VALUES (?, ?, ?, ?, ?, 'active', CURRENT_TIMESTAMP)
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(node_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        // Sync family members
        let _ = self.sync_family_subscriptions(user_id).await.map_err(|e| error!("Failed to sync family subs: {}", e));

        Ok(sub)
    }

    pub async fn get_plan_duration_by_id(&self, duration_id: i64) -> Result<Option<PlanDuration>> {
        let duration = sqlx::query_as::<_, PlanDuration>("SELECT * FROM plan_durations WHERE id = ?")
            .bind(duration_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch plan duration")?;
        Ok(duration)
    }


    pub async fn extend_subscription(&self, user_id: i64, duration_id: i64) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        // 1. Get user and duration
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        
        let duration = sqlx::query_as::<_, crate::models::store::PlanDuration>("SELECT * FROM plan_durations WHERE id = ?")
            .bind(duration_id)
            .fetch_one(&mut *tx)
            .await?;

        // 2. Check balance
        if user.balance < duration.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        // 3. Deduct balance
        sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
            .bind(duration.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // 4. Check for existing active subscription
        let sub = self.extend_subscription_with_duration_internal(user_id, &duration, &mut tx).await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn extend_subscription_with_duration(&self, user_id: i64, duration: &crate::models::store::PlanDuration) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        let sub = self.extend_subscription_with_duration_internal(user_id, duration, &mut tx).await?;
        tx.commit().await?;
        Ok(sub)
    }

    async fn extend_subscription_with_duration_internal(&self, user_id: i64, duration: &crate::models::store::PlanDuration, _tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<Subscription> {

        // 1. Check for existing active subscription
        let existing_sub = self.sub_repo.get_active_by_user(user_id).await?;

        // Logic check: The original query filtered by plan_id too.
        // "WHERE user_id = ? AND plan_id = ? AND status = 'active'"
        // Repo `get_active_by_user` returns ANY active sub.
        // If the user attempts to extend a DIFFERENT plan, we should probably support that or fail?
        // Original logic implies we look for specific plan match. 
        // Let's rely on repo but manually check plan_id if needed, or update repo.
        // Actually, let's keep it simple: if there is an active sub, we extend it.
        // But wait, what if they have active Plan A and buy Plan B?
        // The original code filtered by `duration.plan_id`.
        // So we strictly only extended if the SAME plan was active.
        
        let sub = if let Some(active_sub) = existing_sub {
            if active_sub.plan_id != duration.plan_id {
                 // Different plan, treat as new? Or error?
                 // Original Code: "WHERE ... plan_id = ?"
                 // So if they had Plan A active, and bought Plan B, existing_sub would be None.
                 // And we would create a NEW subscription (line 945).
                 // So they would have TWO active subscriptions?
                 // That seems to be the logic.
                 
                 // Fallback to create new
                 let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
                 let vless_uuid = Uuid::new_v4().to_string();
                 let sub_uuid = Uuid::new_v4().to_string();
                 
                  let id = self.sub_repo.create(
                     user_id,
                     duration.plan_id,
                     &vless_uuid,
                     &sub_uuid,
                     expires_at,
                     "active",
                     None,
                     false
                 ).await?;
                 
                 self.sub_repo.get_by_id(id).await?.unwrap()
            } else {
                // Extend existing
                let new_expires_at = if active_sub.expires_at > Utc::now() {
                    active_sub.expires_at + Duration::days(duration.duration_days as i64)
                } else {
                    Utc::now() + Duration::days(duration.duration_days as i64)
                };

                self.sub_repo.update_expiry(active_sub.id, new_expires_at).await?;
                self.sub_repo.get_by_id(active_sub.id).await?.unwrap()
            }
        } else {
             // Create new if none active
             let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
             let vless_uuid = Uuid::new_v4().to_string();
             let sub_uuid = Uuid::new_v4().to_string();
             
             let id = self.sub_repo.create(
                 user_id,
                 duration.plan_id,
                 &vless_uuid,
                 &sub_uuid,
                 expires_at,
                 "active",
                 None,
                 false
             ).await?;
             
             self.sub_repo.get_by_id(id).await?.unwrap()
        };


        // Sync family members
        let _ = self.sync_family_subscriptions(user_id).await.map_err(|e| error!("Failed to sync family subs: {}", e));

        Ok(sub)
    }

    pub async fn get_user_subscriptions(&self, user_id: i64) -> Result<Vec<SubscriptionWithDetails>> {
        // 1. Fetch Subscriptions using Repo
        let subs = self.sub_repo.get_all_by_user(user_id).await?;

        if subs.is_empty() {
            return Ok(Vec::new());
        }

        // 2. Fetch Plans with Durations (cached or fresh)
        let plans = self.get_active_plans().await?; 

        let mut result = Vec::new();

        for sub_with_details in subs {
            let sub = &sub_with_details.sub;
            let plan = plans.iter().find(|p| p.id == sub.plan_id);
            
            let (name, desc, limit) = if let Some(p) = plan {
                // Find duration with closest days to (expires_at - created_at)
                let actual_days = (sub.expires_at - sub.created_at).num_days();
                
                // Find PlanDuration with closest duration_days
                let _best_dur = p.durations.iter().min_by_key(|d| (d.duration_days as i64 - actual_days).abs());
                
                let limit = Some(p.traffic_limit_gb);
                
                (p.name.clone(), p.description.clone(), limit)
            } else {
                (sub_with_details.plan_name.clone(), None, None)
            };

            result.push(SubscriptionWithDetails {
                sub: sub.clone(),
                plan_name: name,
                plan_description: desc,
                traffic_limit_gb: limit,
            });
        }

        Ok(result)
    }

    pub async fn get_user_gift_codes(&self, user_id: i64) -> Result<Vec<GiftCode>> {
        sqlx::query_as::<_, GiftCode>(
            "SELECT * FROM gift_codes WHERE created_by_user_id = ? AND redeemed_by_user_id IS NULL ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user gift codes")
    }

    pub async fn update_subscription_note(&self, sub_id: i64, note: String) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET note = ? WHERE id = ?")
            .bind(note)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to update subscription note")?;
        Ok(())
    }

    pub async fn update_subscription_node(&self, sub_id: i64, node_id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET node_id = ? WHERE id = ?")
            .bind(node_id)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to update subscription node")?;
        Ok(())
    }



    pub async fn log_payment(&self, user_id: i64, method: &str, amount_cents: i64, external_id: Option<&str>, status: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO payments (user_id, method, amount, external_id, status) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(user_id)
        .bind(method)
        .bind(amount_cents)
        .bind(external_id)
        .bind(status)
        .execute(&self.pool)
        .await
        .context("Failed to log payment")?;
        Ok(())
    }

    pub async fn apply_referral_bonus(&self, pool: &mut sqlx::Transaction<'_, sqlx::Sqlite>, user_id: i64, amount_cents: i64, payment_id: Option<i64>) -> Result<Option<(i64, i64)>> {
        ReferralService::apply_referral_bonus(pool, user_id, amount_cents, payment_id).await
    }

    // ========================================================================
    // Category Management
    // ========================================================================

    pub async fn create_category(&self, name: &str, description: Option<&str>, sort_order: Option<i32>) -> Result<()> {
        sqlx::query("INSERT INTO categories (name, description, sort_order) VALUES (?, ?, ?)")
            .bind(name)
            .bind(description)
            .bind(sort_order)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_category(&self, id: i64) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE category_id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        if count > 0 {
            return Err(anyhow::anyhow!("Cannot delete category with existing products"));
        }

        sqlx::query("DELETE FROM categories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ========================================================================
    // Product Management
    // ========================================================================

    pub async fn get_all_products(&self) -> Result<Vec<crate::models::store::Product>> {
        sqlx::query_as::<_, crate::models::store::Product>("SELECT * FROM products ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all products")
    }

    pub async fn create_product(&self, category_id: i64, name: &str, description: Option<&str>, price: i64, product_type: &str, content: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO products (category_id, name, description, price, product_type, content) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(category_id)
        .bind(name)
        .bind(description)
        .bind(price)
        .bind(product_type)
        .bind(content)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_product(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM products WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ========================================================================
    // Plan Management
    // ========================================================================

    pub async fn get_plans_admin(&self) -> Result<Vec<Plan>> {
        let mut plans = sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_trial = 0")
            .fetch_all(&self.pool)
            .await?;

        for plan in &mut plans {
            let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC"
            )
            .bind(plan.id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
            plan.durations = durations;
        }

        Ok(plans)
    }

    pub async fn get_plan_by_id(&self, id: i64) -> Result<Option<Plan>> {
        let plan_opt = sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(mut plan) = plan_opt {
            let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC"
            )
            .bind(plan.id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
            plan.durations = durations;
            Ok(Some(plan))
        } else {
            Ok(None)
        }
    }

    pub async fn get_plan_group_ids(&self, plan_id: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar("SELECT group_id FROM plan_groups WHERE plan_id = ?")
            .bind(plan_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(ids)
    }

    pub async fn create_plan(&self, name: &str, description: &str, device_limit: i32, traffic_limit_gb: i32, duration_days: Vec<i32>, prices: Vec<i64>, group_ids: Vec<i64>) -> Result<i64> {
        let mut tx = self.pool.begin().await?;

        let plan_id: i64 = sqlx::query("INSERT INTO plans (name, description, is_active, price, traffic_limit_gb, device_limit) VALUES (?, ?, 1, 0, ?, ?) RETURNING id")
            .bind(name)
            .bind(description)
            .bind(traffic_limit_gb)
            .bind(device_limit)
            .fetch_one(&mut *tx)
            .await
            .context("Failed to insert plan")?
            .get(0);

        let count = duration_days.len().min(prices.len());
        for i in 0..count {
            sqlx::query("INSERT INTO plan_durations (plan_id, duration_days, price) VALUES (?, ?, ?)")
                .bind(plan_id)
                .bind(duration_days[i])
                .bind(prices[i])
                .execute(&mut *tx)
                .await?;
        }

        for group_id in group_ids {
            sqlx::query("INSERT INTO plan_groups (plan_id, group_id) VALUES (?, ?)")
                .bind(plan_id)
                .bind(group_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(plan_id)
    }

    pub async fn update_plan(&self, id: i64, name: &str, description: &str, device_limit: i32, traffic_limit_gb: i32, duration_days: Vec<i32>, prices: Vec<i64>, group_ids: Vec<i64>) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE plans SET name = ?, description = ?, device_limit = ?, traffic_limit_gb = ? WHERE id = ?")
            .bind(name)
            .bind(description)
            .bind(device_limit)
            .bind(traffic_limit_gb)
            .bind(id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM plan_durations WHERE plan_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        let count = duration_days.len().min(prices.len());
        for i in 0..count {
            sqlx::query("INSERT INTO plan_durations (plan_id, duration_days, price) VALUES (?, ?, ?)")
                .bind(id)
                .bind(duration_days[i])
                .bind(prices[i])
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("DELETE FROM plan_groups WHERE plan_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        for group_id in group_ids {
            sqlx::query("INSERT INTO plan_groups (plan_id, group_id) VALUES (?, ?)")
                .bind(id)
                .bind(group_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn is_trial_plan(&self, id: i64) -> Result<bool> {
        let is_trial: bool = sqlx::query_scalar("SELECT is_trial FROM plans WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(false);
        Ok(is_trial)
    }

    // ========================================================================
    // Settings Helpers (Node/Trial)
    // ========================================================================

    pub async fn get_active_node_ids(&self) -> Result<Vec<i64>> {
        self.node_repo.get_active_node_ids().await
    }

    pub async fn update_trial_plan_limits(&self, device_limit: i32, traffic_limit_gb: i32) -> Result<()> {
        let trial_plan_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plans WHERE is_trial = 1)")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(false);

        if trial_plan_exists {
            sqlx::query("UPDATE plans SET device_limit = ?, traffic_limit_gb = ? WHERE is_trial = 1")
                .bind(device_limit)
                .bind(traffic_limit_gb)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn get_user_referrals(&self, referrer_id: i64) -> Result<Vec<DetailedReferral>> {
        ReferralService::get_user_referrals(&self.pool, referrer_id).await
    }

    pub async fn get_user_referral_earnings(&self, referrer_id: i64) -> Result<i64> {
        ReferralService::get_user_referral_earnings(&self.pool, referrer_id).await
    }

    pub async fn get_referral_count(&self, user_id: i64) -> Result<i64> {
        ReferralService::get_referral_count(&self.pool, user_id).await
    }

    pub async fn validate_promo(&self, code: &str) -> Result<Option<crate::models::store::PromoCode>> {
        sqlx::query_as::<_, crate::models::store::PromoCode>(
            "SELECT id, code, discount_percent, bonus_amount, max_uses, current_uses, expires_at, created_at FROM promo_codes WHERE code = ? AND (expires_at IS NULL OR expires_at > ?) AND current_uses < max_uses"
        )
        .bind(code)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await
        .context("Failed to validate promo code")
    }

    pub async fn process_order_payment(&self, order_id: i64) -> Result<()> {
        sqlx::query("UPDATE orders SET status = 'paid', paid_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(order_id)
            .execute(&self.pool)
            .await
            .context("Failed to update order status")?;
        Ok(())
    }

    pub async fn purchase_product_with_balance(&self, user_id: i64, product_id: i64) -> Result<crate::models::store::Product> {
        let mut tx = self.pool.begin().await?;

        // 1. Fetch User (for balance check)
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .context("User not found")?;

        // 2. Fetch Product (to get price and checks)
        let product = sqlx::query_as::<_, crate::models::store::Product>("SELECT * FROM products WHERE id = ? AND is_active = 1")
            .bind(product_id)
            .fetch_one(&mut *tx)
            .await
            .context("Product not found or inactive")?;

        // 3. Check Balance
        if user.balance < product.price {
             return Err(anyhow::anyhow!("Insufficient balance"));
        }

        // 4. Deduct Balance
        self.user_repo.adjust_balance(user_id, product.price).await?;

        // 5. Create Order
        use sqlx::Row;
        let order_id: i64 = sqlx::query("INSERT INTO orders (user_id, total_amount, status, paid_at) VALUES (?, ?, 'paid', ?) RETURNING id")
            .bind(user_id)
            .bind(product.price)
            .bind(Utc::now())
            .fetch_one(&mut *tx)
            .await?
            .get(0);

        // 6. Create Order Item
        sqlx::query("INSERT INTO order_items (order_id, product_id, price) VALUES (?, ?, ?)")
            .bind(order_id)
            .bind(product_id)
            .bind(product.price)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        
        Ok(product)
    }

    pub async fn get_product(&self, product_id: i64) -> Result<crate::models::store::Product> {
        sqlx::query_as::<_, crate::models::store::Product>("SELECT * FROM products WHERE id = ?")
            .bind(product_id)
            .fetch_one(&self.pool)
            .await
            .context("Product not found")
    }

    pub async fn get_user_purchased_products(&self, user_id: i64) -> Result<Vec<crate::models::store::Product>> {
        sqlx::query_as::<_, crate::models::store::Product>(
            r#"
            SELECT p.* 
            FROM products p
            JOIN order_items oi ON oi.product_id = p.id
            JOIN orders o ON o.id = oi.order_id
            WHERE o.user_id = ? AND o.status = 'paid'
            ORDER BY o.paid_at DESC
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user purchased products")
    }

    pub async fn get_subscription_links(&self, sub_id: i64) -> Result<Vec<String>> {
        let mut links = Vec::new();

        // 1. Get subscription
        let mut sub = self.sub_repo.get_by_id(sub_id).await?
            .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;
        
        // Self-healing: Generate subscription_uuid if missing
        if sub.subscription_uuid.is_empty() {
            let new_uuid = uuid::Uuid::new_v4().to_string();
            sqlx::query("UPDATE subscriptions SET subscription_uuid = ? WHERE id = ?")
                .bind(&new_uuid)
                .bind(sub_id)
                .execute(&self.pool)
                .await?;
            sub.subscription_uuid = new_uuid;
        }

        let uuid = sub.vless_uuid.clone().unwrap_or_default();
            
            // 2. Get Inbounds for this Plan
            let inbounds = self.node_repo.get_inbounds_for_plan(sub.plan_id).await?;
            
            for inbound in inbounds {
                // Parse stream settings to find SNI/Security
                use crate::models::network::{StreamSettings};
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or(StreamSettings {
                    network: Some("tcp".to_string()),
                    security: Some("none".to_string()),
                    tls_settings: None,
                    reality_settings: None,
                    ws_settings: None,
                    http_upgrade_settings: None,
                    xhttp_settings: None,
                    packet_encoding: None,
                });
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let node = self.node_repo.get_node_by_id(inbound.node_id).await?
                    .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
                let address = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    node.ip.clone()
                } else {
                    inbound.listen_ip.clone()
                };

                let port = inbound.listen_port;
                let remark = inbound.tag.clone();

                match inbound.protocol.as_str() {
                    "vless" => {
                        // vless://uuid@ip:port?security=...&sni=...&fp=...&type=...#remark
                        let mut params = Vec::new();
                        params.push(format!("security={}", security));
                        
                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                let sni = node.reality_sni.clone().unwrap_or_else(|| {
                                    reality.server_names.first().cloned().unwrap_or_else(|| node.domain.clone().unwrap_or_default())
                                });
                                let pub_key = reality.public_key.clone()
                                    .or_else(|| node.reality_pub.clone())
                                    .unwrap_or_default();
                                let short_id = reality.short_ids.first().cloned()
                                    .or_else(|| node.short_id.clone())
                                    .unwrap_or_default();

                                params.push(format!("sni={}", sni));
                                params.push(format!("pbk={}", pub_key)); 
                                if !short_id.is_empty() {
                                    params.push(format!("sid={}", short_id));
                                }
                                params.push("fp=chrome".to_string());
                            }
                        } else if security == "tls" {
                            let sni = node.reality_sni.clone()
                                .or_else(|| stream.tls_settings.as_ref().map(|t| t.server_name.clone()))
                                .unwrap_or_else(|| node.domain.clone().unwrap_or_default());
                            params.push(format!("sni={}", sni));
                        }
                        
                        params.push(format!("type={}", network));
                        
                        if network == "tcp" {
                             params.push("headerType=none".to_string());
                             if security == "reality" {
                                 params.push("flow=xtls-rprx-vision".to_string());
                             }
                        }

                        let link = format!("vless://{}@{}:{}?{}#{}", uuid, address, port, params.join("&"), remark);
                        links.push(link);
                    },
                    "hysteria2" => {
                        // hysteria2://user:password@ip:port?sni=...&insecure=1#remark
                        let mut params = Vec::new();
                        let sni = node.reality_sni.clone()
                            .or_else(|| stream.tls_settings.as_ref().map(|t| t.server_name.clone()))
                            .unwrap_or_else(|| node.domain.clone().unwrap_or_default());
                        params.push(format!("sni={}", sni));
                        params.push("insecure=1".to_string());

                        // Check for OBFS in protocol settings
                        if let Ok(InboundType::Hysteria2(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            if let Some(obfs) = settings.obfs {
                                if obfs.ttype == "salamander" {
                                    params.push("obfs=salamander".to_string());
                                    params.push(format!("obfs-password={}", obfs.password));
                                }
                            }
                        }

                        // Fetch User once if needed
                        let user = self.user_repo.get_by_id(sub.user_id).await?
                            .ok_or_else(|| anyhow::anyhow!("User not found"))?;
                        let tg_id = user.tg_id;
                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));

                        let link = format!("hysteria2://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark);
                        links.push(link);
                    },
                    "trojan" => {
                        let mut params = Vec::new();
                        params.push("security=tls".to_string());
                        let sni = node.reality_sni.clone()
                           .or_else(|| stream.tls_settings.as_ref().and_then(|t| if t.server_name.is_empty() { None } else { Some(t.server_name.clone()) }))
                           .unwrap_or(node.domain.clone().unwrap_or_default());
                        params.push(format!("sni={}", sni));
                        params.push("fp=chrome".to_string());
                        params.push(format!("type={}", network));
                        
                        links.push(format!("trojan://{}@{}:{}?{}#{}", uuid, address, port, params.join("&"), remark));
                    },
                    "tuic" => {
                        let mut params = Vec::new();
                        let sni = node.reality_sni.clone()
                           .or_else(|| stream.tls_settings.as_ref().and_then(|t| if t.server_name.is_empty() { None } else { Some(t.server_name.clone()) }))
                           .unwrap_or(node.domain.clone().unwrap_or_default());
                        params.push(format!("sni={}", sni));
                        params.push("alpn=h3".to_string());
                        
                        let congestion = if let Ok(InboundType::Tuic(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            settings.congestion_control
                        } else {
                            "cubic".to_string()
                        };
                        params.push(format!("congestion_control={}", congestion));
                        
                        links.push(format!("tuic://{}:{}@{}:{}?{}#{}", uuid, uuid.replace("-", ""), address, port, params.join("&"), remark));
                    },
                    "naive" => {
                        let mut params = Vec::new();
                        let sni = node.reality_sni.clone()
                            .or_else(|| stream.tls_settings.as_ref().and_then(|t| if t.server_name.is_empty() { None } else { Some(t.server_name.clone()) }))
                            .unwrap_or(node.domain.clone().unwrap_or_default());
                        params.push(format!("sni={}", sni));

                        if security == "reality" {
                             if let Some(reality) = stream.reality_settings {
                                 params.push(format!("pbk={}", node.reality_pub.as_ref().or(reality.public_key.as_ref()).cloned().unwrap_or_default()));
                                 if let Some(sid) = node.short_id.as_ref().or(reality.short_ids.first()) {
                                     params.push(format!("sid={}", sid));
                                 }
                             }
                        }
                        
                        let user = self.user_repo.get_by_id(sub.user_id).await?
                             .ok_or_else(|| anyhow::anyhow!("User not found"))?;
                        let tg_id = user.tg_id;
                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        
                        links.push(format!("naive+https://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark));
                    },
                    _ => {}
                }
            }
        Ok(links)
    }



    pub async fn generate_subscription_file(&self, user_id: i64) -> Result<String> {
        // 1. Get active subscriptions
        let subs = self.get_user_subscriptions(user_id).await?;
        let active_subs: Vec<_> = subs.into_iter().filter(|s| s.sub.status == "active").collect();

        use crate::singbox::client_generator::{ClientGenerator, ClientOutbound, ClientVlessOutbound, ClientHysteria2Outbound, ClientTlsConfig, ClientRealityConfig, ClientObfs, UtlsConfig};
        let mut client_outbounds = Vec::new();

        for sub in active_subs {
            let uuid = sub.sub.vless_uuid.clone().unwrap_or_default();
            
            // 2. Get Inbounds for this Plan
            // Copied from links generation logic
            let inbounds = self.node_repo.get_inbounds_for_plan(sub.sub.plan_id).await?;
            
            for inbound in inbounds {
                use crate::models::network::{StreamSettings};
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or(StreamSettings {
                    network: Some("tcp".to_string()),
                    security: Some("none".to_string()),
                    tls_settings: None,
                    reality_settings: None,
                    ws_settings: None,
                    http_upgrade_settings: None,
                    xhttp_settings: None,
                    packet_encoding: None,
                });
                let security = stream.security.as_deref().unwrap_or("none");
                let _network = stream.network.as_deref().unwrap_or("tcp");

                // Fetch User once if needed (e.g. for hysteria2/amneziawg)
                let user = self.user_repo.get_by_id(sub.sub.user_id).await?
                    .ok_or_else(|| anyhow::anyhow!("User not found"))?;
                let tg_id = user.tg_id;

                let node = self.node_repo.get_node_by_id(inbound.node_id).await?
                    .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
                let address = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    node.ip.clone()
                } else {
                    inbound.listen_ip.clone()
                };

                let port = inbound.listen_port as u16;
                let tag = inbound.tag.clone();

                match inbound.protocol.as_str() {
                    "vless" => {
                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                let names = if let Some(sni) = &node.reality_sni {
                                    vec![sni.clone()]
                                } else if reality.server_names.is_empty() {
                                    vec!["".to_string()]
                                } else {
                                    reality.server_names.clone()
                                };

                                let pub_key = reality.public_key.clone()
                                    .or_else(|| node.reality_pub.clone())
                                    .unwrap_or_default();
                                let short_id = reality.short_ids.first().cloned()
                                    .or_else(|| node.short_id.clone())
                                    .unwrap_or_default();

                                for sni in &names {
                                    let display_tag = if names.len() > 1 {
                                        format!("{} ({})", tag, sni)
                                    } else {
                                        tag.clone()
                                    };

                                    let tls_config = ClientTlsConfig {
                                        enabled: true,
                                        server_name: sni.clone(),
                                        insecure: false,
                                        alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                        utls: Some(crate::singbox::client_generator::UtlsConfig { enabled: true, fingerprint: "chrome".to_string() }),
                                        reality: Some(ClientRealityConfig {
                                            enabled: true,
                                            public_key: pub_key.clone(),
                                            short_id: short_id.clone(),
                                        })
                                    };

                                    client_outbounds.push(ClientOutbound::Vless(ClientVlessOutbound {
                                         tag: display_tag,
                                         server: address.clone(),
                                         server_port: port,
                                         uuid: uuid.clone(),
                                         flow: Some("xtls-rprx-vision".to_string()),
                                         packet_encoding: Some("xudp".to_string()),
                                         tls: Some(tls_config),
                                    }));
                                }
                            }
                        } else {
                             // Standard TLS or None
                             let mut tls_config = None;
                             if security == "tls" {
                                 if let Some(tls) = stream.tls_settings {
                                     tls_config = Some(ClientTlsConfig {
                                         enabled: true,
                                         server_name: tls.server_name,
                                         insecure: false, // Assume true certs
                                         alpn: None,
                                         utls: None,
                                         reality: None,
                                     });
                                 }
                             }

                            client_outbounds.push(ClientOutbound::Vless(ClientVlessOutbound {
                                 tag,
                                 server: address,
                                 server_port: port,
                                 uuid: uuid.clone(),
                                 flow: None,
                                 packet_encoding: Some("xudp".to_string()),
                                 tls: tls_config,
                            }));
                        }
                    },
                    "hysteria2" => {
                        let mut server_name = node.reality_sni.clone().unwrap_or_else(|| node.domain.clone().unwrap_or_default());
                        let mut insecure = true;
                        
                        if let Some(tls) = stream.tls_settings {
                            if !tls.server_name.is_empty() {
                                server_name = tls.server_name.clone();
                            }
                            // If we have a real SNI that isn't a known fake, assume we want real verification if possible
                            if Some(server_name.as_str()) != node.domain.as_deref() && !server_name.contains("google") && !server_name.contains("yahoo") {
                                 insecure = false;
                             }
                         }

                        let tg_id = tg_id; // Just for clarity as we pre-fetched it
                        
                        // Auth is "user:pass"
                        let password = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        
                        let mut obfs = None;
                        if let Ok(InboundType::Hysteria2(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            if let Some(o) = settings.obfs {
                                obfs = Some(ClientObfs {
                                    ttype: o.ttype,
                                    password: o.password,
                                });
                            }
                        }

                        client_outbounds.push(ClientOutbound::Hysteria2(ClientHysteria2Outbound {
                            tag,
                            server: address,
                            server_port: port,
                            password,
                            tls: ClientTlsConfig {
                                enabled: true,
                                server_name,
                                insecure,
                                alpn: Some(vec!["h3".to_string()]),
                                utls: None,
                                reality: None,
                            },
                            obfs,
                        }));
                    },
                    "amneziawg" => {
                        if let Ok(InboundType::AmneziaWg(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            // Stable client key derivation
                            let client_priv = self.derive_awg_key(&uuid);
                            
                            let tg_id = tg_id; // Just for clarity as we pre-fetched it
                                
                            let client_ip = format!("10.10.0.{}", (tg_id % 250) + 2);
                            
                            use crate::singbox::client_generator::ClientAmneziaWgOutbound;
                            client_outbounds.push(ClientOutbound::AmneziaWg(ClientAmneziaWgOutbound {
                                tag,
                                server: address,
                                server_port: port,
                                local_address: vec![format!("{}/32", client_ip)],
                                private_key: client_priv,
                                peer_public_key: self.priv_to_pub(&settings.private_key), // Actually Peer is Server, so we need Server's Public Key. settings.private_key is Server's Private Key.
                                preshared_key: None,
                                jc: settings.jc,
                                jmin: settings.jmin,
                                jmax: settings.jmax,
                                s1: settings.s1,
                                s2: settings.s2,
                                h1: settings.h1,
                                h2: settings.h2,
                                h3: settings.h3,
                                h4: settings.h4,
                            }));
                        }
                    },
                    "trojan" => {
                        use crate::singbox::client_generator::ClientTrojanOutbound;
                        let mut server_name = node.reality_sni.clone().unwrap_or_else(|| node.domain.clone().unwrap_or_default());
                        let mut insecure = true;
                        
                        if let Some(tls) = stream.tls_settings {
                            if !tls.server_name.is_empty() {
                                server_name = tls.server_name.clone();
                            }
                            if Some(server_name.as_str()) != node.domain.as_deref() && !server_name.contains("google") && !server_name.contains("yahoo") {
                                 insecure = false;
                             }
                         }

                        let mut tls_config = Some(ClientTlsConfig {
                            enabled: true,
                            server_name: server_name.clone(),
                            insecure,
                            alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                            utls: Some(UtlsConfig {
                                enabled: true,
                                fingerprint: "chrome".to_string(),
                            }),
                            reality: None,
                        });

                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                let sni = node.reality_sni.clone()
                                    .or_else(|| reality.server_names.first().cloned())
                                    .unwrap_or_default();
                                
                                tls_config = Some(ClientTlsConfig {
                                    enabled: true,
                                    server_name: sni,
                                    insecure: false,
                                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                    utls: Some(UtlsConfig {
                                        enabled: true,
                                        fingerprint: "chrome".to_string(),
                                    }),
                                    reality: Some(ClientRealityConfig {
                                        enabled: true,
                                        public_key: reality.public_key.or(node.reality_pub).unwrap_or_default(),
                                        short_id: reality.short_ids.first().cloned().unwrap_or_default(),
                                    }),
                                });
                            }
                        }

                        client_outbounds.push(ClientOutbound::Trojan(ClientTrojanOutbound {
                            tag,
                            server: address,
                            server_port: port,
                            password: uuid.clone(),
                            tls: tls_config,
                        }));
                    },
                    "tuic" => {
                        use crate::singbox::client_generator::{ClientTuicOutbound};
                        
                        // TUIC settings
                         let mut server_name = node.reality_sni.clone().unwrap_or_else(|| node.domain.clone().unwrap_or_default());
                         if let Some(tls) = stream.tls_settings {
                             if !tls.server_name.is_empty() {
                                 server_name = tls.server_name.clone();
                             }
                         }

                         // Removed unused tg_id query

                        
                         let password = uuid.replace("-", ""); // Using UUID (stripped) as password

                         let congestion = if let Ok(InboundType::Tuic(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                             settings.congestion_control
                         } else {
                             "cubic".to_string()
                         };

                         client_outbounds.push(ClientOutbound::Tuic(ClientTuicOutbound {
                            tag,
                            server: address,
                            server_port: port,
                            uuid: uuid.clone(),
                            password,
                            congestion_control: congestion,
                            tls: ClientTlsConfig {
                                enabled: true,
                                server_name,
                                insecure: false,
                                alpn: Some(vec!["h3".to_string()]),
                                utls: None,
                                reality: None,
                            },
                         }));
                    },
                    "naive" => {
                        use crate::singbox::client_generator::ClientHttpOutbound;
                        
                        let server_name = node.reality_sni.clone()
                            .or_else(|| stream.tls_settings.as_ref().and_then(|t| if t.server_name.is_empty() { None } else { Some(t.server_name.clone()) }))
                            .unwrap_or(node.domain.clone().unwrap_or_default());
                        
                        let password = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        
                        let mut reality_config = None;
                        if security == "reality" {
                             if let Some(reality) = &stream.reality_settings {
                                 reality_config = Some(ClientRealityConfig {
                                     enabled: true,
                                     public_key: node.reality_pub.clone().or(reality.public_key.clone()).unwrap_or_default(),
                                     short_id: node.short_id.clone().or(reality.short_ids.first().cloned()).unwrap_or_default(),
                                 });
                             }
                        }

                        client_outbounds.push(ClientOutbound::Http(ClientHttpOutbound {
                            tag,
                            server: address,
                            server_port: port,
                            username: None,
                            password: Some(password),
                            tls: Some(ClientTlsConfig {
                                enabled: true,
                                server_name,
                                insecure: false,
                                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                                utls: Some(UtlsConfig { enabled: true, fingerprint: "chrome".to_string() }),
                                reality: reality_config,
                            }),
                        }));
                    },
                    _ => {}
                }
            }
        }

        let profile = ClientGenerator::generate(client_outbounds, "ru");
        Ok(serde_json::to_string_pretty(&profile)?)
    }

    pub async fn generate_subscription_links(&self, user_id: i64) -> Result<Vec<String>> {
        let mut links = Vec::new();

        // 1. Get active subscriptions
        let subs = self.get_user_subscriptions(user_id).await?;
        let active_subs: Vec<_> = subs.into_iter().filter(|s| s.sub.status == "active").collect();

        for sub in active_subs {
            let uuid = sub.sub.vless_uuid.clone().unwrap_or_default();
            
            // 2. Get Inbounds for this Plan
            let inbounds = self.node_repo.get_inbounds_for_plan(sub.sub.plan_id).await?;
            
            for inbound in inbounds {
                // Parse stream settings to find SNI/Security
                use crate::models::network::{StreamSettings};
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or(StreamSettings {
                    network: Some("tcp".to_string()),
                    security: Some("none".to_string()),
                    tls_settings: None,
                    reality_settings: None,
                    ws_settings: None,
                    http_upgrade_settings: None,
                    xhttp_settings: None,
                    packet_encoding: None,
                });
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let node = self.node_repo.get_node_by_id(inbound.node_id).await?
                    .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
                let address = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    node.ip.clone()
                } else {
                    inbound.listen_ip.clone()
                };

                let port = inbound.listen_port;
                let remark = inbound.tag.clone();

                match inbound.protocol.as_str() {
                    "vless" => {
                        // vless://uuid@ip:port?security=...&sni=...&fp=...&type=...#remark
                        let mut params = Vec::new();
                        params.push(format!("security={}", security));
                        
                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                let sni = node.reality_sni.clone()
                                    .or_else(|| reality.server_names.first().cloned())
                                    .unwrap_or_default();
                                params.push(format!("sni={}", sni));
                                params.push(format!("pbk={}", reality.public_key.or(node.reality_pub).unwrap_or_default())); 
                                params.push("fp=chrome".to_string());
                            }
                        } else if security == "tls" {
                            if let Some(tls) = stream.tls_settings {
                                params.push(format!("sni={}", tls.server_name));
                            }
                        }
                        
                        params.push(format!("type={}", network));
                        
                        if network == "tcp" {
                             params.push("headerType=none".to_string());
                             if security == "reality" {
                                 params.push("flow=xtls-rprx-vision".to_string());
                             }
                        }

                        let link = format!("vless://{}@{}:{}?{}#{}", uuid, address, port, params.join("&"), remark);
                        links.push(link);
                    },
                    "hysteria2" => {
                        // hysteria2://user:password@ip:port?sni=...&insecure=1#remark
                        let mut params = Vec::new();
                        let mut server_name = node.reality_sni.clone().unwrap_or_else(|| "drive.google.com".to_string());
                        let mut use_insecure = true;

                        if security == "tls" {
                            if let Some(tls) = stream.tls_settings {
                                if !tls.server_name.is_empty() {
                                    server_name = tls.server_name.clone();
                                }
                                match server_name.as_str() {
                                    "drive.google.com" | "www.yahoo.com" => {}, 
                                    _ => { use_insecure = false; }
                                }
                            }
                        }
                        params.push(format!("sni={}", server_name));
                        params.push(format!("insecure={}", if use_insecure { "1" } else { "0" }));

                        // Check for OBFS in protocol settings
                        if let Ok(InboundType::Hysteria2(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            if let Some(obfs) = settings.obfs {
                                if obfs.ttype == "salamander" {
                                    params.push("obfs=salamander".to_string());
                                    params.push(format!("obfs-password={}", obfs.password));
                                }
                            }
                        }

                        // Use TG ID just like in config generation
                        let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?")
                            .bind(sub.sub.user_id)
                            .fetch_optional(&self.pool)
                            .await?
                            .unwrap_or(0);

                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));

                        let link = format!("hysteria2://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark);
                        links.push(link);
                    },
                    "amneziawg" => {
                        if let Ok(InboundType::AmneziaWg(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            let _client_priv = self.derive_awg_key(&uuid);
                            let server_pub = self.priv_to_pub(&settings.private_key);
                            
                            let mut params = Vec::new();
                            params.push(format!("pk={}", server_pub));
                            params.push(format!("jc={}", settings.jc));
                            params.push(format!("jmin={}", settings.jmin));
                            params.push(format!("jmax={}", settings.jmax));
                            params.push(format!("s1={}", settings.s1));
                            params.push(format!("s2={}", settings.s2));
                            params.push(format!("h1={}", settings.h1));
                            params.push(format!("h2={}", settings.h2));
                            params.push(format!("h3={}", settings.h3));
                            params.push(format!("h4={}", settings.h4));

                            let link = format!("awg://{}:{}?{}#{}", address, port, params.join("&"), remark);
                            links.push(link);
                        }
                    },
                    "trojan" => {
                        let mut params = Vec::new();
                        params.push(format!("security={}", security));
                        params.push(format!("type={}", network));
                        
                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                let sni = node.reality_sni.clone()
                                    .or_else(|| reality.server_names.first().cloned())
                                    .unwrap_or_default();
                                params.push(format!("sni={}", sni));
                                params.push(format!("pbk={}", reality.public_key.or(node.reality_pub).unwrap_or_default()));
                                params.push("fp=chrome".to_string());
                            }
                        } else if security == "tls" {
                            if let Some(tls) = stream.tls_settings {
                                params.push(format!("sni={}", tls.server_name));
                            }
                        }

                        let link = format!("trojan://{}@{}:{}?{}#{}", uuid, address, port, params.join("&"), remark);
                        links.push(link);
                    },
                    "tuic" => {
                        // tuic://uuid:password@ip:port?congestion_control=cubic&sni=...&alpn=h3#remark
                        let mut params = Vec::new();
                        let mut server_name = node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string());
                        
                        if let Some(tls) = stream.tls_settings {
                             if !tls.server_name.is_empty() {
                                 server_name = tls.server_name.clone();
                             }
                        }
                        params.push(format!("sni={}", server_name));
                        params.push("alpn=h3".to_string());
                        
                        let congestion = if let Ok(InboundType::Tuic(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                             settings.congestion_control
                        } else {
                             "cubic".to_string()
                        };
                        params.push(format!("congestion_control={}", congestion));
                        
                        let _tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?")
                             .bind(sub.sub.user_id)
                             .fetch_optional(&self.pool)
                             .await?
                             .unwrap_or(0);
                        
                        let password = uuid.replace("-", "");
                        let auth = format!("{}:{}", uuid, password);

                        let link = format!("tuic://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark);
                        links.push(link);
                    },
                    _ => {}
                }
            }
        }
        Ok(links)
    }

    // ==================== DEVICE LIMIT MANAGEMENT ====================

    /// Get the device limit for a specific subscription
    pub async fn get_subscription_device_limit(&self, subscription_id: i64) -> Result<i32> {
        let limit = self.sub_repo.get_device_limit(subscription_id).await?;
        Ok(limit.unwrap_or(0))
    }
    /// Update the list of active IPs for a subscription
    pub async fn update_subscription_ips(&self, subscription_id: i64, ip_list: Vec<String>) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Delete all existing IP records for this subscription
        sqlx::query!("DELETE FROM subscription_ip_tracking WHERE subscription_id = ?", subscription_id)
            .execute(&mut *tx)
            .await?;

        // Insert new IP records
        let now = Utc::now();
        for ip in ip_list {
            sqlx::query!(
                "INSERT INTO subscription_ip_tracking (subscription_id, client_ip, last_seen_at) VALUES (?, ?, ?)",
                subscription_id,
                ip,
                now
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Get active IPs for a subscription (within last 15 minutes)
    pub async fn get_subscription_active_ips(&self, subscription_id: i64) -> Result<Vec<crate::models::store::SubscriptionIpTracking>> {
        let cutoff = Utc::now() - Duration::minutes(15);

        let ips = sqlx::query_as::<_, crate::models::store::SubscriptionIpTracking>(
            "SELECT * FROM subscription_ip_tracking WHERE subscription_id = ? AND last_seen_at > ? ORDER BY last_seen_at DESC"
        )
        .bind(subscription_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active IPs")?;

        Ok(ips)
    }

    /// Clean up old IP tracking records (older than 1 hour)
    pub async fn cleanup_old_ip_tracking(&self) -> Result<u64> {
        let cutoff = Utc::now() - Duration::hours(1);

        let result = sqlx::query("DELETE FROM subscription_ip_tracking WHERE last_seen_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Update user's referral code (alias)
    pub async fn update_user_referral_code(&self, user_id: i64, new_code: &str) -> Result<()> {
        ReferralService::update_user_referral_code(&self.pool, user_id, new_code).await
    }

    /// Set user's referrer by referral code
    pub async fn set_user_referrer(&self, user_id: i64, referrer_code: &str) -> Result<()> {
        let referrer = self.get_user_by_referral_code(referrer_code.trim()).await?
            .context("Referrer not found")?;

        if referrer.id == user_id {
            return Err(anyhow::anyhow!("You cannot refer yourself"));
        }

        // Check if user already has a referrer
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;

        if user.referrer_id.is_some() {
            return Err(anyhow::anyhow!("Referrer is already set and cannot be changed"));
        }

        self.user_repo.set_referrer_id(user_id, referrer.id).await?;

        Ok(())
    }

    /// Delete a plan and refund all active users pro-rated
    pub async fn delete_plan_and_refund(&self, plan_id: i64) -> Result<(i32, i64)> {
        let mut tx = self.pool.begin().await?;
        
        // 1. Get all active subscriptions for this plan
        let active_subs = self.sub_repo.get_active_by_plan(plan_id).await?;

        let mut refunded_users = 0;
        let mut total_refunded_cents = 0;

        for sub in active_subs {
             // Calculate remaining days
             let now = Utc::now();
             if sub.expires_at > now {
                 let remaining_duration = sub.expires_at - now;
                 let remaining_days = remaining_duration.num_days().max(1); // At least 1 day if > 0
                 
                 // Find the price per day for this plan (approximate from base duration)
                 // We try to find the duration that matches the original subscription length
                 // But we don't store original duration in subscription directly easily (only start/end)
                 // So we get average daily price from plan_durations
                 let price_per_day: f64 = sqlx::query_scalar(
                    "SELECT CAST(price AS REAL) / duration_days FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC LIMIT 1"
                 )
                 .bind(plan_id)
                 .fetch_optional(&mut *tx)
                 .await?
                 .unwrap_or(0.0);
                 
                 let refund_amount_cents = (remaining_days as f64 * price_per_day) as i64;
                 
                 if refund_amount_cents > 0 {
                     // Credit User
                     self.user_repo.adjust_balance(sub.user_id, refund_amount_cents).await?;
                        
                     // Log Transaction/Activity
                     // We don't have a transaction log table for "refunds" specifically yet in schema shown, 
                     // but we have activity_log.
                     let _ = crate::services::activity_service::ActivityService::log_tx(&mut *tx, Some(sub.user_id), "Refund", &format!("Plan deleted. Refund for {} days: ${:.2}", remaining_days, refund_amount_cents as f64 / 100.0)).await;
                     
                     total_refunded_cents += refund_amount_cents;
                     refunded_users += 1;
                 }
             }
        }

        // 2. Delete Subscriptions (Active and Inactive)
        self.sub_repo.delete_by_plan_id(plan_id).await?;

        // 3. Delete Plan Durations
        sqlx::query("DELETE FROM plan_durations WHERE plan_id = ?")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;

        // 4. Delete Plan Inbounds binding
        sqlx::query("DELETE FROM plan_inbounds WHERE plan_id = ?")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;
            
        // 5. Delete Plan
        sqlx::query("DELETE FROM plans WHERE id = ?")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        
        Ok((refunded_users, total_refunded_cents))
    }

    // ========== Quick Wins Features ==========
    
    /// Toggle auto-renewal for a subscription
    pub async fn toggle_auto_renewal(&self, subscription_id: i64) -> Result<bool> {
        self.sub_repo.toggle_auto_renewal(subscription_id).await
    }
    
    /// Process all auto-renewals for subscriptions expiring in next 24h
    pub async fn process_auto_renewals(&self) -> Result<Vec<RenewalResult>> {
        let subs = self.sub_repo.get_expiring_auto_renewals().await?;
        
        let mut results = vec![];
        
        for (sub_id, user_id, plan_id, plan_name, balance) in subs {
            let price = sqlx::query_scalar::<_, i64>(
                "SELECT price FROM plan_durations WHERE plan_id = ? ORDER BY duration_days LIMIT 1"
            )
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;
            
            if balance >= price {
                self.sub_repo.extend_expiry_days(sub_id, 30).await?;
                
                self.user_repo.adjust_balance(user_id, -price).await?;
                
                info!("Auto-renewed subscription {} for user {}", sub_id, user_id);
                results.push(RenewalResult::Success { user_id, sub_id, amount: price, plan_name });
            } else {
                results.push(RenewalResult::InsufficientFunds { user_id, sub_id, required: price, available: balance });
            }
        }
        
        Ok(results)
    }
    
    /// Get the free trial plan
    pub async fn get_trial_plan(&self) -> Result<Plan> {
        let mut plan = sqlx::query_as::<_, Plan>(
            "SELECT * FROM plans WHERE COALESCE(is_trial, 0) = 1 AND is_active = 1 LIMIT 1"
        )
        .fetch_one(&self.pool)
        .await
        .context("Trial plan not configured")?;
        
        plan.durations = sqlx::query_as::<_, PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = ?"
        )
        .bind(plan.id)
        .fetch_all(&self.pool)
        .await?;
        
        Ok(plan)
    }
    
    /// Mark user as having used their free trial
    pub async fn mark_trial_used(&self, user_id: i64) -> Result<()> {
        self.user_repo.mark_trial_used(user_id).await?;
        Ok(())
    }
    
    /// Create a trial subscription
    pub async fn create_trial_subscription(&self, user_id: i64, plan_id: i64, duration_days: i64) -> Result<i64> {
        let vless_uuid = Uuid::new_v4().to_string();
        let sub_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days);

        let sub_id = self.sub_repo.create(
            user_id,
            plan_id,
            &vless_uuid,
            &sub_uuid,
            expires_at,
            "active",
            None,
            true
        ).await?;
        
        info!("Created trial subscription {} for user {} ({} days)", sub_id, user_id, duration_days);
        Ok(sub_id)
    }
    
    /// Check and send traffic/expiry alerts (returns list of users who need alerts)
    pub async fn check_traffic_alerts(&self) -> Result<Vec<(i64, AlertType, i64)>> {
        let mut alerts_to_send = vec![];
        
        let subs = self.sub_repo.get_active_with_traffic_limit().await?;
        
        for (sub_id, user_id, used_bytes, traffic_gb, alerts_json) in subs {
            if traffic_gb == 0 { continue; }
            
            let total_bytes = traffic_gb as i64 * 1024 * 1024 * 1024;
            let percentage = (used_bytes as f64 / total_bytes as f64) * 100.0;
            
            let mut alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();
            
            if percentage >= 80.0 && !alerts.contains(&"80_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic80, sub_id));
                alerts.push("80_percent".to_string());
            }
            
            if percentage >= 90.0 && !alerts.contains(&"90_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic90, sub_id));
                alerts.push("90_percent".to_string());
            }
            
            if !alerts.is_empty() {
                let alerts_json = serde_json::to_string(&alerts)?;
                self.sub_repo.update_alerts_sent(sub_id, &alerts_json).await?;
            }
        }
        
        Ok(alerts_to_send)
    }
    /// Add an item to the user's shopping cart
    pub async fn add_to_cart(&self, user_id: i64, product_id: i64, quantity: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO cart_items (user_id, product_id, quantity) 
             VALUES (?, ?, ?) 
             ON CONFLICT(user_id, product_id) 
             DO UPDATE SET quantity = quantity + ?"
        )
        .bind(user_id)
        .bind(product_id)
        .bind(quantity)
        .bind(quantity)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Retrieve all items in a user's cart
    pub async fn get_user_cart(&self, user_id: i64) -> Result<Vec<CartItem>> {
        let items = sqlx::query_as::<_, CartItem>(
            "SELECT c.id, c.user_id, c.product_id, c.quantity, p.name as product_name, p.price 
             FROM cart_items c 
             JOIN products p ON c.product_id = p.id 
             WHERE c.user_id = ?"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(items)
    }

    /// Clear a user's shopping cart
    pub async fn clear_cart(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Checkout cart items (simple balance-based implementation)
    pub async fn checkout_cart(&self, user_id: i64) -> Result<Vec<String>> {
        let cart = self.get_user_cart(user_id).await?;
        if cart.is_empty() {
             return Err(anyhow::anyhow!("Cart is empty"));
        }

        let total_price: i64 = cart.iter().map(|item| item.price * item.quantity).sum();
        
        let mut tx = self.pool.begin().await?;

        // Check balance
        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        if balance < total_price {
            return Err(anyhow::anyhow!("Insufficient balance. Need {}, have {}", total_price, balance));
        }

        // Deduct balance
        self.user_repo.adjust_balance(user_id, total_price).await?;

        // Create order
        let order_id: i64 = sqlx::query_scalar("INSERT INTO orders (user_id, total_amount, status) VALUES (?, ?, 'completed') RETURNING id")
            .bind(user_id)
            .bind(total_price)
            .fetch_one(&mut *tx)
            .await?;

        // Add order items
        for item in cart {
            sqlx::query("INSERT INTO order_items (order_id, product_id, quantity, price) VALUES (?, ?, ?, ?)")
                .bind(order_id)
                .bind(item.product_id)
                .bind(item.quantity)
                .bind(item.price)
                .execute(&mut *tx)
                .await?;
        }

        // Clear cart
        sqlx::query("DELETE FROM cart_items WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        info!("Successfully processed order #{} for user {}", order_id, user_id);
        
        let mut notes = vec![format!("Order #{} placed successfully", order_id)];
        notes.push(format!("Total Amount: ${}.{:02}", total_price / 100, total_price % 100));
        notes.push("Items will be provisioned shortly.".to_string());
        
        Ok(notes)
    }

    /// Derives a stable X25519 private key from a UUID string
    fn derive_awg_key(&self, uuid: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(uuid.as_bytes());
        hasher.update(b"amneziawg-key-salt");
        let result = hasher.finalize();
        
        let mut key = [0u8; 32];
        key.copy_from_slice(&result[..32]);
        
        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;
        
        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, key)
    }

    /// Converts an X25519 private key (base64) to a public key (base64)
    fn priv_to_pub(&self, priv_b64: &str) -> String {
        use x25519_dalek::{StaticSecret, PublicKey};
        
        let priv_bytes = base64::Engine::decode(&base64::prelude::BASE64_STANDARD, priv_b64).unwrap_or_default();
        if priv_bytes.len() != 32 {
            return "".to_string();
        }
        
        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(&priv_bytes);
        
        let secret = StaticSecret::from(key_arr);
        let public = PublicKey::from(&secret);
        
        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, public.as_bytes())
    }
}
