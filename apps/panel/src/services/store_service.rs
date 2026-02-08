use sqlx::SqlitePool;
use anyhow::{Context, Result};
use chrono::{Utc, Duration};
use tracing::{info, error};
use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::models::store::{
    User, Plan, Subscription, GiftCode, PlanDuration, 
    RenewalResult, AlertType, DetailedReferral, SubscriptionWithDetails, CartItem
};

#[derive(Debug, Clone)]
pub struct StoreService {
    pool: SqlitePool,
}

impl StoreService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn get_pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    pub async fn get_categories(&self) -> Result<Vec<crate::models::store::Category>> {
        sqlx::query_as::<_, crate::models::store::Category>(
            "SELECT id, name, description, is_active, sort_order, created_at FROM categories WHERE is_active = 1 ORDER BY sort_order ASC"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch categories")
    }

    pub async fn get_next_sni(&self, current_sni: &str, tier: i32) -> Result<String> {
        // 1. Exclude current SNI
        // 2. Filter by tier (try same tier first)
        // 3. Order by health_score DESC
        // 4. Return top result
        
        let sni: Option<String> = sqlx::query_scalar(
            "SELECT domain FROM sni_pool 
             WHERE domain != ? AND tier <= ? AND is_active = 1
             ORDER BY health_score DESC, tier ASC
             LIMIT 1"
        )
        .bind(current_sni)
        .bind(tier)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get next SNI")?;
        
        Ok(sni.unwrap_or_else(|| "www.google.com".to_string()))
    }

    pub async fn log_sni_rotation(
        &self, 
        node_id: i64, 
        old_sni: &str, 
        new_sni: &str, 
        reason: &str
    ) -> Result<i64> {
        let res = sqlx::query(
            "INSERT INTO sni_rotation_log (node_id, old_sni, new_sni, reason)
             VALUES (?, ?, ?, ?)"
        )
        .bind(node_id)
        .bind(old_sni)
        .bind(new_sni)
        .bind(reason)
        .execute(&self.pool)
        .await
        .context("Failed to log SNI rotation")?;

        Ok(res.last_insert_rowid())
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
        sqlx::query_as::<_, crate::models::node::Node>(
            "SELECT * FROM nodes WHERE status = 'active'"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active nodes")
    }

    pub async fn get_user_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE tg_id = ?"
        )
        .bind(tg_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch user by TG ID")
    }

    pub async fn get_user_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE referral_code = ?"
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch user by referral code")
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
        // First check if user exists to avoid overwriting referrer_id if it's already set
        let existing = self.get_user_by_tg_id(tg_id).await?;
        
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
            RETURNING id, tg_id, username, full_name, balance, referral_code, referrer_id, is_banned, language_code, terms_accepted_at, warning_count, trial_used, trial_used_at, last_bot_msg_id, created_at
            "#
        )
        .bind(tg_id)
        .bind(username)
        .bind(full_name)
        .bind(tg_id.to_string()) // Default referral code is just the TG ID
        .bind(final_referrer_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to upsert user")?;

        // Analytics Hooks
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

    pub async fn ban_user(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET is_banned = 1 WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_user_language(&self, user_id: i64, lang: &str) -> Result<()> {
        sqlx::query("UPDATE users SET language_code = ? WHERE id = ?")
            .bind(lang)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        // Legacy: kept for compatibility but superseded by history tracking
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
        sqlx::query("UPDATE users SET terms_accepted_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
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
        let vless_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, expires_at, status)
            VALUES (?, ?, ?, ?, 'pending')
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
            "#
        )
        .bind(user_id)
        .bind(duration.plan_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        // Analytics
        let _ = crate::services::analytics_service::AnalyticsService::track_order(&self.pool).await;

        Ok(sub)
    }

    pub async fn activate_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ? AND user_id = ?")
            .bind(sub_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Subscription is not pending"));
        }

        // Calculate duration from original intent (expires_at - created_at)
        let duration = sub.expires_at - sub.created_at;
        let new_expires_at = Utc::now() + duration;

        let updated_sub = sqlx::query_as::<_, Subscription>(
            r#"
            UPDATE subscriptions 
            SET expires_at = ?, status = 'active' 
            WHERE id = ? 
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
            "#
        )
        .bind(new_expires_at)
        .bind(sub_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(updated_sub)
    }

    pub async fn convert_subscription_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        // 1. Fetch Subscription
        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ? AND user_id = ?")
            .bind(sub_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .context("Subscription not found")?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Only pending subscriptions can be converted to gifts"));
        }

        // 2. Calculate duration
        // For pending subs, duration is stored in (expires_at - created_at)
        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        // 3. Delete Subscription
        sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

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
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
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
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
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
        sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete subscription")?;
        Ok(())
    }

    pub async fn admin_refund_subscription(&self, sub_id: i64, amount: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // 1. Get Sub to find user
        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .fetch_one(&mut *tx)
            .await
            .context("Subscription not found")?;

        // 2. Delete Sub
        sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

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
        sqlx::query("UPDATE subscriptions SET expires_at = datetime(expires_at, '+' || ? || ' days') WHERE id = ?")
            .bind(days)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to extend subscription")?;
        Ok(())
    }

    pub async fn admin_gift_subscription(&self, user_id: i64, plan_id: i64, duration_days: i32) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        // 1. Select an active node
        // Simple strategy: pick the first available active node. 
        let node_id: i64 = sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active' LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active nodes available to assign"))?;

        // 2. Prepare subscription data
        let vless_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        // 3. Create Active Subscription
        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, created_at)
            VALUES (?, ?, ?, ?, ?, 'active', CURRENT_TIMESTAMP)
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
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
        Ok(sub)
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
        let existing_sub = sqlx::query_as::<_, Subscription>(
            "SELECT id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note FROM subscriptions WHERE user_id = ? AND plan_id = ? AND status = 'active'"
        )
        .bind(user_id)
        .bind(duration.plan_id)
        .fetch_optional(&mut *tx)
        .await?;

        let sub = if let Some(active_sub) = existing_sub {
            // Extend existing
            let new_expires_at = if active_sub.expires_at > Utc::now() {
                active_sub.expires_at + Duration::days(duration.duration_days as i64)
            } else {
                Utc::now() + Duration::days(duration.duration_days as i64)
            };

            let updated_sub = sqlx::query_as::<_, Subscription>(
                r#"
                UPDATE subscriptions 
                SET expires_at = ? 
                WHERE id = ? 
                RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
                "#
            )
            .bind(new_expires_at)
            .bind(active_sub.id)
            .fetch_one(&mut *tx)
            .await?;
            
            updated_sub
        } else {
            // Create new if none found (fallback)
            let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
            let vless_uuid = Uuid::new_v4().to_string();

            sqlx::query_as::<_, Subscription>(
                r#"
                INSERT INTO subscriptions (user_id, plan_id, vless_uuid, expires_at, status)
                VALUES (?, ?, ?, ?, 'active')
                RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note
                "#
            )
            .bind(user_id)
            .bind(duration.plan_id)
            .bind(vless_uuid)
            .bind(expires_at)
            .fetch_one(&mut *tx)
            .await?
        };

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn get_user_subscriptions(&self, user_id: i64) -> Result<Vec<SubscriptionWithDetails>> {
        // 1. Fetch Subscriptions
        let subs = sqlx::query_as::<_, Subscription>(
            "SELECT id, user_id, plan_id, node_id, vless_uuid, expires_at, status, created_at, used_traffic, traffic_updated_at, note FROM subscriptions WHERE user_id = ? ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions")?;

        if subs.is_empty() {
            return Ok(Vec::new());
        }

        // 2. Fetch Plans with Durations (cached or fresh)
        let plans = self.get_active_plans().await?; 

        let mut result = Vec::new();

        for sub in subs {
            let plan = plans.iter().find(|p| p.id == sub.plan_id);
            
            let (name, desc, limit) = if let Some(p) = plan {
                // Find duration with closest days to (expires_at - created_at)
                let actual_days = (sub.expires_at - sub.created_at).num_days();
                
                // Find PlanDuration with closest duration_days
                let _best_dur = p.durations.iter().min_by_key(|d| (d.duration_days as i64 - actual_days).abs());
                
                let limit = Some(p.traffic_limit_gb);
                
                (p.name.clone(), p.description.clone(), limit)
            } else {
                ("Unknown Plan".to_string(), None, None)
            };

            result.push(SubscriptionWithDetails {
                sub,
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
        // 10% bonus for the referrer
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut **pool)
            .await?;
        
        if let Some(referrer_id) = user.referrer_id {
            let bonus = amount_cents / 10;
            if bonus > 0 {
                // 1. Update balance
                sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
                    .bind(bonus)
                    .bind(referrer_id)
                    .execute(&mut **pool)
                    .await?;
                
                // 2. Log to referral_bonuses
                sqlx::query("INSERT INTO referral_bonuses (referrer_id, referred_id, amount, payment_id) VALUES (?, ?, ?, ?)")
                    .bind(referrer_id)
                    .bind(user_id)
                    .bind(bonus)
                    .bind(payment_id)
                    .execute(&mut **pool)
                    .await?;

                info!("Applied referral bonus of {} to user {} (from user {})", bonus, referrer_id, user_id);

                // Fetch referrer tg_id for notification
                let referrer_tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?")
                    .bind(referrer_id)
                    .fetch_optional(&mut **pool)
                    .await?;
                
                if let Some(tg_id) = referrer_tg_id {
                    return Ok(Some((tg_id, bonus)));
                }
            }
        }
        Ok(None)
    }

    pub async fn get_user_referrals(&self, referrer_id: i64) -> Result<Vec<DetailedReferral>> {
        sqlx::query_as::<_, DetailedReferral>(
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
                COALESCE(CAST(SUM(rb.bonus_value) AS INTEGER), 0) as total_earned
            FROM users u
            LEFT JOIN referral_bonuses rb ON u.id = rb.referred_user_id AND rb.user_id = ?
            WHERE u.referrer_id = ?
            GROUP BY u.id
            ORDER BY u.created_at DESC
            "#
        )
        .bind(referrer_id)
        .bind(referrer_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch detailed referrals")
    }

    pub async fn get_user_referral_earnings(&self, referrer_id: i64) -> Result<i64> {
        let total: Option<i64> = sqlx::query_scalar("SELECT CAST(SUM(bonus_value) AS INTEGER) FROM referral_bonuses WHERE user_id = ?")
            .bind(referrer_id)
            .fetch_one(&self.pool)
            .await
            .ok()
            .flatten();
        Ok(total.unwrap_or(0))
    }

    pub async fn get_referral_count(&self, user_id: i64) -> Result<i64> {
        let count = sqlx::query_scalar!("SELECT COUNT(*) FROM users WHERE referrer_id = ?", user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count as i64)
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
        sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
            .bind(product.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

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
        let sub: Option<Subscription> = sqlx::query_as("SELECT * FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .fetch_optional(&self.pool)
            .await?;
        
        if let Some(sub) = sub {
            let uuid = sub.vless_uuid.clone().unwrap_or_default();
            
            // 2. Get Inbounds for this Plan
            let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(
                r#"
                SELECT i.* FROM inbounds i
                JOIN plan_inbounds pi ON pi.inbound_id = i.id
                WHERE pi.plan_id = ? AND i.enable = 1
                "#
            )
            .bind(sub.plan_id)
            .fetch_all(&self.pool)
            .await?;
            
            for inbound in inbounds {
                // Parse stream settings to find SNI/Security
                use crate::models::network::{StreamSettings};
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or(StreamSettings {
                    network: Some("tcp".to_string()),
                    security: Some("none".to_string()),
                    tls_settings: None,
                    reality_settings: None,
                });
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let (address, reality_pub, short_id) = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    // We need the Node's public IP and Reality Key
                    let node_details: Option<(String, Option<String>, Option<String>)> = sqlx::query_as("SELECT ip, reality_pub, short_id FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                    
                    if let Some((ip, pub_key, sid)) = node_details {
                        (ip, pub_key, sid)
                    } else {
                        (inbound.listen_ip.clone(), None, None)
                    }
                } else {
                    // If specifically binding to an IP, we might still need the key if it's on the same node
                     let node_details: Option<(Option<String>, Option<String>)> = sqlx::query_as("SELECT reality_pub, short_id FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                     
                     if let Some((pub_key, sid)) = node_details {
                         (inbound.listen_ip.clone(), pub_key, sid)
                     } else {
                         (inbound.listen_ip.clone(), None, None)
                     }
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
                                params.push(format!("sni={}", reality.server_names.first().cloned().unwrap_or_default()));
                                params.push(format!("pbk={}", reality_pub.unwrap_or_default())); 
                                if let Some(sid) = &short_id {
                                    params.push(format!("sid={}", sid));
                                }
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
                     if let Some(tls) = stream.tls_settings {
                        params.push(format!("sni={}", tls.server_name));
                    }
                    params.push("insecure=1".to_string()); // Self-signed usually

                    // Check for OBFS in protocol settings
                    use crate::models::network::InboundType;
                    if let Ok(InboundType::Hysteria2(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                        if let Some(obfs) = settings.obfs {
                            if obfs.ttype == "salamander" {
                                params.push("obfs=salamander".to_string());
                                params.push(format!("obfs-password={}", obfs.password));
                            }
                        }
                    }

                    // Fetch TG ID for auth name
                    // We need to fetch it inside the loop or pre-fetch it. Pre-fetching is better but we are inside a loop over inbounds which is inside...
                    // Actually sub.user_id is available. Ideally we fetch tg_id once.
                    // Let's do a quick scalar query here (caching would be better but this is fine for now)
                    let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?")
                        .bind(sub.user_id)
                        .fetch_optional(&self.pool)
                        .await?
                        .unwrap_or(0); // Fallback to 0 if not found (shouldn't happen)

                    let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));

                    let link = format!("hysteria2://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark);
                    links.push(link);
                },
                    _ => {}
                }
            }
        }
        Ok(links)
    }



    pub async fn generate_subscription_file(&self, user_id: i64) -> Result<String> {
        // 1. Get active subscriptions
        let subs = self.get_user_subscriptions(user_id).await?;
        let active_subs: Vec<_> = subs.into_iter().filter(|s| s.sub.status == "active").collect();

        use crate::singbox::client_generator::{ClientGenerator, ClientOutbound, ClientVlessOutbound, ClientHysteria2Outbound, ClientTlsConfig, ClientRealityConfig, ClientObfs};
        let mut client_outbounds = Vec::new();

        for sub in active_subs {
            let uuid = sub.sub.vless_uuid.clone().unwrap_or_default();
            
            // 2. Get Inbounds for this Plan
            // Copied from links generation logic
            let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(
                r#"
                SELECT i.* FROM inbounds i
                JOIN plan_inbounds pi ON pi.inbound_id = i.id
                WHERE pi.plan_id = ? AND i.enable = 1
                "#
            )
            .bind(sub.sub.plan_id)
            .fetch_all(&self.pool)
            .await?;
            
            for inbound in inbounds {
                use crate::models::network::{StreamSettings, InboundType};
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or(StreamSettings {
                    network: Some("tcp".to_string()),
                    security: Some("none".to_string()),
                    tls_settings: None,
                    reality_settings: None,
                });
                let security = stream.security.as_deref().unwrap_or("none");
                let _network = stream.network.as_deref().unwrap_or("tcp");

                let (address, reality_pub) = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    let node_details: Option<(String, Option<String>)> = sqlx::query_as("SELECT ip, reality_pub FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                    if let Some((ip, pub_key)) = node_details { (ip, pub_key) } else { (inbound.listen_ip.clone(), None) }
                } else {
                     let pub_key: Option<String> = sqlx::query_scalar("SELECT reality_pub FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                    (inbound.listen_ip.clone(), pub_key)
                };

                let port = inbound.listen_port as u16;
                let tag = inbound.tag.clone();

                match inbound.protocol.as_str() {
                    "vless" => {
                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                let names = if reality.server_names.is_empty() {
                                    vec!["".to_string()]
                                } else {
                                    reality.server_names.clone()
                                };

                                for (_idx, sni) in names.iter().enumerate() {
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
                                            public_key: reality_pub.clone().unwrap_or_default(),
                                            short_id: reality.short_ids.first().cloned().unwrap_or_default(),
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
                        let mut server_name = "drive.google.com".to_string();
                        let mut insecure = true;
                        
                        if let Some(tls) = stream.tls_settings {
                            server_name = tls.server_name.clone();
                            if server_name != "drive.google.com" && server_name != "www.yahoo.com" {
                                insecure = false;
                            }
                        }

                        let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?")
                            .bind(sub.sub.user_id)
                            .fetch_optional(&self.pool)
                            .await?
                            .unwrap_or(0);
                        
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
                        // Parse AmneziaWG settings to get obfuscation params
                        if let Ok(InboundType::AmneziaWg(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            // Generate client keypair
                            let (client_priv, _client_pub) = {
                                use std::process::Command;
                                let output = Command::new("sing-box")
                                    .args(&["generate", "wireguard-keypair"])
                                    .output();
                                
                                if let Ok(output) = output {
                                    if output.status.success() {
                                        let output_str = String::from_utf8_lossy(&output.stdout);
                                        let mut priv_key = String::new();
                                        let mut pub_key = String::new();
                                        
                                        for line in output_str.lines() {
                                            if let Some(key) = line.strip_prefix("PrivateKey:") {
                                                priv_key = key.trim().to_string();
                                            } else if let Some(key) = line.strip_prefix("PublicKey:") {
                                                pub_key = key.trim().to_string();
                                            }
                                        }
                                        (priv_key, pub_key)
                                    } else {
                                        continue;
                                    }
                                } else {
                                    continue;
                                }
                            };

                            let client_ip = format!("10.10.0.{}/32", 2 + client_outbounds.len());
                            
                            use crate::singbox::client_generator::ClientAmneziaWgOutbound;
                            client_outbounds.push(ClientOutbound::AmneziaWg(ClientAmneziaWgOutbound {
                                tag,
                                server: address,
                                server_port: port,
                                local_address: vec![client_ip],
                                private_key: client_priv,
                                peer_public_key: settings.private_key,
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
            let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(
                r#"
                SELECT i.* FROM inbounds i
                JOIN plan_inbounds pi ON pi.inbound_id = i.id
                WHERE pi.plan_id = ? AND i.enable = 1
                "#
            )
            .bind(sub.sub.plan_id)
            .fetch_all(&self.pool)
            .await?;
            
            for inbound in inbounds {
                // Parse stream settings to find SNI/Security
                use crate::models::network::{StreamSettings};
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or(StreamSettings {
                    network: Some("tcp".to_string()),
                    security: Some("none".to_string()),
                    tls_settings: None,
                    reality_settings: None,
                });
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let (address, reality_pub) = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    // We need the Node's public IP and Reality Key
                    let node_details: Option<(String, Option<String>)> = sqlx::query_as("SELECT ip, reality_pub FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                    
                    if let Some((ip, pub_key)) = node_details {
                        (ip, pub_key)
                    } else {
                        (inbound.listen_ip.clone(), None)
                    }
                } else {
                    // If specifically binding to an IP, we might still need the key if it's on the same node
                     let pub_key: Option<String> = sqlx::query_scalar("SELECT reality_pub FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                    (inbound.listen_ip.clone(), pub_key)
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
                                params.push(format!("sni={}", reality.server_names.first().cloned().unwrap_or_default()));
                                params.push(format!("pbk={}", reality_pub.unwrap_or_default())); 
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
                        if security == "tls" {
                            let mut use_insecure = true;
                            if let Some(tls) = stream.tls_settings {
                                match tls.server_name.as_str() {
                                    "drive.google.com" | "www.yahoo.com" => {}, // Do nothing, keep insecure=true
                                    _ => { use_insecure = false; }
                                }
                                params.push(format!("sni={}", tls.server_name));
                            }
                            params.push(format!("insecure={}", if use_insecure { "1" } else { "0" }));
                        } else {
                            // No TLS? Hysteria2 usually implies TLS/QUIC.
                            params.push("insecure=1".to_string());
                        }

                        // Check for OBFS in protocol settings
                        use crate::models::network::InboundType;
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
                    _ => {}
                }
            }
        }
        Ok(links)
    }

    // ==================== DEVICE LIMIT MANAGEMENT ====================

    /// Get the device limit for a specific subscription
    pub async fn get_subscription_device_limit(&self, subscription_id: i64) -> Result<i32> {
        let limit: Option<i32> = sqlx::query_scalar(
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = ?"
        )
            .bind(subscription_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to fetch device limit for subscription")?;

        let limit = limit.unwrap_or(0); // Handle nulls if any

        Ok(limit)
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
        let clean_code = new_code.trim();
        if clean_code.is_empty() {
            return Err(anyhow::anyhow!("Referral code cannot be empty"));
        }

        sqlx::query("UPDATE users SET referral_code = ? WHERE id = ?")
            .bind(clean_code)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to update referral code. It might already be taken.")?;

        Ok(())
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

        sqlx::query("UPDATE users SET referrer_id = ? WHERE id = ?")
            .bind(referrer.id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Delete a plan and refund all active users pro-rated
    pub async fn delete_plan_and_refund(&self, plan_id: i64) -> Result<(i32, i64)> {
        let mut tx = self.pool.begin().await?;
        
        // 1. Get all active subscriptions for this plan
        let active_subs = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE plan_id = ? AND status = 'active'"
        )
        .bind(plan_id)
        .fetch_all(&mut *tx)
        .await?;

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
                     sqlx::query("UPDATE users SET balance = balance + ? WHERE id = ?")
                        .bind(refund_amount_cents)
                        .bind(sub.user_id)
                        .execute(&mut *tx)
                        .await?;
                        
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
        sqlx::query("DELETE FROM subscriptions WHERE plan_id = ?")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;

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
        let current: bool = sqlx::query_scalar::<_, Option<i32>>("SELECT auto_renew FROM subscriptions WHERE id = ?")
            .bind(subscription_id)
            .fetch_one(&self.pool)
            .await?
            .map(|v| v != 0)
            .unwrap_or(false);
        
        let new_value = !current;
        
        sqlx::query("UPDATE subscriptions SET auto_renew = ? WHERE id = ?")
            .bind(new_value as i32)
            .bind(subscription_id)
            .execute(&self.pool)
            .await?;
        
        Ok(new_value)
    }
    
    /// Process all auto-renewals for subscriptions expiring in next 24h
    pub async fn process_auto_renewals(&self) -> Result<Vec<RenewalResult>> {
        let subs = sqlx::query_as::<_, (i64, i64, i64, String, i64)>(
            "SELECT s.id, s.user_id, s.plan_id, p.name, u.balance 
             FROM subscriptions s
             JOIN users u ON s.user_id = u.id
             JOIN plans p ON s.plan_id = p.id
             WHERE COALESCE(s.auto_renew, 0) = 1
             AND s.status = 'active'
             AND datetime(s.expires_at) BETWEEN datetime('now') AND datetime('now', '+1 day')"
        )
        .fetch_all(&self.pool)
        .await?;
        
        let mut results = vec![];
        
        for (sub_id, user_id, plan_id, plan_name, balance) in subs {
            let price = sqlx::query_scalar::<_, i64>(
                "SELECT price FROM plan_durations WHERE plan_id = ? ORDER BY duration_days LIMIT 1"
            )
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;
            
            if balance >= price {
                sqlx::query("UPDATE subscriptions SET expires_at = datetime(expires_at, '+30 days') WHERE id = ?")
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
                
                sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
                    .bind(price)
                    .bind(user_id)
                    .execute(&self.pool)
                    .await?;
                
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
        sqlx::query("UPDATE users SET trial_used = 1, trial_used_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    /// Create a trial subscription
    pub async fn create_trial_subscription(&self, user_id: i64, plan_id: i64, duration_days: i64) -> Result<i64> {
        let sub_id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions 
             (user_id, plan_id, status, expires_at, used_traffic, is_trial, created_at) 
             VALUES (?, ?, 'active', datetime('now', '+' || ? || ' days'), 0, 1, CURRENT_TIMESTAMP) 
             RETURNING id"
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(duration_days)
        .fetch_one(&self.pool)
        .await?;
        
        info!("Created trial subscription {} for user {} ({} days)", sub_id, user_id, duration_days);
        Ok(sub_id)
    }
    
    /// Check and send traffic/expiry alerts (returns list of users who need alerts)
    pub async fn check_traffic_alerts(&self) -> Result<Vec<(i64, AlertType, i64)>> {
        let mut alerts_to_send = vec![];
        
        let subs = sqlx::query_as::<_, (i64, i64, i64, i64, String)>(
            "SELECT s.id, s.user_id, s.used_traffic, pd.traffic_gb, COALESCE(s.alerts_sent, '[]') 
             FROM subscriptions s
             JOIN plan_durations pd ON s.plan_id = pd.plan_id
             WHERE s.status = 'active' AND pd.traffic_gb > 0"
        )
        .fetch_all(&self.pool)
        .await?;
        
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
                sqlx::query("UPDATE subscriptions SET alerts_sent = ? WHERE id = ?")
                    .bind(&alerts_json)
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
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
        sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
            .bind(total_price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

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
}
