use tracing::{info, error};
use tokio::time::{interval, Duration};
use crate::AppState;
use chrono::Utc;

pub struct MonitoringService {
    state: AppState,
}

impl MonitoringService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn start(&self) {
        info!("Starting background monitoring service...");
        let mut interval = interval(Duration::from_secs(60)); // Every 1 minute
        let mut minute_counter = 0;

        loop {
            interval.tick().await;
            minute_counter += 1;
            
            // Every minute: Check online status
            if let Err(e) = self.check_node_status().await {
                error!("Monitoring error (node status): {}", e);
            }
            if let Err(e) = self.check_frontend_status().await {
                error!("Monitoring error (frontend status): {}", e);
            }

            // Every 5 minutes: Check expirations
            if minute_counter % 5 == 0 {
                if let Err(e) = self.check_expirations().await {
                    error!("Monitoring error (expirations): {}", e);
                }
                if let Err(e) = self.check_traffic().await {
                    error!("Monitoring error (traffic): {}", e);
                }
            }
            
            // Every hour (60 mins): Check and process auto-renewals
            if minute_counter % 60 == 0 {
                if let Err(e) = self.process_auto_renewals().await {
                    error!("Auto-renewal processing error: {}", e);
                }
            }
            
            // Every 6 hours (360 mins): Check for traffic alerts
            if minute_counter % 360 == 0 {
                if let Err(e) = self.check_traffic_alerts().await {
                    error!("Traffic alerts error: {}", e);
                }
                // Reset counter to prevent overflow (though u64 is huge)
                if minute_counter > 10000 {
                    minute_counter = 0;
                }
            }
        }
    }

    async fn check_node_status(&self) -> anyhow::Result<()> {
        // Mark nodes as offline if last_seen > 5 minutes ago
        // Using SQLite datetime modifier
        let rows_affected = sqlx::query("UPDATE nodes SET status = 'offline' WHERE last_seen < datetime('now', '-5 minutes') AND status != 'offline' AND status != 'new'")
            .execute(&self.state.pool)
            .await?
            .rows_affected();

        if rows_affected > 0 {
             info!("Marked {} nodes as offline", rows_affected);
        }
        Ok(())
    }

    async fn check_frontend_status(&self) -> anyhow::Result<()> {
        // Mark frontends as offline if last_heartbeat > 5 minutes ago
        // Verify 'status' column exists via migration first (safe check: just run query, if fails it logs error)
        let rows_affected = sqlx::query("UPDATE frontend_servers SET status = 'offline' WHERE last_heartbeat < datetime('now', '-5 minutes') AND status != 'offline'")
            .execute(&self.state.pool)
            .await?
            .rows_affected();

        if rows_affected > 0 {
             info!("Marked {} frontends as offline", rows_affected);
        }
        Ok(())
    }

    async fn check_expirations(&self) -> anyhow::Result<()> {
        let now = Utc::now();
        
        // Find active subscriptions that have expired
        let expired_subs: Vec<(i64, i64)> = sqlx::query_as("SELECT id, user_id FROM subscriptions WHERE status = 'active' AND expires_at < ?")
            .bind(now)
            .fetch_all(&self.state.pool)
            .await?;

        if expired_subs.is_empty() {
            return Ok(());
        }

        info!("Found {} expired subscriptions. Updating status...", expired_subs.len());

        for (sub_id, user_id) in expired_subs {
            sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE id = ?")
                .bind(sub_id)
                .execute(&self.state.pool)
                .await?;
            
            info!("Subscription {} for user {} marked as expired", sub_id, user_id);
        }

        // Trigger global re-sync to remove expired users from nodes

        Ok(())
    }

    async fn check_traffic(&self) -> anyhow::Result<()> {
        // Placeholder...
        Ok(())
    }

    /// Process auto-renewals for subscriptions expiring in next 24h
    async fn process_auto_renewals(&self) -> anyhow::Result<()> {
        use crate::models::store::RenewalResult;
        
        let results = self.state.store_service.process_auto_renewals().await?;
        
        if results.is_empty() {
            return Ok(());
        }
        
        info!("Processing {} auto-renewal results", results.len());
        
        for result in results {
            match result {
                RenewalResult::Success { user_id, sub_id, amount, plan_name } => {
                    // Get Telegram ID
                    if let Ok(Some(user)) = sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = ?")
                        .bind(user_id)
                        .fetch_optional(&self.state.pool)
                        .await {
                        
                        let msg = format!(
                            "✅ *Auto\\-Renewed\\!*\n\n\
                             💎 Plan: {}\n\
                             💳 Charged: ${:.2}\n\
                             📅 Valid for: 30 days",
                            plan_name.replace("-", "\\-").replace(".", "\\."),
                            amount as f64 / 100.0
                        );
                        
                        let _ = self.state.bot_manager.send_notification(user.0, &msg).await;
                        
                        info!("Auto-renewed subscription {} for user {}, charged ${:.2}", sub_id, user_id, amount as f64 / 100.0);
                    }
                }
                RenewalResult::InsufficientFunds { user_id, sub_id, required, available } => {
                    if let Ok(Some(user)) = sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = ?")
                        .bind(user_id)
                        .fetch_optional(&self.state.pool)
                        .await {
                        
                        let msg = format!(
                            "⚠️ *Auto\\-Renewal Failed*\n\n\
                             💰 Balance: ${:.2}\n\
                             💳 Required: ${:.2}\n\n\
                             Please top up your account to renew your subscription\\.",
                            available as f64 / 100.0,
                            required as f64 / 100.0
                        );
                        
                        let _ = self.state.bot_manager.send_notification(user.0, &msg).await;
                        
                        info!("Auto-renewal failed for sub {} (user {}): insufficient funds", sub_id, user_id);
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Check traffic usage and send alerts at 80%, 90%
    async fn check_traffic_alerts(&self) -> anyhow::Result<()> {
        use crate::models::store::AlertType;
        
        let alerts = self.state.store_service.check_traffic_alerts().await?;
        
        if alerts.is_empty() {
            return Ok(());
        }
        
        info!("Sending {} traffic alerts", alerts.len());
        
        for (user_id, alert_type, _sub_id) in alerts {
            if let Ok(Some(user)) = sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = ?")
                .bind(user_id)
                .fetch_optional(&self.state.pool)
                .await {
                
                let msg = match alert_type {
                    AlertType::Traffic80 => {
                        "⚠️ *Traffic Alert*\n\n\
                         You've used *80%* of your traffic\\.\n\
                         Consider upgrading your plan or topping up\\."
                    }
                    AlertType::Traffic90 => {
                        "⚠️ *Traffic Alert*\n\n\
                         You've used *90%* of your traffic\\.\n\
                         _Service will be paused at 100%\\._"
                    }
                    AlertType::Expiry3Days => {
                        "⏰ *Expiry Alert*\n\n\
                         Your subscription expires in *3 days*\\.\n\
                         Renew now to avoid interruption\\."
                    }
                };
                
                let _ = self.state.bot_manager.send_notification(user.0, msg).await;
            }
        }
        
        Ok(())
    }
}
