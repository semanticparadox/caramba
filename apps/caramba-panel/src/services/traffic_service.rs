use crate::AppState;
use crate::services::analytics_service::AnalyticsService;
use chrono::Utc;
use std::collections::HashSet;
use tokio::time::{Duration, interval};
use tracing::{error, info};

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
        let active_node_ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active'")
                .fetch_all(&self.state.pool)
                .await?;

        for node_id in active_node_ids {
            // Note: Per-user traffic usage is now reported via node heartbeats
            // and processed in api/v2/node.rs. Aggregate node stats could be
            // fetched here in the future if needed.
            info!(
                "Node {} traffic sync handled via heartbeat reporting",
                node_id
            );
        }

        // After syncing, enforce quotas
        self.enforce_quotas().await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn process_node_usage(
        &self,
        _node_id: i64,
        usage: serde_json::Value,
    ) -> anyhow::Result<()> {
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
                                let _ =
                                    AnalyticsService::track_traffic(&self.state.pool, bytes as i64)
                                        .await;

                                // Family Plan Logic: Trickle up to parent
                                if let Some((user_id, note, plan_id)) = sub_details {
                                    if note == "Family" {
                                        // Find parent
                                        let parent_id: Option<i64> = sqlx::query_scalar(
                                            "SELECT parent_id FROM users WHERE id = $1",
                                        )
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
        let expired = self
            .state
            .subscription_service
            .expire_over_quota_subscriptions()
            .await?;

        if expired.is_empty() {
            return Ok(());
        }

        info!(
            "Found {} subscriptions exceeding quota. Suspending...",
            expired.len()
        );

        let mut nodes_to_notify = HashSet::new();
        for row in expired {
            if let Some(node_id) = row.node_id {
                nodes_to_notify.insert(node_id);
            }

            info!(
                "Subscription {} for user {} suspended (traffic quota reached)",
                row.subscription_id, row.user_id
            );

            if let Err(e) = self
                .state
                .connection_service
                .kill_subscription_connections(row.subscription_id)
                .await
            {
                error!(
                    "Failed to reset active sessions for expired subscription {}: {}",
                    row.subscription_id, e
                );
            }
        }

        for node_id in nodes_to_notify {
            if let Err(e) = self
                .state
                .orchestration_service
                .notify_node_update(node_id)
                .await
            {
                error!(
                    "Failed to trigger config refresh after quota enforcement for node {}: {}",
                    node_id, e
                );
            }
        }

        // Agents also pull config periodically, but explicit publish reduces stale window.

        Ok(())
    }
}
