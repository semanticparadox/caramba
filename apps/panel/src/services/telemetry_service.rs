use sqlx::MySqlPool;
use std::sync::Arc;
use tracing::{info, warn};

use crate::services::store_service::StoreService;
use crate::services::notification_service::NotificationService;
use crate::bot_manager::BotManager;

#[derive(Clone)]
pub struct TelemetryService {
    pool: MySqlPool,
    store_service: Arc<StoreService>,
    notification_service: Arc<NotificationService>,
    bot_manager: Arc<BotManager>,
}

impl TelemetryService {
    pub fn new(
        pool: MySqlPool,
        store_service: Arc<StoreService>,
        notification_service: Arc<NotificationService>,
        bot_manager: Arc<BotManager>,
    ) -> Self {
        Self { 
            pool, 
            store_service, 
            notification_service,
            bot_manager 
        }
    }

    pub async fn process_heartbeat(
        &self,
        node_id: i64,
        active_connections: Option<u32>,
        traffic_up: u64,
        traffic_down: u64,
        _speed_mbps: Option<i32>,
    ) -> Result<(), sqlx::Error> {
        // 1. Store the metrics (if we had a time-series DB, but for now just log or check anomalies)
        // We might want to store 'active_connections' in the nodes table for real-time dashboard
        
        if let Some(conns) = active_connections {
             // Update node status
             sqlx::query("UPDATE nodes SET active_connections = ? WHERE id = ?")
                 .bind(conns)
                 .bind(node_id)
                 .execute(&self.pool)
                 .await?;

             // 2. Anomaly Detection (Simple Heuristic for "Connection Freezing")
             if conns > 50 && (traffic_up + traffic_down) < 1024 {
                 warn!("⚠️ Potential Censorship Detected on Node {}: {} connections but only {} bytes traffic.", 
                     node_id, conns, traffic_up + traffic_down);
                 
                 // Trigger Self-Healing
                 let _ = self.trigger_mitigation(node_id).await;
             }
        }

        Ok(())
    }

    async fn trigger_mitigation(&self, node_id: i64) -> Result<(), sqlx::Error> {
        // 1. Rotate SNI
        info!("🔧 Triggering SNI Rotation for Node {} due to detected censorship.", node_id);
        
        match self.store_service.rotate_node_sni(node_id, "Auto-Heal: Connection Freezing").await {
            Ok((old_sni, new_sni, rotation_id)) => {
                 info!("✅ Auto-Healed Node {}: {} -> {}", node_id, old_sni, new_sni);
                 
                 // 2. Notify Users
                 if let Some(bot) = self.bot_manager.get_bot().await.ok() {
                     let notify_svc = self.notification_service.clone();
                     let old = old_sni.clone();
                     let new = new_sni.clone();
                     tokio::spawn(async move {
                         let _ = notify_svc.notify_sni_rotation(&bot, node_id, &old, &new, rotation_id).await;
                     });
                 }
            },
            Err(e) => {
                // If no other SNI, we can't do much. Log error.
                // It's not a fatal error for the heartbeat itself
                warn!("❌ Failed to auto-heal node {}: {}", node_id, e);
            }
        }
        
        Ok(())
    }

}
