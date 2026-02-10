use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::{Subscription, SubscriptionWithDetails};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct SubscriptionRepository {
    pool: SqlitePool,
}

impl SubscriptionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by ID")
    }
    
    pub async fn get_by_uuid(&self, uuid: &str) -> Result<Option<Subscription>> {
         sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE subscription_uuid = ?")
            .bind(uuid)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by UUID")
    }

    pub async fn get_active_by_user(&self, user_id: i64) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE user_id = ? AND status = 'active' ORDER BY expires_at DESC LIMIT 1"
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch active subscription for user")
    }

    pub async fn get_active_by_plan(&self, plan_id: i64) -> Result<Vec<Subscription>> {
        sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE plan_id = ? AND status = 'active'"
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active subscriptions for plan")
    }
    
    pub async fn get_all_by_user(&self, user_id: i64) -> Result<Vec<SubscriptionWithDetails>> {
        sqlx::query_as::<_, SubscriptionWithDetails>(
            "SELECT s.*, p.name as plan_name, n.name as node_name 
             FROM subscriptions s 
             JOIN plans p ON s.plan_id = p.id
             LEFT JOIN nodes n ON s.node_id = n.id
             WHERE s.user_id = ?
             ORDER BY s.created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions")
    }

    pub async fn get_active_plan_id_by_user(&self, user_id: i64) -> Result<Option<i64>> {
        sqlx::query_scalar("SELECT plan_id FROM subscriptions WHERE user_id = ? AND status = 'active' LIMIT 1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch active plan ID")
    }

    pub async fn create(
        &self, 
        user_id: i64, 
        plan_id: i64, 
        vless_uuid: &str, 
        sub_uuid: &str,
        expires_at: DateTime<Utc>,
        status: &str,
        note: Option<&str>,
        is_trial: bool
    ) -> Result<i64> {
        let id = sqlx::query_scalar(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, note, created_at, is_trial)
            VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?)
            RETURNING id
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(expires_at)
        .bind(status)
        .bind(note)
        .bind(is_trial)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create subscription")?;
        
        Ok(id)
    }

    pub async fn update_expiry(&self, id: i64, new_expiry: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = ?, status = 'active' WHERE id = ?")
            .bind(new_expiry)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn extend_expiry_days(&self, id: i64, days: i64) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = datetime(expires_at, '+' || ? || ' days') WHERE id = ?")
            .bind(days)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn expire_family_subs(&self, parent_id: i64) -> Result<()> {
         // This is a bit specific logic "WHERE note='Family'..." but okay for now or generic?
         // Let's make it specific as it matches the query found
         // Actually the query matches by user_id
         sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE user_id = ? AND note = 'Family' AND status = 'active'")
            .bind(parent_id)
            .execute(&self.pool)
            .await?;
         Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_by_plan_id(&self, plan_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE plan_id = ?")
            .bind(plan_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status_and_expiry(&self, id: i64, status: &str, expires_at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET status = ?, expires_at = ? WHERE id = ?")
            .bind(status)
            .bind(expires_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_auto_renew(&self, id: i64) -> Result<bool> {
        let current: bool = sqlx::query_scalar::<_, Option<i32>>("SELECT auto_renew FROM subscriptions WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?
            .map(|v| v != 0)
            .unwrap_or(false);
        
        let new_value = !current;
        
        sqlx::query("UPDATE subscriptions SET auto_renew = ? WHERE id = ?")
            .bind(new_value as i32)
            .bind(id)
            .execute(&self.pool)
            .await?;
            
        Ok(new_value)
    }

    pub async fn toggle_auto_renewal(&self, id: i64) -> Result<bool> {
        self.toggle_auto_renew(id).await
    }

    pub async fn update_alerts_sent(&self, id: i64, alerts_json: &str) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET alerts_sent = ? WHERE id = ?")
            .bind(alerts_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn get_expiring_auto_renewals(&self) -> Result<Vec<(i64, i64, i64, String, i64)>> {
        // Returns (sub_id, user_id, plan_id, plan_name, user_balance)
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
        Ok(subs)
    }
    
    pub async fn get_active_with_traffic_limit(&self) -> Result<Vec<(i64, i64, i64, i64, String)>> {
        // Returns (sub_id, user_id, used_bytes, traffic_gb, alerts_json)
        // This query joins plan_durations, implying "Plan" domain knowledge. 
        // Ideally this joint object should be defined in models.
        let subs = sqlx::query_as::<_, (i64, i64, i64, i64, String)>(
            "SELECT s.id, s.user_id, s.used_traffic, pd.traffic_gb, COALESCE(s.alerts_sent, '[]') 
             FROM subscriptions s
             JOIN plan_durations pd ON s.plan_id = pd.plan_id
             WHERE s.status = 'active' AND pd.traffic_gb > 0"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(subs)
    }
    
    pub async fn get_device_limit(&self, sub_id: i64) -> Result<Option<i32>> {
        sqlx::query_scalar(
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = ?"
        )
        .bind(sub_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get device limit")
    }
    
    // For Family logic
    pub async fn update_family_sub(&self, id: i64, expires_at: DateTime<Utc>, plan_id: i64, node_id: Option<i64>) -> Result<()> {
         // matches the update query in sync_family_subscriptions
         sqlx::query("UPDATE subscriptions SET expires_at = ?, plan_id = ?, node_id = ?, status = 'active', note = 'Family' WHERE id = ?")
            .bind(expires_at)
            .bind(plan_id)
            .bind(node_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
         Ok(())
    }
    
    // Create Simplified (e.g. for trial or family where some fields are default)
    // Actually the generic create above is fine, just maybe make args Option?
    // Let's stick to explicit args for now.
}
