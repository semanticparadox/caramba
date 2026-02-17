use sqlx::PgPool;
use anyhow::Result;
use tracing::info;
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

    pub async fn get_node_groups(&self, node_id: i64) -> Result<Vec<crate::models::groups::NodeGroup>> {
        self.node_repo.get_groups_by_node(node_id).await
    }

    pub async fn get_node_inbounds(&self, node_id: i64) -> Result<Vec<crate::models::network::Inbound>> {
        let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>("SELECT * FROM inbounds WHERE node_id = $1 ORDER BY listen_port ASC")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await?;
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
        
        // Phase 16: Ensure node is added to "Default" group
        let default_group = self.node_repo.get_group_by_name("Default").await?;
        let group_id = match default_group {
            Some(g) => g.id,
            None => {
                // Create Default Group if missing
                self.node_repo.create_group("Default", Some("Default group for new nodes")).await?
            }
        };
        
        self.node_repo.add_node_to_group(id, group_id).await?;
        
        Ok(id)
    }

    pub async fn update_node(&self, id: i64, name: &str, ip: &str, relay_id: Option<i64>, is_relay: bool) -> Result<()> {
        sqlx::query("UPDATE nodes SET name = $1, ip = $2, relay_id = $3, is_relay = $4 WHERE id = $5")
            .bind(name)
            .bind(ip)
            .bind(relay_id)
            .bind(is_relay)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
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
            let _ = sqlx::query("UPDATE nodes SET reality_sni = ? WHERE id = ?")
                .bind(best_sni)
                .bind(id)
                .execute(&self.pool)
                .await;
        }

        Ok(())
    }

    pub async fn delete_node(&self, id: i64) -> Result<()> {
        // 1. Delete plan_inbound links for this node's inbounds
        let _ = sqlx::query(
            "DELETE FROM plan_inbounds WHERE inbound_id IN (SELECT id FROM inbounds WHERE node_id = $1)"
        )
            .bind(id)
            .execute(&self.pool)
            .await;

        // 2. Delete inbounds belonging to this node
        let _ = sqlx::query("DELETE FROM inbounds WHERE node_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await;

        // 3. Delete plan-node links
        let _ = sqlx::query("DELETE FROM plan_nodes WHERE node_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await;

        // 4. Delete node group memberships
        let _ = sqlx::query("DELETE FROM node_group_members WHERE node_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await;

        // 5. Clear SNI rotation logs
        let _ = sqlx::query("DELETE FROM sni_rotation_log WHERE node_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await;

        // 6. Nullify SNI pool discovered_by references
        let _ = sqlx::query("UPDATE sni_pool SET discovered_by_node_id = NULL WHERE discovered_by_node_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await;

        // 7. Unlink subscriptions
        let _ = sqlx::query("UPDATE subscriptions SET node_id = NULL WHERE node_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await;

        // 8. Finally, delete the node itself
        self.node_repo.delete_node(id).await.map_err(|e| e.into())
    }
}
