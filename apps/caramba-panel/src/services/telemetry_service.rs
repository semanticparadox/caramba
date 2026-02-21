use anyhow::Result;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{info, warn};

use crate::bot_manager::BotManager;
use crate::services::notification_service::NotificationService;
use crate::services::security_service::SecurityService;

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
            bot_manager,
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

            let node_load: Option<(Option<f64>, Option<f64>, Option<i32>)> =
                sqlx::query_as("SELECT last_cpu, last_ram, max_users FROM nodes WHERE id = $1")
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
                 WHERE id = $9",
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
                    warn!(
                        "⚠️ Potential Censorship Detected on Node {}: {} connections but only {} bytes traffic.",
                        node_id,
                        conns,
                        diff_in + diff_eg
                    );

                    let _ = self.trigger_mitigation(node_id).await;
                }
            }
        }

        if let Some(snis) = discovered_snis {
            // Reduce repeated churn from node scans:
            // - keep only first unique domain entries in this heartbeat
            // - hard-cap processing to avoid DB spikes on noisy /24 blocks
            let mut seen_domains = HashSet::new();
            for sni in snis.into_iter().take(256) {
                let domain = sni.domain.trim().to_lowercase();
                if domain.is_empty() || !seen_domains.insert(domain.clone()) {
                    continue;
                }

                if let Err(reason) = classify_discovered_domain(&domain) {
                    if let Err(e) = auto_blacklist_domain(
                        &self.pool,
                        &domain,
                        &format!("Auto-filter: {}", reason),
                    )
                    .await
                    {
                        warn!(
                            "Failed to auto-blacklist discovered SNI '{}': {}",
                            domain, e
                        );
                    } else {
                        info!(
                            "🚫 Neighbor Sniper: Auto-blacklisted noisy SNI '{}' ({})",
                            domain, reason
                        );
                    }
                    continue;
                }

                let is_blacklisted: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM sni_blacklist WHERE domain = $1)",
                )
                .bind(&domain)
                .fetch_one(&self.pool)
                .await
                .unwrap_or(false);

                if is_blacklisted {
                    continue;
                }

                let insert_primary = sqlx::query(
                    "INSERT INTO sni_pool (domain, tier, notes, is_active, discovered_by_node_id, health_score) VALUES ($1, 1, $2, TRUE, $3, 100) ON CONFLICT(domain) DO UPDATE SET notes = EXCLUDED.notes, discovered_by_node_id = COALESCE(sni_pool.discovered_by_node_id, EXCLUDED.discovered_by_node_id)"
                )
                    .bind(&domain)
                    .bind(format!("Discovered by Node {} (Sniper)", node_id))
                    .bind(node_id)
                    .execute(&self.pool)
                    .await;

                if let Err(e) = insert_primary {
                    warn!(
                        "SNI insert with full schema failed for '{}': {}. Trying compatibility fallback.",
                        domain, e
                    );
                    if let Err(e2) = sqlx::query(
                        "INSERT INTO sni_pool (domain) VALUES ($1) ON CONFLICT(domain) DO NOTHING",
                    )
                    .bind(&domain)
                    .execute(&self.pool)
                    .await
                    {
                        warn!("SNI fallback insert failed for '{}': {}", domain, e2);
                        continue;
                    }
                }

                info!(
                    "💎 Neighbor Sniper: Persisted discovered SNI {} from Node {}",
                    domain, node_id
                );

                let node_sni: Option<String> =
                    sqlx::query_scalar("SELECT reality_sni FROM nodes WHERE id = $1")
                        .bind(node_id)
                        .fetch_one(&self.pool)
                        .await
                        .unwrap_or(None);

                let is_generic = node_sni
                    .as_deref()
                    .map(|s| s == "www.google.com" || s == "google.com" || s == "www.microsoft.com")
                    .unwrap_or(true);

                if is_generic {
                    let _ = sqlx::query("UPDATE nodes SET reality_sni = $1 WHERE id = $2")
                        .bind(&domain)
                        .bind(node_id)
                        .execute(&self.pool)
                        .await;
                    info!(
                        "✨ Neighbor Sniper: Automatically assigned discovered SNI {} to Node {}",
                        domain, node_id
                    );
                }
            }
        }

        Ok(())
    }

    async fn trigger_mitigation(&self, node_id: i64) -> Result<()> {
        info!(
            "🔧 Triggering SNI Rotation for Node {} due to detected censorship.",
            node_id
        );

        match self
            .security_service
            .rotate_node_sni(node_id, "Auto-Heal: Connection Freezing")
            .await
        {
            Ok((old_sni, new_sni, rotation_id)) => {
                info!(
                    "✅ Auto-Healed Node {}: {} -> {}",
                    node_id, old_sni, new_sni
                );

                if let Some(bot) = self
                    .bot_manager
                    .get_bot()
                    .await
                    .ok()
                    .map(|b| b as teloxide::Bot)
                {
                    let notify_svc = self.notification_service.clone();
                    let old = old_sni.clone();
                    let new = new_sni.clone();
                    tokio::spawn(async move {
                        let _ = notify_svc
                            .notify_sni_rotation(&bot, node_id, &old, &new, rotation_id)
                            .await;
                    });
                }
            }
            Err(e) => {
                warn!("❌ Failed to auto-heal node {}: {}", node_id, e);
            }
        }

        Ok(())
    }
}

fn classify_discovered_domain(domain: &str) -> Result<(), String> {
    let domain = domain.trim().to_lowercase();

    if domain.is_empty() {
        return Err("empty domain".to_string());
    }
    if domain.len() > 120 {
        return Err("domain is too long".to_string());
    }
    if !domain.contains('.') {
        return Err("not a fqdn".to_string());
    }
    if domain.contains(' ') || domain.contains('_') {
        return Err("contains invalid characters".to_string());
    }

    if !domain
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
    {
        return Err("contains non-dns characters".to_string());
    }

    if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return Err("malformed fqdn".to_string());
    }

    const DENY_SUBSTRINGS: &[&str] = &[
        "localhost",
        "traefik",
        "plesk",
        "parallels",
        "easypanel",
        "directadmin",
        "cpanel",
        "access-denied",
        "access denied",
        "forbidden",
        "sni-support-required",
    ];
    if DENY_SUBSTRINGS.iter().any(|needle| domain.contains(needle)) {
        return Err("service/control-plane noise".to_string());
    }

    const DENY_SUFFIXES: &[&str] = &[
        ".local",
        ".localdomain",
        ".internal",
        ".lan",
        ".invalid",
        ".example",
        ".test",
        ".home.arpa",
        ".traefik.default",
        ".plesk.page",
        ".vps.ovh.net",
    ];
    if DENY_SUFFIXES.iter().any(|suffix| domain.ends_with(suffix)) {
        return Err("reserved/internal/provider suffix".to_string());
    }

    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 || labels.len() > 8 {
        return Err("invalid label count".to_string());
    }

    for label in &labels {
        if label.is_empty() || label.len() > 63 {
            return Err("invalid label length".to_string());
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("label starts/ends with '-'".to_string());
        }
    }

    let tld = labels.last().copied().unwrap_or_default();
    const RESERVED_TLDS: &[&str] = &[
        "local", "internal", "lan", "invalid", "example", "test", "default",
    ];
    if RESERVED_TLDS.contains(&tld) {
        return Err("reserved tld".to_string());
    }
    if tld.len() < 2 {
        return Err("invalid tld".to_string());
    }

    Ok(())
}

async fn auto_blacklist_domain(pool: &PgPool, domain: &str, reason: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO sni_blacklist (domain, reason) VALUES ($1, $2) ON CONFLICT (domain) DO NOTHING",
    )
    .bind(domain)
    .bind(reason)
    .execute(pool)
    .await?;

    // Keep pool clean if this domain was already inserted before filtering hardened.
    sqlx::query("UPDATE sni_pool SET is_active = FALSE, health_score = 0 WHERE domain = $1")
        .bind(domain)
        .execute(pool)
        .await?;

    Ok(())
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
    use super::{classify_discovered_domain, derive_recommended_max_users};

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

    #[test]
    fn discovered_domain_filter_accepts_normal_domains() {
        assert!(classify_discovered_domain("hitchhive.app").is_ok());
        assert!(classify_discovered_domain("api.kd367.fr").is_ok());
    }

    #[test]
    fn discovered_domain_filter_rejects_noise() {
        assert!(classify_discovered_domain("Plesk").is_err());
        assert!(classify_discovered_domain("traefik.default").is_err());
        assert!(classify_discovered_domain("with space.example.com").is_err());
        assert!(classify_discovered_domain("localhost").is_err());
        assert!(classify_discovered_domain("Parallels Panel").is_err());
        assert!(classify_discovered_domain("vps-40d02f7d.vps.ovh.net").is_err());
        assert!(classify_discovered_domain("sni-support-required-for-valid-ssl").is_err());
    }
}
