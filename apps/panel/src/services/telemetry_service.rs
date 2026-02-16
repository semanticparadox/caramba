use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{info, warn};

use crate::services::security_service::SecurityService;
use crate::services::notification_service::NotificationService;
use crate::bot_manager::BotManager;

#[derive(Clone)]
pub struct TelemetryService {
    pool: SqlitePool,
    security_service: Arc<SecurityService>,
    notification_service: Arc<NotificationService>,
    bot_manager: Arc<BotManager>,
}

impl TelemetryService {
    pub fn new(
        pool: SqlitePool,
        security_service: Arc<SecurityService>,
        notification_service: Arc<NotificationService>,
        bot_manager: Arc<BotManager>,
    ) -> Self {
        Self { 
            pool, 
            security_service, 
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
        speed_mbps: Option<i32>,
        discovered_snis: Option<Vec<exarobot_shared::DiscoveredSni>>,
        uptime: u64,
    ) -> Result<(), sqlx::Error> {
        // 1. Fetch current totals and previous session counters
        let node_data: Option<(i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT total_ingress, total_egress, last_session_ingress, last_session_egress FROM nodes WHERE id = ?"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((mut total_in, mut total_eq, last_sess_in, last_sess_eg)) = node_data {
            // Logic: traffic_up/down are CUMULATIVE for the agent session
            // If they are smaller than last seen, the agent restarted.
            
            let diff_in = if traffic_up >= last_sess_in as u64 {
                traffic_up - last_sess_in as u64
            } else {
                traffic_up // Restarted
            };

            let diff_eg = if traffic_down >= last_sess_eg as u64 {
                traffic_down - last_sess_eg as u64
            } else {
                traffic_down // Restarted
            };

            total_in += diff_in as i64;
            total_eq += diff_eg as i64;

            // Simple heuristic for max users: Speed / 8 Mbps per user (rounded)
            let calculated_max = if let Some(s) = speed_mbps {
                if s > 0 { Some(s / 8) } else { None }
            } else {
                None
            };

            // Update node
            sqlx::query(
                "UPDATE nodes SET 
                    active_connections = ?, 
                    total_ingress = ?, 
                    total_egress = ?, 
                    last_session_ingress = ?, 
                    last_session_egress = ?,
                    uptime = ?,
                    current_speed_mbps = COALESCE(?, current_speed_mbps),
                    max_users = COALESCE(?, max_users)
                 WHERE id = ?"
            )
            .bind(active_connections)
            .bind(total_in)
            .bind(total_eq)
            .bind(traffic_up as i64)
            .bind(traffic_down as i64)
            .bind(uptime as i64)
            .bind(speed_mbps)
            .bind(calculated_max)
            .bind(node_id)
            .execute(&self.pool)
            .await?;

            // 2. Anomaly Detection (Simple Heuristic for "Connection Freezing")
            if let Some(conns) = active_connections {
                 if conns > 50 && (diff_in + diff_eg) < 1024 {
                     warn!("⚠️ Potential Censorship Detected on Node {}: {} connections but only {} bytes traffic.", 
                         node_id, conns, diff_in + diff_eg);
                     
                     // Trigger Self-Healing
                     let _ = self.trigger_mitigation(node_id).await;
                 }
            }
        }
        
        // 3. Process Discovered SNIs (Phase 7/57 - Neighbor Sniper)
        if let Some(snis) = discovered_snis {
            for sni in snis {
                // Phase 57: Filter out "junk" SNIs
                let domain = sni.domain.to_lowercase();
                if domain.split('.').count() > 4 || domain.contains("traefik") || domain.contains("localhost") || domain.len() > 50 {
                    continue;
                }

                // Check Blacklist
                let is_blacklisted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sni_blacklist WHERE domain = ?)")
                    .bind(&domain)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(false);
                
                if is_blacklisted {
                    continue;
                }

                // Insert into sni_pool table if doesn't exist, tagging as discovered by specific node
                let _ = sqlx::query("INSERT INTO sni_pool (domain, tier, notes, is_active, discovered_by_node_id, health_score) VALUES (?, 1, ?, 1, ?, 100) ON CONFLICT(domain) DO UPDATE SET notes = EXCLUDED.notes")
                    .bind(&domain)
                    .bind(format!("Discovered by Node {} (Sniper)", node_id))
                    .bind(node_id)
                    .execute(&self.pool)
                    .await;
                
                info!("💎 Neighbor Sniper: Persisted discovered SNI {} from Node {}", domain, node_id);

                // --- NEW: Semi-Auto Assignment to Node ---
                // If the node currently has NO reality_sni, or it's a generic one like google.com, 
                // and we just discovered a fresh local one, update the node immediately.
                // This fulfills the user request: "SNI же должен ставится после нахождения сканером на нашем хостинге рядом"
                let node_sni: Option<String> = sqlx::query_scalar("SELECT reality_sni FROM nodes WHERE id = ?")
                    .bind(node_id)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(None);

                let is_generic = node_sni.as_deref().map(|s| s == "www.google.com" || s == "google.com" || s == "www.microsoft.com").unwrap_or(true);
                
                if is_generic {
                    let _ = sqlx::query("UPDATE nodes SET reality_sni = ? WHERE id = ?")
                        .bind(&domain)
                        .bind(node_id)
                        .execute(&self.pool)
                        .await;
                    info!("✨ Neighbor Sniper: Automatically assigned discovered SNI {} to Node {}", domain, node_id);
                }
            }
        }

        Ok(())
    }

    async fn trigger_mitigation(&self, node_id: i64) -> Result<(), sqlx::Error> {
        // 1. Rotate SNI
        info!("🔧 Triggering SNI Rotation for Node {} due to detected censorship.", node_id);
        
        match self.security_service.rotate_node_sni(node_id, "Auto-Heal: Connection Freezing").await {
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
