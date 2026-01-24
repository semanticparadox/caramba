use sqlx::SqlitePool;
use tracing::{info, error};
use std::sync::Arc;

use crate::models::store::Subscription;
use crate::models::node::Node;
use crate::singbox::{ConfigGenerator};
use crate::ssh::execute_remote_script;
use crate::services::store_service::StoreService;

pub struct OrchestrationService {
    pool: SqlitePool,
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
        // Generate keys using x25519-dalek 2.0+ (StaticSecret is now used usually, or EphemeralSecret)
        // EphemeralSecret::new is deprecated -> random_from_rng
        let (priv_key, pub_key) = {
            use rand::rngs::OsRng;
            use x25519_dalek::{StaticSecret, PublicKey};
            
            // Use StaticSecret which allows exporting bytes easily
            let secret = StaticSecret::random_from_rng(OsRng);
            let public = PublicKey::from(&secret);
            (hex::encode(secret.to_bytes()), hex::encode(public.as_bytes()))
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
            
        use crate::models::network::{InboundType, VlessSettings, RealitySettings, Hysteria2Settings, Hysteria2Obfs};
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
                dest: "www.microsoft.com:443".to_string(),
                server_names: vec!["www.microsoft.com".to_string(), "microsoft.com".to_string()],
                private_key: priv_key,
                short_ids: vec![short_id],
                max_time_diff: Some(0), // Added Option<i64> to model
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
        let obfs_pass = uuid::Uuid::new_v4().to_string().replace("-", "");
        
        let hy2_settings_struct = Hysteria2Settings {
             users: vec![],
             up_mbps: 100,
             down_mbps: 100,
             obfs: Some(Hysteria2Obfs {
                 ttype: "salamander".to_string(),
                 password: obfs_pass,
             }),
             masquerade: None,
        };

        let hy2_json = serde_json::to_string(&InboundType::Hysteria2(hy2_settings_struct))?;
        
        // Hysteria 2 uses UDP, correct. Stream settings usually empty or basic.
        let hy2_stream = StreamSettings {
            network: "udp".to_string(), // Hysteria is UDP based
            security: "none".to_string(),
            tls_settings: None,
            reality_settings: None,
        };
        
        sqlx::query("INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, enable) VALUES (?, ?, 'hysteria2', 443, ?, ?, 1)")
            .bind(node_id)
            .bind(format!("hysteria2-{}", node_id))
            .bind(hy2_json)
            .bind(serde_json::to_string(&hy2_stream)?)
            .execute(&self.pool)
            .await?;
            
        // 3. Trigger Sync
        self.sync_node_config(node_id).await
    }

    /// Generates Node Config JSON without applying it (Internal)
    pub async fn generate_node_config_json(&self, node_id: i64) -> anyhow::Result<(crate::models::node::Node, serde_json::Value)> {
        // 1. Fetch node details
        let node: Node = sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
            .bind(node_id)
            .fetch_one(&self.pool)
            .await?;

        // 2. Fetch Inbounds for this node
        let mut inbounds: Vec<crate::models::network::Inbound> = sqlx::query_as("SELECT * FROM inbounds WHERE node_id = ?")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await?;

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

            let plan_ids_str = linked_plans.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
            let query = format!("SELECT * FROM subscriptions WHERE status = 'active' AND plan_id IN ({})", plan_ids_str);
            
            let active_subs: Vec<Subscription> = sqlx::query_as(&query)
                .fetch_all(&self.pool)
                .await?;

            use crate::models::network::{InboundType, VlessClient, Hysteria2User};

            match serde_json::from_str::<InboundType>(&inbound.settings) {
                Ok(mut settings) => {
                    match &mut settings {
                        InboundType::Vless(ref mut vless) => {
                            for sub in &active_subs {
                                if let Some(uuid) = &sub.vless_uuid {
                                    vless.clients.push(VlessClient {
                                        id: uuid.clone(),
                                        email: format!("user_{}", sub.user_id),
                                        flow: "xtls-rprx-vision".to_string(), // Default flow
                                    });
                                }
                            }
                        },
                        InboundType::Hysteria2(ref mut hy2) => {
                             for sub in &active_subs {
                                if let Some(uuid) = &sub.vless_uuid {
                                    hy2.users.push(Hysteria2User {
                                        name: format!("user_{}", sub.user_id),
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

        // 4. Generate Config
        let config = ConfigGenerator::generate_config(
            &node.ip,
            inbounds
        );
        
        Ok((node, serde_json::to_value(&config)?))
    }

    /// Syncs configuration for a specific node
    pub async fn sync_node_config(&self, node_id: i64) -> anyhow::Result<()> {
        info!("Syncing config for node ID: {}", node_id);

        let (node, config_value) = self.generate_node_config_json(node_id).await?;
        
        if node.status != "active" {
            // return Err(anyhow::anyhow!("Node is not active"));
        }

        let config_json = serde_json::to_string_pretty(&config_value)?;
        let password = node.ssh_password; 
        
        info!("Pushing updated Sing-box configuration to {}...", node.ip);
        
        let safe_cmd = if node.ssh_user == "root" {
             format!("mkdir -p /etc/sing-box && echo '{}' > /etc/sing-box/config.json && systemctl restart sing-box", config_json.replace("'", "'\\''"))
        } else {
             format!("sudo mkdir -p /etc/sing-box && echo '{}' | sudo tee /etc/sing-box/config.json > /dev/null && sudo systemctl restart sing-box", config_json.replace("'", "'\\''"))
        };

        let (tx, mut _rx) = tokio::sync::mpsc::channel(10);
        execute_remote_script(&node.ip, &node.ssh_user, node.ssh_port, &password, &safe_cmd, tx).await?;
        
        Ok(())
    }

    /// Syncs all active nodes
    pub async fn sync_all_nodes(&self) -> anyhow::Result<()> {
        let active_nodes: Vec<Node> = sqlx::query_as("SELECT * FROM nodes WHERE status = 'active'")
            .fetch_all(&self.pool)
            .await?;

        info!("Syncing {} active nodes. Store service status: active", active_nodes.len());

        for node in active_nodes {
            if let Err(e) = self.sync_node_config(node.id).await {
                error!("Failed to sync node {}: {}", node.ip, e);
            }
        }

        Ok(())
    }

    /// Get all nodes (for admin UI)
    pub async fn get_all_nodes(&self) -> anyhow::Result<Vec<Node>> {
        let all_nodes: Vec<Node> = sqlx::query_as("SELECT * FROM nodes ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;

        Ok(all_nodes)
    }

    /// Fetches traffic usage from a node via sing-box API
    pub async fn get_node_usage(&self, node_id: i64) -> anyhow::Result<serde_json::Value> {
        let node: Node = sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
            .bind(node_id)
            .fetch_one(&self.pool)
            .await?;

        let password = node.ssh_password; // Option<String>
        
        // Command to fetch stats from sing-box via its local API (default port 9090 if configured)
        // ... (lines 175-186 omitted/same)
        
        let cmd = "curl -s http://127.0.0.1:9090/traffic || echo '{}'";
        
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        execute_remote_script(&node.ip, &node.ssh_user, node.ssh_port, &password, cmd, tx).await?;
        
        let mut full_output = String::new();
        while let Some(line) = rx.recv().await {
            full_output.push_str(&line);
        }

        match serde_json::from_str(&full_output) {
            Ok(json) => Ok(json),
            Err(_) => Ok(serde_json::json!({})),
        }
    }
}
