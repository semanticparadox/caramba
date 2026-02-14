use sqlx::SqlitePool;
use anyhow::Result;
use crate::repositories::node_repo::NodeRepository;
use crate::models::node::Node;

#[derive(Debug, Clone)]
pub struct InfrastructureService {
    pool: SqlitePool,
    node_repo: NodeRepository,
}

impl InfrastructureService {
    pub fn new(pool: SqlitePool) -> Self {
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

    pub async fn get_user_nodes(&self, _user_id: i64) -> Result<Vec<Node>> {
        self.node_repo.get_active_nodes().await
    }

    pub async fn create_node(&self, name: &str, ip: &str, vpn_port: i32, auto_configure: bool) -> Result<i64> {
        let token = uuid::Uuid::new_v4().to_string();
        let doomsday_password = rand::distributions::Alphanumeric
            .sample_iter(&mut rand::thread_rng())
            .take(12)
            .map(char::from)
            .collect::<String>();
        
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

    pub async fn update_node(&self, id: i64, name: &str, ip: &str, relay_id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE nodes SET name = ?, ip = ?, relay_id = ? WHERE id = ?")
            .bind(name)
            .bind(ip)
            .bind(relay_id)
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
        // Clear SNI Logs (Shared logic)
        let _ = sqlx::query("DELETE FROM sni_rotation_log WHERE node_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await;

        // Unlink Subscriptions
        let _ = sqlx::query("UPDATE subscriptions SET node_id = NULL WHERE node_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await;

        self.node_repo.delete_node(id).await.map_err(|e| e.into())
    }
}
