use tracing::{info, error};
use tokio::time::{interval, Duration};
use crate::AppState;
use chrono::Utc;
use crate::models::node::Node;

pub struct TrafficService {
    state: AppState,
}

impl TrafficService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn start(&self) {
        info!("Starting background traffic monitoring service...");
        let mut interval = interval(Duration::from_secs(600)); // Every 10 minutes

        loop {
            interval.tick().await;
            if let Err(e) = self.sync_traffic().await {
                error!("Traffic monitoring error: {}", e);
            }
        }
    }

    async fn sync_traffic(&self) -> anyhow::Result<()> {
        info!("Syncing traffic usage from all active nodes...");
        
        let active_nodes: Vec<Node> = sqlx::query_as("SELECT * FROM nodes WHERE status = 'active'")
            .fetch_all(&self.state.pool)
            .await?;

        for node in active_nodes {
            match self.state.orchestration_service.get_node_usage(node.id).await {
                Ok(usage) => {
                    if let Err(e) = self.process_node_usage(node.id, usage).await {
                        error!("Failed to process usage for node {}: {}", node.ip, e);
                    }
                },
                Err(e) => {
                    error!("Failed to fetch usage for node {}: {}", node.ip, e);
                }
            }
        }

        // After syncing, enforce quotas
        self.enforce_quotas().await?;

        Ok(())
    }

    async fn process_node_usage(&self, _node_id: i64, usage: serde_json::Value) -> anyhow::Result<()> {
        if let Some(users_usage) = usage.get("users") {
            if let Some(users_map) = users_usage.as_object() {
                let mut tx = self.state.pool.begin().await?;
                for (user_tag, bytes_val) in users_map {
                    if let Some(bytes) = bytes_val.as_u64() {
                        if user_tag.starts_with("user_") {
                            if let Ok(sub_id) = user_tag[5..].parse::<i64>() {
                                sqlx::query("UPDATE subscriptions SET used_traffic = used_traffic + ?, traffic_updated_at = ? WHERE id = ?")
                                    .bind(bytes as i64)
                                    .bind(Utc::now())
                                    .bind(sub_id)
                                    .execute(&mut *tx)
                                    .await?;
                            }
                        }
                    }
                }
                tx.commit().await?;
            }
        }
        
        Ok(())
    }

    async fn enforce_quotas(&self) -> anyhow::Result<()> {
        // Find subscriptions that exceeded their plan's traffic limit
        let overloaded_subs: Vec<(i64, i64, i32)> = sqlx::query_as(
            "SELECT s.id, s.user_id, p.traffic_limit_gb 
             FROM subscriptions s
             JOIN plans p ON s.plan_id = p.id
             WHERE s.status = 'active' 
             AND p.traffic_limit_gb > 0 
             AND s.used_traffic >= (CAST(p.traffic_limit_gb AS BIGINT) * 1024 * 1024 * 1024)"
        )
        .fetch_all(&self.state.pool)
        .await?;

        if overloaded_subs.is_empty() {
            return Ok(());
        }

        info!("Found {} subscriptions exceeding quota. Suspending...", overloaded_subs.len());

        for (sub_id, user_id, limit) in overloaded_subs {
            sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE id = ?")
                .bind(sub_id)
                .execute(&self.state.pool)
                .await?;
            
            info!("Subscription {} for user {} suspended (Limit: {} GB reached)", sub_id, user_id, limit);
        }

        // Trigger global re-sync to remove suspended users from configurations
        let orch = self.state.orchestration_service.clone();
        tokio::spawn(async move {
            let _ = orch.sync_all_nodes().await;
        });

        Ok(())
    }
}
