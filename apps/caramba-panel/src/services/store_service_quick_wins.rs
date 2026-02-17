// Quick Wins Features: Auto-Renewal, Traffic Alerts, Free Trial
// Added to: StoreService

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenewalResult {
    Success { user_id: i64, sub_id: i64, amount: i64, plan_name: String },
    InsufficientFunds { user_id: i64, sub_id: i64, required: i64, available: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    Traffic80,
    Traffic90,
    Expiry3Days,
}

impl StoreService {
    /// Toggle auto-renewal for a subscription
    pub async fn toggle_auto_renewal(&self, subscription_id: i64) -> Result<bool> {
        let current = sqlx::query_scalar::<_, Option<bool>>("SELECT auto_renew FROM subscriptions WHERE id = ?")
            .bind(subscription_id)
            .fetch_one(&self.pool)
            .await?
            .unwrap_or(Some(false));
        
        let new_value = !current.unwrap_or(false);
        
        sqlx::query("UPDATE subscriptions SET auto_renew = ? WHERE id = ?")
            .bind(new_value)
            .bind(subscription_id)
            .execute(&self.pool)
            .await?;
        
        Ok(new_value)
    }
    
    /// Process all auto-renewals for subscriptions expiring in next 24h
    pub async fn process_auto_renewals(&self) -> Result<Vec<RenewalResult>> {
        // Get subscriptions expiring in next 24h with auto_renew enabled
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
            // Get plan price (use first duration)
            let price = sqlx::query_scalar::<_, i64>(
                "SELECT price FROM plan_durations WHERE plan_id = ? ORDER BY duration_days LIMIT 1"
            )
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;
            
            if balance >= price {
                // Extend subscription by 30 days
                sqlx::query("UPDATE subscriptions SET expires_at = datetime(expires_at, '+30 days') WHERE id = ?")
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
                
                // Deduct balance
                sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
                    .bind(price)
                    .bind(user_id)
                    .execute(&self.pool)
                    .await?;
                
                info!("Auto-renewed subscription {} for user {}, charged {}", sub_id, user_id, price);
                
                results.push(RenewalResult::Success { 
                    user_id, 
                    sub_id, 
                    amount: price,
                    plan_name 
                });
            } else {
                results.push(RenewalResult::InsufficientFunds { 
                    user_id, 
                    sub_id, 
                    required: price, 
                    available: balance 
                });
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
        
        // Load durations
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
    
    /// Create a subscription (including trial subscriptions)
    pub async fn create_trial_subscription(&self, user_id: i64, plan_id: i64) -> Result<i64> {
        // Get plan duration
        let duration = sqlx::query_as::<_, PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = ? LIMIT 1"
        )
        .bind(plan_id)
        .fetch_one(&self.pool)
        .await?;
        
        // Create subscription
        let sub_id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions 
             (user_id, plan_id, status, expires_at, used_traffic, is_trial, created_at) 
             VALUES (?, ?, 'active', datetime('now', '+' || ? || ' days'), 0, 1, CURRENT_TIMESTAMP) 
             RETURNING id"
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(duration.duration_days)
        .fetch_one(&self.pool)
        .await?;
        
        info!("Created trial subscription {} for user {}", sub_id, user_id);
        
        Ok(sub_id)
    }
    
    /// Check and send traffic/expiry alerts
    pub async fn check_and_send_alerts(&self) -> Result<Vec<(i64, AlertType)>> {
        let mut alerts_to_send = vec![];
        
        // Traffic alerts (80%, 90%)
        let subs = sqlx::query_as::<_, (i64, i64, i64, String)>(
            "SELECT s.id, s.user_id, s.used_traffic, COALESCE(s.alerts_sent, '[]') 
             FROM subscriptions s
             JOIN plan_durations pd ON s.plan_id = pd.plan_id
             WHERE s.status = 'active' AND pd.traffic_gb > 0"
        )
        .fetch_all(&self.pool)
        .await?;
        
        for (sub_id, user_id, used_traffic_bytes, alerts_json) in subs {
            // Get traffic limit from plan
            let traffic_limit_gb: i32 = sqlx::query_scalar(
                "SELECT pd.traffic_gb FROM plan_durations pd
                 JOIN subscriptions s ON s.plan_id = pd.plan_id
                 WHERE s.id = ? LIMIT 1"
            )
            .bind(sub_id)
            .fetch_one(&self.pool)
            .await?;
            
            if traffic_limit_gb == 0 { continue; }
            
            let total_traffic_bytes = traffic_limit_gb as i64 * 1024 * 1024 * 1024;
            let percentage = (used_traffic_bytes as f64 / total_traffic_bytes as f64) * 100.0;
            
            let mut alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();
            
            // Check 80% threshold
            if percentage >= 80.0 && !alerts.contains(&"80_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic80));
                alerts.push("80_percent".to_string());
            }
            
            // Check 90% threshold
            if percentage >= 90.0 && !alerts.contains(&"90_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic90));
                alerts.push("90_percent".to_string());
            }
            
            // Update alerts_sent
            if !alerts.is_empty() {
                let alerts_json = serde_json::to_string(&alerts)?;
                sqlx::query("UPDATE subscriptions SET alerts_sent = ? WHERE id = ?")
                    .bind(&alerts_json)
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
            }
        }
        
        // Expiry alerts (3 days before)
        let expiring_subs = sqlx::query_as::<_, (i64, String)>(
            "SELECT s.user_id, COALESCE(s.alerts_sent, '[]')
             FROM subscriptions s
             WHERE s.status = 'active'
             AND datetime(s.expires_at) BETWEEN datetime('now', '+2 days') AND datetime('now', '+3 days')"
        )
        .fetch_all(&self.pool)
        .await?;
        
        for (user_id, alerts_json) in expiring_subs {
            let alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();
            if !alerts.contains(&"expiry_3d".to_string()) {
                alerts_to_send.push((user_id, AlertType::Expiry3Days));
            }
        }
        
        Ok(alerts_to_send)
    }
}
