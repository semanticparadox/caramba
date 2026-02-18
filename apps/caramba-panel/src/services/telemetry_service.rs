use sqlx::PgPool;
use std::sync::Arc;
use tracing::{info, warn};
use anyhow::Result;

use crate::services::security_service::SecurityService;
use crate::services::notification_service::NotificationService;
use crate::bot_manager::BotManager;

#[derive(Clone)]
pub struct TelemetryService {
    pool: PgPool,
    security_service: Arc<SecurityService>,
    notification_service: Arc<NotificationService>,
    bot_manager: Arc<BotManager>,
}

impl TelemetryService {
    pub fn new(
        pool: PgPool,
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
        discovered_snis: Option<Vec<caramba_shared::DiscoveredSni>>,
        uptime: u64,
    ) -> Result<()> {
        let node_data: Option<(i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT total_ingress, total_egress, last_session_ingress, last_session_egress FROM nodes WHERE id = $1"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((mut total_in, mut total_eq, last_sess_in, last_sess_eg)) = node_data {
            let diff_in = if traffic_up >= last_sess_in as u64 {
                traffic_up - last_sess_in as u64
            } else {
                traffic_up 
            };

            let diff_eg = if traffic_down >= last_sess_eg as u64 {
                traffic_down - last_sess_eg as u64
            } else {
                traffic_down 
            };

            total_in += diff_in as i64;
            total_eq += diff_eg as i64;

            let node_load: Option<(Option<f64>, Option<f64>, Option<i32>)> = sqlx::query_as(
                "SELECT last_cpu, last_ram, max_users FROM nodes WHERE id = $1"
            )
            .bind(node_id)
            .fetch_optional(&self.pool)
            .await?;

            let calculated_max = node_load.and_then(|(cpu, ram, prev_max)| {
                derive_recommended_max_users(speed_mbps, cpu, ram, prev_max)
            });

            sqlx::query(
                "UPDATE nodes SET 
                    active_connections = $1, 
                    total_ingress = $2, 
                    total_egress = $3, 
                    last_session_ingress = $4, 
                    last_session_egress = $5,
                    uptime = $6,
                    current_speed_mbps = COALESCE($7, current_speed_mbps),
                    max_users = COALESCE($8, max_users)
                 WHERE id = $9"
            )
            .bind(active_connections.map(|c| c as i32))
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

            if let Some(conns) = active_connections {
                 if conns > 50 && (diff_in + diff_eg) < 1024 {
                     warn!("⚠️ Potential Censorship Detected on Node {}: {} connections but only {} bytes traffic.", 
                         node_id, conns, diff_in + diff_eg);
                     
                     let _ = self.trigger_mitigation(node_id).await;
                 }
            }
        }
        
        if let Some(snis) = discovered_snis {
            for sni in snis {
                let domain = sni.domain.to_lowercase();
                if domain.split('.').count() > 4 || domain.contains("traefik") || domain.contains("localhost") || domain.len() > 50 {
                    continue;
                }

                let is_blacklisted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sni_blacklist WHERE domain = $1)")
                    .bind(&domain)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(false);
                
                if is_blacklisted {
                    continue;
                }

                let _ = sqlx::query("INSERT INTO sni_pool (domain, tier, notes, is_active, discovered_by_node_id, health_score) VALUES ($1, 1, $2, TRUE, $3, 100) ON CONFLICT(domain) DO UPDATE SET notes = EXCLUDED.notes")
                    .bind(&domain)
                    .bind(format!("Discovered by Node {} (Sniper)", node_id))
                    .bind(node_id)
                    .execute(&self.pool)
                    .await;
                
                info!("💎 Neighbor Sniper: Persisted discovered SNI {} from Node {}", domain, node_id);

                let node_sni: Option<String> = sqlx::query_scalar("SELECT reality_sni FROM nodes WHERE id = $1")
                    .bind(node_id)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(None);

                let is_generic = node_sni.as_deref().map(|s| s == "www.google.com" || s == "google.com" || s == "www.microsoft.com").unwrap_or(true);
                
                if is_generic {
                    let _ = sqlx::query("UPDATE nodes SET reality_sni = $1 WHERE id = $2")
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

    async fn trigger_mitigation(&self, node_id: i64) -> Result<()> {
        info!("🔧 Triggering SNI Rotation for Node {} due to detected censorship.", node_id);
        
        match self.security_service.rotate_node_sni(node_id, "Auto-Heal: Connection Freezing").await {
            Ok((old_sni, new_sni, rotation_id)) => {
                 info!("✅ Auto-Healed Node {}: {} -> {}", node_id, old_sni, new_sni);
                 
                 if let Some(bot) = self.bot_manager.get_bot().await.ok().map(|b| b as teloxide::Bot) {
                     let notify_svc = self.notification_service.clone();
                     let old = old_sni.clone();
                     let new = new_sni.clone();
                     tokio::spawn(async move {
                         let _ = notify_svc.notify_sni_rotation(&bot, node_id, &old, &new, rotation_id).await;
                     });
                 }
            },
            Err(e) => {
                warn!("❌ Failed to auto-heal node {}: {}", node_id, e);
            }
        }
        
        Ok(())
    }
}

fn derive_recommended_max_users(
    speed_mbps: Option<i32>,
    cpu_usage: Option<f64>,
    ram_usage: Option<f64>,
    previous_max: Option<i32>,
) -> Option<i32> {
    let speed = speed_mbps?;
    if speed <= 0 {
        return None;
    }

    // Base capacity from measured throughput.
    let base_capacity = (speed / 8).clamp(2, 10_000) as f64;

    // Load-aware headroom factor.
    let avg_load = match (cpu_usage, ram_usage) {
        (Some(cpu), Some(ram)) => Some(((cpu + ram) / 2.0).clamp(0.0, 100.0)),
        (Some(cpu), None) => Some(cpu.clamp(0.0, 100.0)),
        (None, Some(ram)) => Some(ram.clamp(0.0, 100.0)),
        (None, None) => None,
    };

    let load_factor = match avg_load {
        Some(load) if load >= 90.0 => 0.35,
        Some(load) if load >= 80.0 => 0.5,
        Some(load) if load >= 70.0 => 0.65,
        Some(load) if load >= 60.0 => 0.8,
        _ => 1.0,
    };

    let raw_recommended = (base_capacity * load_factor).round() as i32;
    let raw_recommended = raw_recommended.max(1);

    // Smooth fluctuations to avoid jitter in dashboard/automation.
    let smoothed = if let Some(prev) = previous_max.filter(|v| *v > 0) {
        ((prev as f64 * 0.7) + (raw_recommended as f64 * 0.3)).round() as i32
    } else {
        raw_recommended
    };

    Some(smoothed.max(1))
}

#[cfg(test)]
mod tests {
    use super::derive_recommended_max_users;

    #[test]
    fn derive_recommended_max_users_low_load_tracks_speed() {
        let result = derive_recommended_max_users(Some(800), Some(20.0), Some(30.0), None);
        assert_eq!(result, Some(100));
    }

    #[test]
    fn derive_recommended_max_users_high_load_reduces_capacity() {
        let result = derive_recommended_max_users(Some(800), Some(90.0), Some(90.0), None);
        assert_eq!(result, Some(35));
    }

    #[test]
    fn derive_recommended_max_users_applies_smoothing() {
        let result = derive_recommended_max_users(Some(800), Some(90.0), Some(90.0), Some(100));
        // raw would be 35, smoothed => 70% of 100 + 30% of 35 = 80.5 => 81
        assert_eq!(result, Some(81));
    }
}
