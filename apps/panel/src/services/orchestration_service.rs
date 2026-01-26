use sqlx::SqlitePool;
use tracing::{info, error};
use std::sync::Arc;


// Removed unused Subscription import
use crate::models::node::Node;
use crate::singbox::{ConfigGenerator};
use crate::services::store_service::StoreService;


pub struct OrchestrationService {
    pub pool: SqlitePool,
    #[allow(dead_code)]
    store_service: Arc<StoreService>,
}

impl OrchestrationService {
    pub fn new(pool: SqlitePool, store_service: Arc<StoreService>) -> Self {
        Self { pool, store_service }
    }
    /// Initializes default inbounds (VLESS Reality & Hysteria 2) for a fresh node
    pub async fn init_default_inbounds(&self, node_id: i64) -> anyhow::Result<()> {
        info!("Initializing default inbounds for node {}", node_id);
        
        // 1. VLESS Reality (Vision)
        // Use sing-box native generation for guaranteed compatibility
        let (priv_key, pub_key) = {
            use std::process::Command;
            
            let output = Command::new("sing-box")
                .args(&["generate", "reality-keypair"])
                .output()
                .map_err(|e| anyhow::anyhow!("Failed to execute sing-box generate: {}. Ensure sing-box is installed.", e))?;
            
            if !output.status.success() {
                return Err(anyhow::anyhow!("sing-box generate failed: {}", String::from_utf8_lossy(&output.stderr)));
            }
            
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut priv_key = String::new();
            let mut pub_key = String::new();
            
            for line in output_str.lines() {
                if let Some(key) = line.strip_prefix("PrivateKey:") {
                    priv_key = key.trim().to_string();
                } else if let Some(key) = line.strip_prefix("PublicKey:") {
                    pub_key = key.trim().to_string();
                }
            }
            
            if priv_key.is_empty() || pub_key.is_empty() {
                return Err(anyhow::anyhow!("Failed to parse sing-box output"));
            }
            
            (priv_key, pub_key)
        };
        
        let short_id = hex::encode(&rand::random::<[u8; 8]>());
        
        // Save keys to node
        sqlx::query("UPDATE nodes SET reality_priv = ?, reality_pub = ?, short_id = ? WHERE id = ?")
            .bind(&priv_key)
            .bind(&pub_key)
            .bind(&short_id)
            .bind(node_id)
            .execute(&self.pool)
            .await?;
            
        use crate::models::network::{InboundType, VlessSettings, RealitySettings, Hysteria2Settings};
        // Removed DestOverride if not in models
        
        let vless_settings_struct = VlessSettings {
            clients: vec![],
            decryption: "none".to_string(),
            fallbacks: None,
        };
        let vless_json = serde_json::to_string(&InboundType::Vless(vless_settings_struct))?;
        
        // Stream Settings for Reality
        use crate::models::network::StreamSettings;
        let stream_settings = StreamSettings {
            network: "tcp".to_string(),
            security: "reality".to_string(),
            tls_settings: None,
            reality_settings: Some(RealitySettings {
                show: true,
                xver: 0,
                dest: "drive.google.com:443".to_string(),
                server_names: vec!["drive.google.com".to_string()],
                private_key: priv_key,
                short_ids: vec![short_id],
                max_time_diff: Some(0), 
            }),
        };
        let stream_json = serde_json::to_string(&stream_settings)?;
        
        sqlx::query("INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, enable) VALUES (?, ?, 'vless', 443, ?, ?, 1)")
            .bind(node_id)
            .bind(format!("vless-reality-{}", node_id))
            .bind(vless_json) // This might be wrong if InboundType wrapper is used inside "settings". 
            // Usually "settings" column stores the Inner object OR the Typed object?
            // "settings" usually stores the protocol settings.
            // Let's assume we store the "VlessSettings" JSON, not "InboundType::Vless".
            // But ConfigGenerator needs to know.
            // Let's assume we store pure VlessSettings JSON inside 'settings' column.
            // Re-check Sync logic: `match serde_json::from_str::<InboundType>(&inbound.settings)`
            // So it expects the Enum wrapper! Okay.
            .bind(stream_json)
            .execute(&self.pool)
            .await?;

        // 2. Hysteria 2
        // let obfs_pass = uuid::Uuid::new_v4().to_string().replace("-", ""); // Unused when OBFS disabled
        
        let hy2_settings_struct = Hysteria2Settings {
             users: vec![],
             up_mbps: 100,
             down_mbps: 100,
             obfs: None, // Disabled by default for better compatibility (matches Blitz)
             masquerade: Some("/opt/exarobot/apps/panel/assets/masquerade".to_string()),
        };

        let hy2_json = serde_json::to_string(&InboundType::Hysteria2(hy2_settings_struct))?;
        
        // Hysteria 2 uses UDP, correct. Stream settings with TLS.
        use crate::models::network::TlsSettings;
        let hy2_stream = StreamSettings {
            network: "udp".to_string(), // Hysteria is UDP based
            security: "tls".to_string(),
            tls_settings: Some(TlsSettings {
                server_name: "drive.google.com".to_string(),
                certificates: None, // Will use auto-generated certs
            }),
            reality_settings: None,
        };
        
        sqlx::query("INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, enable) VALUES (?, ?, 'hysteria2', 8443, ?, ?, 1)")
            .bind(node_id)
            .bind(format!("hysteria2-{}", node_id))
            .bind(hy2_json)
            .bind(serde_json::to_string(&hy2_stream)?)
            .execute(&self.pool)
            .await?;
            
        Ok(())
    }

    /// Generates Node Config JSON without applying it (Internal)
    pub async fn generate_node_config_json(&self, node_id: i64) -> anyhow::Result<(crate::models::node::Node, serde_json::Value)> {
        info!("Step 1: Fetching node details for ID: {}", node_id);
        // 1. Fetch node details
        let node: Node = sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
            .bind(node_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                error!("Failed to fetch node: {}", e);
                e
            })?;

        info!("Step 2: Fetching inbounds for node {}", node_id);
        // 2. Fetch Inbounds for this node
        let mut inbounds: Vec<crate::models::network::Inbound> = sqlx::query_as("SELECT * FROM inbounds WHERE node_id = ?")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await?;

        info!("Step 3: Injecting users for {} inbounds", inbounds.len());
        // 3. For each inbound, inject authorized users
        for inbound in &mut inbounds {
            // Find plans linked to this inbound
            let linked_plans: Vec<i64> = sqlx::query_scalar("SELECT plan_id FROM plan_inbounds WHERE inbound_id = ?")
                .bind(inbound.id)
                .fetch_all(&self.pool)
                .await?;

            if linked_plans.is_empty() {
                continue;
            }

    
        // Helper struct for query
        #[derive(sqlx::FromRow)]
        struct SubWithUser {
            vless_uuid: Option<String>,
            tg_id: i64,
            username: Option<String>,
        }

        let plan_ids_str = linked_plans.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        
        let query = format!(
            r#"
            SELECT s.vless_uuid, u.tg_id, u.username
            FROM subscriptions s
            JOIN users u ON s.user_id = u.id
            WHERE s.status = 'active' AND s.plan_id IN ({})
            "#, 
            plan_ids_str
        );
        
        let active_subs: Vec<SubWithUser> = sqlx::query_as(&query)
            .fetch_all(&self.pool)
            .await?;

        use crate::models::network::{InboundType, VlessClient, Hysteria2User};

        match serde_json::from_str::<InboundType>(&inbound.settings) {
            Ok(mut settings) => {
                match &mut settings {
                    InboundType::Vless(vless) => {
                        for sub in &active_subs {
                            if let Some(uuid) = &sub.vless_uuid {
                                // Logic: Use TG ID as clean identifier (most stable). 
                                // Alternatively use username if requested, but TG ID is safer for auth.
                                // User asked for "ID telegram or username without @"
                                // Let's use TG ID as primary auth name to avoid breakage if they change username.
                                let auth_name = sub.tg_id.to_string();
                                
                                // Clean username for logging/comments (optional)
                                let _display_name = sub.username.clone().unwrap_or_default().replace("@", "");

                                info!("🔑 Injecting VLESS user: {} (UUID: {})", auth_name, uuid);
                                vless.clients.push(VlessClient {
                                    id: uuid.clone(),
                                    email: auth_name,
                                    flow: "xtls-rprx-vision".to_string(), 
                                });
                            }
                        }
                    },
                    InboundType::Hysteria2(hy2) => {
                         for sub in &active_subs {
                            if let Some(uuid) = &sub.vless_uuid {
                                let auth_name = sub.tg_id.to_string();
                                
                                info!("🔑 Injecting HYSTERIA user: {} (Pass: {})", auth_name, uuid);
                                hy2.users.push(Hysteria2User {
                                    name: auth_name,
                                    password: uuid.clone(),
                                });
                            }
                        }
                    },
                    _ => {}
                }
                inbound.settings = serde_json::to_string(&settings)?;
            },
                Err(e) => {
                    error!("Skipping user injection for inbound {} due to parse error: {}", inbound.tag, e);
                }
            }
        }

        info!("Step 4: generating final sing-box config JSON");
        // 4. Generate Config
        let config = ConfigGenerator::generate_config(
            &node.ip,
            inbounds
        );
        
        info!("Config generation successful for node {}", node_id);
        Ok((node, serde_json::to_value(&config)?))
    }

    /// Get all nodes (for admin UI)
    pub async fn get_all_nodes(&self) -> anyhow::Result<Vec<Node>> {
        let all_nodes: Vec<Node> = sqlx::query_as("SELECT * FROM nodes ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;

        Ok(all_nodes)
    }

}
