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

        // Уведомляем по планам, а не по subscriptions.node_id: node_id бывает
        // NULL, а подписка может обслуживаться несколькими нодами.
        let mut plans_to_regen: HashSet<i64> = HashSet::new();

        if !expired.is_empty() {
            info!(
                "Found {} paid subscriptions exceeding quota. Suspending...",
                expired.len()
            );

            for row in expired {
                plans_to_regen.insert(row.plan_id);

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

                // Same fallback as the time-based expiry sweep: put the user
                // back on the free plan so they keep a way in to renew.
                match self
                    .state
                    .store_service
                    .ensure_free_plan_subscription(row.user_id)
                    .await
                {
                    // Regenerate the free plan's configs too — the plan id goes
                    // into the same set the paid plans use below.
                    Ok(Some(free_plan_id)) => {
                        plans_to_regen.insert(free_plan_id);
                    }
                    Ok(None) => {}
                    Err(e) => error!(
                        "Failed to restore the free plan for user {} after quota expiry: {}",
                        row.user_id, e
                    ),
                }
            }
        }

        // --- Бесплатные планы: переводим в 'throttled' и обрываем соединения ---
        // 'throttled' исключает подписку из конфигов нод (config generation
        // отбирает только 'active'); суточное пополнение (daily_traffic_topup)
        // вернёт статус 'active' и снова разошлёт конфиги.
        let throttled = self
            .state
            .subscription_service
            .throttle_free_quota_subscriptions()
            .await?;

        if !throttled.is_empty() {
            info!(
                "Throttled {} free-plan subscriptions exceeding daily quota",
                throttled.len()
            );

            for row in throttled {
                plans_to_regen.insert(row.plan_id);

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

        if !plans_to_regen.is_empty() {
            let plan_ids: Vec<i64> = plans_to_regen.into_iter().collect();
            if let Err(e) = self
                .state
                .orchestration_service
                .notify_nodes_for_plans(&plan_ids)
                .await
            {
                error!(
                    "Failed to trigger config refresh after quota enforcement for plans {:?}: {}",
                    plan_ids, e
                );
            }
        }

        // Agents also pull config periodically, but explicit publish reduces stale window.

        Ok(())
    }
}
