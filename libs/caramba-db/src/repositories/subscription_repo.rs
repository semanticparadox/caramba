use sqlx::PgPool;
use anyhow::{Context, Result};
use crate::models::store::{Subscription, SubscriptionWithDetails};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct SubscriptionRepository {
    pool: PgPool,
}

impl SubscriptionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by ID")
    }
    
    pub async fn get_by_uuid(&self, uuid: &str) -> Result<Option<Subscription>> {
         sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE subscription_uuid = $1")
            .bind(uuid)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by UUID")
    }

    pub async fn get_active_by_user(&self, user_id: i64) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE user_id = $1 AND status = 'active' ORDER BY expires_at DESC LIMIT 1"
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch active subscription for user")
    }

    pub async fn get_active_by_plan(&self, plan_id: i64) -> Result<Vec<Subscription>> {
        sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE plan_id = $1 AND status = 'active'"
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active subscriptions for plan")
    }
    
    pub async fn get_all_by_user(&self, user_id: i64) -> Result<Vec<SubscriptionWithDetails>> {
        sqlx::query_as::<_, SubscriptionWithDetails>(
            "SELECT s.*, p.name as plan_name, p.description as plan_description, p.traffic_limit_gb 
             FROM subscriptions s 
             JOIN plans p ON s.plan_id = p.id
             LEFT JOIN nodes n ON s.node_id = n.id
             WHERE s.user_id = $1
             ORDER BY s.created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions")
    }

    pub async fn get_active_plan_id_by_user(&self, user_id: i64) -> Result<Option<i64>> {
        sqlx::query_scalar("SELECT plan_id FROM subscriptions WHERE user_id = $1 AND status = 'active' LIMIT 1")
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
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, note, created_at, is_trial)
            VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP, $8)
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
        sqlx::query("UPDATE subscriptions SET expires_at = $1, status = 'active' WHERE id = $2")
            .bind(new_expiry)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn extend_expiry_days(&self, id: i64, days: i64) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = expires_at + ($1 * interval '1 day') WHERE id = $2")
            .bind(days)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn expire_family_subs(&self, parent_id: i64) -> Result<()> {
         sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE user_id = $1 AND note = 'Family' AND status = 'active'")
            .bind(parent_id)
            .execute(&self.pool)
            .await?;
         Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_by_plan_id(&self, plan_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE plan_id = $1")
            .bind(plan_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status_and_expiry(&self, id: i64, status: &str, expires_at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET status = $1, expires_at = $2 WHERE id = $3")
            .bind(status)
            .bind(expires_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_auto_renew(&self, id: i64) -> Result<bool> {
        let current: bool = sqlx::query_scalar::<_, Option<i32>>("SELECT auto_renew FROM subscriptions WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?
            .map(|v| v != 0)
            .unwrap_or(false);
        
        let new_value = !current;
        
        sqlx::query("UPDATE subscriptions SET auto_renew = $1 WHERE id = $2")
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
        sqlx::query("UPDATE subscriptions SET alerts_sent = $1 WHERE id = $2")
            .bind(alerts_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    pub async fn get_expiring_auto_renewals(&self) -> Result<Vec<(i64, i64, i64, String, i64)>> {
        let subs = sqlx::query_as::<_, (i64, i64, i64, String, i64)>(
            "SELECT s.id, s.user_id, s.plan_id, p.name, u.balance 
             FROM subscriptions s
             JOIN users u ON s.user_id = u.id
             JOIN plans p ON s.plan_id = p.id
             WHERE COALESCE(s.auto_renew, 0) = 1
             AND s.status = 'active'
             AND s.expires_at BETWEEN CURRENT_TIMESTAMP AND CURRENT_TIMESTAMP + interval '1 day'"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(subs)
    }
    
    pub async fn get_active_with_traffic_limit(&self) -> Result<Vec<(i64, i64, i64, i64, String)>> {
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
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = $1"
        )
        .bind(sub_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get device limit")
    }
    
    pub async fn update_family_sub(&self, id: i64, expires_at: DateTime<Utc>, plan_id: i64, node_id: Option<i64>) -> Result<()> {
         sqlx::query("UPDATE subscriptions SET expires_at = $1, plan_id = $2, node_id = $3, status = 'active', note = 'Family' WHERE id = $4")
            .bind(expires_at)
            .bind(plan_id)
            .bind(node_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
         Ok(())
    }
    
    pub async fn get_trial_plan(&self) -> Result<Option<crate::models::store::Plan>> {
        let mut plan = sqlx::query_as::<_, crate::models::store::Plan>(
            "SELECT * FROM plans WHERE COALESCE(is_trial, 0) = 1 AND is_active = TRUE LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(ref mut p) = plan {
            p.durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = $1 ORDER BY duration_days ASC"
            )
            .bind(p.id)
            .fetch_all(&self.pool)
            .await?;
        }
        Ok(plan)
    }

    pub async fn get_active_subs_by_plans(&self, plan_ids: &[i64]) -> Result<Vec<(i64, Option<String>, i64, Option<String>)>> {
        if plan_ids.is_empty() {
            return Ok(Vec::new());
        }
        
        sqlx::query_as::<_, (i64, Option<String>, i64, Option<String>)>(
            r#"
            SELECT s.id, s.vless_uuid, u.tg_id, u.username
            FROM subscriptions s
            JOIN users u ON s.user_id = u.id
            WHERE LOWER(s.status) = 'active' AND s.plan_id = ANY($1)
            "#
        )
        .bind(plan_ids)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active subs by plans")
    }

    pub async fn update_ips(&self, sub_id: i64, ips: Vec<String>) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM subscription_ip_tracking WHERE subscription_id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        for ip in ips {
            sqlx::query("INSERT INTO subscription_ip_tracking (subscription_id, client_ip, last_seen_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT DO NOTHING")
                .bind(sub_id)
                .bind(ip)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
