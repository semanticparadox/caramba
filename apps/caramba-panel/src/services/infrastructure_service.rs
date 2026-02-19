use sqlx::PgPool;
use anyhow::{anyhow, Result};
use tracing::{info, warn};
use rand::distr::{Alphanumeric, SampleString};
use caramba_db::repositories::node_repo::NodeRepository;
use caramba_db::models::node::Node;

#[derive(Debug, Clone)]
pub struct InfrastructureService {
    pool: PgPool,
    pub node_repo: NodeRepository,
}

impl InfrastructureService {
    pub fn new(pool: PgPool) -> Self {
        let node_repo = NodeRepository::new(pool.clone());
        Self { pool, node_repo }
    }

    pub async fn get_node_by_id(&self, node_id: i64) -> Result<Node> {
        self.node_repo.get_node_by_id(node_id).await?
            .ok_or_else(|| anyhow::anyhow!("Node not found"))
    }

    pub async fn get_all_nodes(&self) -> Result<Vec<Node>> {
        self.node_repo.get_all_nodes().await
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<Node>> {
        self.node_repo.get_active_nodes().await
    }

    pub async fn get_node_groups(&self, node_id: i64) -> Result<Vec<caramba_db::models::groups::NodeGroup>> {
        self.node_repo.get_groups_by_node(node_id).await
    }

    pub async fn get_node_inbounds(&self, node_id: i64) -> Result<Vec<caramba_db::models::network::Inbound>> {
        let mut inbounds = self.node_repo.get_inbounds_by_node(node_id).await?;
        inbounds.sort_by_key(|i| i.listen_port);
        Ok(inbounds)
    }

    pub async fn get_user_nodes(&self, _user_id: i64) -> Result<Vec<Node>> {
        self.node_repo.get_active_nodes().await
    }

    pub async fn create_node(&self, name: &str, ip: &str, vpn_port: i32, auto_configure: bool) -> Result<i64> {
        let token = uuid::Uuid::new_v4().to_string();
        let doomsday_password = Alphanumeric.sample_string(&mut rand::rng(), 12);
        
        let final_ip = if ip.is_empty() { 
            format!("pending-{}", &token[0..8]) 
        } else { 
            ip.to_string() 
        };

        let node = Node {
            id: 0,
            name: name.to_string(),
            ip: final_ip,
            status: "new".to_string(),
            reality_pub: None,
            reality_priv: None,
            short_id: None,
            domain: None,
            root_password: None,
            version: None,
            target_version: None,
            vpn_port: vpn_port as i64,
            last_seen: None,
            created_at: chrono::Utc::now(),
            join_token: Some(token),
            auto_configure,
            is_enabled: true,
            country_code: None,
            country: None,
            city: None,
            flag: None,
            reality_sni: None,
            load_stats: None,
            check_stats_json: None,
            sort_order: 0,
            latitude: None,
            longitude: None,
            config_qos_enabled: false,
            config_block_torrent: false,
            config_block_ads: false,
            config_block_porn: false,
            last_latency: None,
            last_cpu: None,
            last_ram: None,
            max_ram: 0,
            cpu_cores: 0,
            cpu_model: None,
            speed_limit_mbps: 0,
            max_users: 0,
            current_speed_mbps: 0,
            relay_id: None,
            active_connections: None,
            total_ingress: 0,
            total_egress: 0,
            uptime: 0,
            last_session_ingress: 0,
            last_session_egress: 0,
            doomsday_password: Some(doomsday_password),
            last_synced_at: None,
            last_sync_trigger: None,
            is_relay: false,
            pending_log_collection: false,
        };

        let id = self.node_repo.create_node(&node).await?;
        
        // Non-critical enrichment: if group tables are out-of-sync, keep node created.
        match self.node_repo.get_group_by_name("Default").await {
            Ok(default_group) => {
                let group_id = match default_group {
                    Some(g) => g.id,
                    None => match self
                        .node_repo
                        .create_group("Default", Some("Default group for new nodes"))
                        .await
                    {
                        Ok(new_id) => new_id,
                        Err(e) => {
                            warn!("Node {} created but failed to create Default group: {}", id, e);
                            return Ok(id);
                        }
                    },
                };

                if let Err(e) = self.node_repo.add_node_to_group(id, group_id).await {
                    warn!("Node {} created but failed to attach to Default group: {}", id, e);
                }
            }
            Err(e) => warn!("Node {} created but failed to query Default group: {}", id, e),
        }
        
        Ok(id)
    }

    pub async fn update_node(&self, id: i64, name: &str, ip: &str, relay_id: Option<i64>, is_relay: bool) -> Result<()> {
        let primary = sqlx::query("UPDATE nodes SET name = $1, ip = $2, relay_id = $3, is_relay = $4 WHERE id = $5")
            .bind(name)
            .bind(ip)
            .bind(relay_id)
            .bind(is_relay)
            .bind(id)
            .execute(&self.pool)
            .await;

        match primary {
            Ok(_) => Ok(()),
            Err(e) => {
                // Backward-compat for installations where nodes.relay_id is not migrated yet.
                let msg = e.to_string();
                if msg.contains("relay_id") && msg.contains("does not exist") {
                    sqlx::query("UPDATE nodes SET name = $1, ip = $2, is_relay = $3 WHERE id = $4")
                        .bind(name)
                        .bind(ip)
                        .bind(is_relay)
                        .bind(id)
                        .execute(&self.pool)
                        .await?;
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub async fn toggle_node_enable(&self, id: i64) -> Result<()> {
        self.node_repo.toggle_enabled(id).await?;
        Ok(())
    }

    pub async fn activate_node(&self, id: i64, security_service: &crate::services::security_service::SecurityService) -> Result<()> {
        // 1. Mark as active
        self.node_repo.update_status(id, "active").await?;

        // 2. Automated Smart SNI selection
        if let Ok(best_sni) = security_service.get_best_sni_for_node(id).await {
            info!("🎯 Smart Setup: Auto-selected best SNI for Node {}: {}", id, best_sni);
            let _ = sqlx::query("UPDATE nodes SET reality_sni = $1 WHERE id = $2")
                .bind(best_sni)
                .bind(id)
                .execute(&self.pool)
                .await;
        }

        Ok(())
    }

    pub async fn delete_node(&self, id: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        Self::run_cleanup_optional(
            &mut tx,
            "DELETE FROM plan_inbounds WHERE inbound_id IN (SELECT id FROM inbounds WHERE node_id = $1)",
            id,
            "plan_inbounds cleanup",
        )
        .await?;
        Self::run_cleanup_optional(
            &mut tx,
            "DELETE FROM inbounds WHERE node_id = $1",
            id,
            "inbounds cleanup",
        )
        .await?;
        Self::run_cleanup_optional(
            &mut tx,
            "DELETE FROM plan_nodes WHERE node_id = $1",
            id,
            "plan_nodes cleanup",
        )
        .await?;
        Self::run_cleanup_optional(
            &mut tx,
            "DELETE FROM node_group_members WHERE node_id = $1",
            id,
            "node_group_members cleanup",
        )
        .await?;
        Self::run_cleanup_optional(
            &mut tx,
            "DELETE FROM sni_rotation_log WHERE node_id = $1",
            id,
            "sni_rotation_log cleanup",
        )
        .await?;
        Self::run_cleanup_optional(
            &mut tx,
            "UPDATE sni_pool SET discovered_by_node_id = NULL WHERE discovered_by_node_id = $1",
            id,
            "sni_pool unlink",
        )
        .await?;
        Self::run_cleanup_optional(
            &mut tx,
            "UPDATE subscriptions SET node_id = NULL WHERE node_id = $1",
            id,
            "subscriptions unlink",
        )
        .await?;

        let result = sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow!("Node {} not found", id));
        }

        // Keep UX deterministic for fresh labs: if all nodes were removed, reset sequence.
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
            .fetch_one(&mut *tx)
            .await?;
        if remaining == 0 {
            let _ = sqlx::query("SELECT setval(pg_get_serial_sequence('nodes', 'id'), 1, false)")
                .execute(&mut *tx)
                .await;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn run_cleanup_optional(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sql: &str,
        node_id: i64,
        step: &str,
    ) -> Result<()> {
        match sqlx::query(sql).bind(node_id).execute(&mut **tx).await {
            Ok(_) => Ok(()),
            Err(e) => {
                if is_undefined_table_or_column(&e) {
                    warn!("Skipping {} for node {} (legacy schema): {}", step, node_id, e);
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }
}

fn is_undefined_table_or_column(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = err {
        if let Some(code) = db_err.code() {
            return code == "42P01" || code == "42703";
        }
    }
    false
}
