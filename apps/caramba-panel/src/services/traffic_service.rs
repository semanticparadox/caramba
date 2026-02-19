use tracing::{info, error};
use tokio::time::{interval, Duration};
use crate::AppState;
use chrono::Utc;
use crate::services::analytics_service::AnalyticsService;

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

        // Fetch only IDs to stay compatible across schema variants (INT4/INT8 column drift).
        let active_node_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active'")
            .fetch_all(&self.state.pool)
            .await?;

        for node_id in active_node_ids {
            // Note: Per-user traffic usage is now reported via node heartbeats 
            // and processed in api/v2/node.rs. Aggregate node stats could be 
            // fetched here in the future if needed.
            info!("Node {} traffic sync handled via heartbeat reporting", node_id);
        }

        // After syncing, enforce quotas
        self.enforce_quotas().await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn process_node_usage(&self, _node_id: i64, usage: serde_json::Value) -> anyhow::Result<()> {
        if let Some(users_usage) = usage.get("users") {
            if let Some(users_map) = users_usage.as_object() {
                let mut tx = self.state.pool.begin().await?;
                for (user_tag, bytes_val) in users_map {
                    if let Some(bytes) = bytes_val.as_u64() {
                        if user_tag.starts_with("user_") {
                            if let Ok(sub_id) = user_tag[5..].parse::<i64>() {
                                let sub_details = sqlx::query_as::<_, (i64, String, i64)>(
                                    "UPDATE subscriptions SET used_traffic = used_traffic + $1, traffic_updated_at = $2 WHERE id = $3 RETURNING user_id, COALESCE(note, ''), plan_id"
                                )
                                .bind(bytes as i64)
                                .bind(Utc::now())
                                .bind(sub_id)
                                .fetch_optional(&mut *tx)
                                .await?;
                                
                                // Simple analytics
                                let _ = AnalyticsService::track_traffic(&self.state.pool, bytes as i64).await;

                                // Family Plan Logic: Trickle up to parent
                                if let Some((user_id, note, plan_id)) = sub_details {
                                    if note == "Family" {
                                        // Find parent
                                        let parent_id: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM users WHERE id = $1")
                                            .bind(user_id)
                                            .fetch_optional(&mut *tx)
                                            .await?;
                                        
                                        if let Some(pid) = parent_id {
                                            // Update parent's active subscription of same plan
                                            sqlx::query("UPDATE subscriptions SET used_traffic = used_traffic + $1 WHERE user_id = $2 AND plan_id = $3 AND status = 'active'")
                                                .bind(bytes as i64)
                                                .bind(pid)
                                                .bind(plan_id)
                                                .execute(&mut *tx)
                                                .await?;
                                        }
                                    }
                                }
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
            sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE id = $1")
                .bind(sub_id)
                .execute(&self.state.pool)
                .await?;
            
            info!("Subscription {} for user {} suspended (Limit: {} GB reached)", sub_id, user_id, limit);
        }

        // Agents pull config automatically - no sync needed

        Ok(())
    }
}
