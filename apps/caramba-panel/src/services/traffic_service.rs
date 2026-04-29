use crate::AppState;
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

    // NOTE: `process_node_usage` was deleted in this commit. It interpreted
    // `user_{N}` keys as subscription IDs, while the live heartbeat path in
    // `api/v2/node.rs::heartbeat` correctly interprets them as Telegram IDs
    // (with `unnest()` bulk-update resolving tg_id → user_id → subscription).
    // Re-wiring the old function would silently write traffic to the wrong
    // subscriptions.

    async fn enforce_quotas(&self) -> anyhow::Result<()> {
        // --- Платные планы: истекаем и уведомляем ноды ---
        let expired = self
            .state
            .subscription_service
            .expire_over_quota_subscriptions()
            .await?;

        let mut nodes_to_notify = HashSet::new();

        if !expired.is_empty() {
            info!(
                "Found {} paid subscriptions exceeding quota. Suspending...",
                expired.len()
            );

            for row in expired {
                if let Some(node_id) = row.node_id {
                    nodes_to_notify.insert(node_id);
                }

                info!(
                    "Subscription {} for user {} expired (traffic quota reached)",
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
        }

        // --- Бесплатные планы: статус не меняем, но обрываем соединения ---
        // Трафик восстановится при суточном пополнении (daily_traffic_topup).
        let throttled = self
            .state
            .subscription_service
            .throttled_free_quota_subscriptions()
            .await?;

        if !throttled.is_empty() {
            info!(
                "Found {} free-plan subscriptions exceeding daily quota. Throttling connections...",
                throttled.len()
            );

            for row in throttled {
                if let Err(e) = self
                    .state
                    .connection_service
                    .kill_subscription_connections(row.subscription_id)
                    .await
                {
                    error!(
                        "Failed to kill connections for throttled free subscription {}: {}",
                        row.subscription_id, e
                    );
                }
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
