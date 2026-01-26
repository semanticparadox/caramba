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
        let mut interval = interval(Duration::from_secs(300)); // Every 5 minutes

        loop {
            interval.tick().await;
            if let Err(e) = self.check_expirations().await {
                error!("Monitoring error (expirations): {}", e);
            }
            if let Err(e) = self.check_traffic().await {
                error!("Monitoring error (traffic): {}", e);
            }
        }
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
        let orch = self.state.orchestration_service.clone();
        tokio::spawn(async move {
            // Agents pull config automatically - no sync needed
        });

        Ok(())
    }

    async fn check_traffic(&self) -> anyhow::Result<()> {
        // Placeholder for traffic monitoring. 
        // In reality, this would query Sing-box API for each node.
        // For Phase 8, we've planned to query Sing-box API.
        // Since we don't have a Sing-box client yet, we'll mark this as a future improvement
        // or implement a basic version that reads from a mock/sqlite if we had traffic counters.
        Ok(())
    }
}
