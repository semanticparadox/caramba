use sqlx::SqlitePool;
use tracing::{info, error};
use std::sync::Arc;


// Removed unused Subscription import
use crate::models::node::Node;
use crate::singbox::{ConfigGenerator};
use crate::services::store_service::StoreService;


use crate::repositories::node_repo::NodeRepository;

pub struct OrchestrationService {
    pub pool: SqlitePool,
    pub node_repo: NodeRepository,
    #[allow(dead_code)]
    store_service: Arc<StoreService>,
}

impl OrchestrationService {
    pub fn new(pool: SqlitePool, store_service: Arc<StoreService>) -> Self {
        let node_repo = NodeRepository::new(pool.clone());
        Self { pool, node_repo, store_service }
    }
    /// Initializes default inbounds (VLESS Reality & Hysteria 2) for a fresh node
    pub async fn init_default_inbounds(&self, node_id: i64) -> anyhow::Result<()> {
        info!("Initializing default inbounds for node {}", node_id);
        
        // Fetch node for SNI defaults
        let node = self.node_repo.get_node_by_id(node_id).await?
            .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
        
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
        let mut updated_node = node.clone();
        updated_node.reality_priv = Some(priv_key.clone());
        updated_node.reality_pub = Some(pub_key.clone());
        updated_node.short_id = Some(short_id.clone());
        self.node_repo.update_node(&updated_node).await?;
            
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
            network: Some("tcp".to_string()),
            security: Some("reality".to_string()),
            tls_settings: None,
            reality_settings: Some(RealitySettings {
                show: true,
                xver: 0,
                dest: "drive.google.com:443".to_string(),
                server_names: vec!["drive.google.com".to_string()],
                private_key: priv_key,
                public_key: Some(pub_key),
                short_ids: vec![short_id],
                max_time_diff: Some(0), 
            }),
            ws_settings: None,
            http_upgrade_settings: None,
        };
        let stream_json = serde_json::to_string(&stream_settings)?;
        
        use crate::models::network::Inbound;
        let vless_inbound = Inbound {
            id: 0, // Generated
            node_id,
            tag: format!("vless-reality-{}", node_id),
            protocol: "vless".to_string(),
            listen_port: 443,
            listen_ip: "0.0.0.0".to_string(),
            settings: vless_json,
            stream_settings: stream_json,
            remark: None,
            enable: true,
            last_rotated_at: None,
            created_at: None,
        };
        self.node_repo.upsert_inbound(&vless_inbound).await?;

        // 2. Hysteria 2
        let hy2_settings_struct = Hysteria2Settings {
             users: vec![],
             up_mbps: 100,
             down_mbps: 100,
             obfs: None, 
             masquerade: Some("file:///opt/exarobot/apps/panel/assets/masquerade".to_string()),
        };

        let hy2_json = serde_json::to_string(&InboundType::Hysteria2(hy2_settings_struct))?;
        
        // Hysteria 2 uses UDP, correct. Stream settings with TLS.
        use crate::models::network::TlsSettings;
        let hy2_stream = StreamSettings {
            network: Some("udp".to_string()), // Hysteria is UDP based
            security: Some("tls".to_string()),
            tls_settings: Some(TlsSettings {
                server_name: node.reality_sni.clone().unwrap_or_else(|| "drive.google.com".to_string()),
                certificates: None, // Will use auto-generated certs
            }),
            reality_settings: None,
            ws_settings: None,
            http_upgrade_settings: None,
        };
        
        let hy2_inbound = Inbound {
            id: 0,
            node_id,
            tag: format!("hysteria2-{}", node_id),
            protocol: "hysteria2".to_string(),
            listen_port: 8443,
            listen_ip: "0.0.0.0".to_string(),
            settings: hy2_json,
            stream_settings: serde_json::to_string(&hy2_stream)?,
            remark: None,
            enable: true,
            last_rotated_at: None,
            created_at: None,
        };
        self.node_repo.upsert_inbound(&hy2_inbound).await?;

        // Link to default plan (ID=1)
        self.node_repo.link_node_inbounds_to_plan(1, node_id).await?;

        // 3. AmneziaWG
        // Pre-generate random values to avoid Send trait issues with ThreadRng
        let (awg_jc, awg_jmin, awg_jmax, awg_s1, awg_s2, awg_h1, awg_h2, awg_h3, awg_h4) = {
            use rand::Rng;
            let mut rng = rand::rng();
            (
                rng.random_range(3..=10),
                rng.random_range(40..=100),
                rng.random_range(500..=1000),
                rng.random_range(20..=100),
                rng.random_range(20..=100),
                rng.random::<u32>(),
                rng.random::<u32>(),
                rng.random::<u32>(),
                rng.random::<u32>(),
            )
        };
        
        // Generate WireGuard Keypair
        let (awg_priv, _awg_pub) = {
            use std::process::Command;
            let output = Command::new("sing-box")
                .args(&["generate", "wireguard-keypair"])
                .output()
                .map_err(|e| anyhow::anyhow!("Failed to execute sing-box generate (awg): {}", e))?;
            
            if !output.status.success() {
                return Err(anyhow::anyhow!("sing-box generate awg failed"));
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
            (priv_key, pub_key)
        };

        let awg_settings = crate::models::network::AmneziaWgSettings {
            users: vec![],
            private_key: awg_priv,
            listen_port: 51820,
            jc: awg_jc,
            jmin: awg_jmin,
            jmax: awg_jmax,
            s1: awg_s1,
            s2: awg_s2,
            h1: awg_h1,
            h2: awg_h2,
            h3: awg_h3,
            h4: awg_h4,
        };

        let awg_json = serde_json::to_string(&InboundType::AmneziaWg(awg_settings))?;
        
        let awg_inbound = Inbound {
            id: 0,
            node_id,
            tag: format!("amneziawg-{}", node_id),
            protocol: "amneziawg".to_string(),
            listen_port: 51820,
            listen_ip: "0.0.0.0".to_string(),
            settings: awg_json,
            stream_settings: "{}".to_string(),
            remark: None,
            enable: true,
            last_rotated_at: None,
            created_at: None,
        };
        self.node_repo.upsert_inbound(&awg_inbound).await?;
            
        // Link AWG to default plan too - already linked via link_node_inbounds_to_plan above if generic
        // but let's be safe if we want specific protocols.
        // Actually link_node_inbounds_to_plan links ALL inbounds of the node.

        // 4. TUIC
        use crate::models::network::TuicSettings;
        let tuic_settings = TuicSettings {
             users: vec![],
             congestion_control: "cubic".to_string(),
             auth_timeout: "3s".to_string(),
             zero_rtt_handshake: false,
             heartbeat: "10s".to_string(),
        };

        let tuic_json = serde_json::to_string(&InboundType::Tuic(tuic_settings))?;
        
        let tuic_stream = StreamSettings {
            network: Some("udp".to_string()),
            security: Some("tls".to_string()),
            tls_settings: Some(TlsSettings {
                server_name: node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string()),
                certificates: None,
            }),
            reality_settings: None,
            ws_settings: None,
            http_upgrade_settings: None,
        };

        let tuic_inbound = Inbound {
            id: 0,
            node_id,
            tag: format!("tuic-{}", node_id),
            protocol: "tuic".to_string(),
            listen_port: 9443,
            listen_ip: "0.0.0.0".to_string(),
            settings: tuic_json,
            stream_settings: serde_json::to_string(&tuic_stream)?,
            remark: None,
            enable: true,
            last_rotated_at: None,
            created_at: None,
        };
        self.node_repo.upsert_inbound(&tuic_inbound).await?;
            
        Ok(())
    }

    pub async fn reset_inbounds(&self, node_id: i64) -> anyhow::Result<()> {
        // Delete existing inbounds to force regeneration with fresh keys
        sqlx::query("DELETE FROM inbounds WHERE node_id = ?")
            .bind(node_id)
            .execute(&self.pool)
            .await?;
            
        // Recreate default inbounds with fresh keys
        self.init_default_inbounds(node_id).await
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
        let mut inbounds = self.node_repo.get_inbounds_by_node(node_id).await?;

        // Dynamic SNI Override (for Auto Rotation)
        if let Some(new_sni) = &node.reality_sni {
            use crate::models::network::StreamSettings;
            
            for inbound in &mut inbounds {
                // Try to parse stream settings
                if let Ok(mut stream) = serde_json::from_str::<StreamSettings>(&inbound.stream_settings) {
                    // Check if Reality is enabled
                    if let Some(reality) = &mut stream.reality_settings {
                        info!("🔄 Applying Dynamic SNI for inbound {}: {}", inbound.tag, new_sni);
                        reality.server_names = vec![new_sni.clone()];
                        reality.dest = format!("{}:443", new_sni);
                        
                        // Serialize back to inbound.stream_settings
                        if let Ok(new_json) = serde_json::to_string(&stream) {
                            inbound.stream_settings = new_json;
                        }
                    }
                }
            }
        }

        info!("Step 3: Injecting users for {} inbounds", inbounds.len());
        // 3. For each inbound, inject authorized users
        for inbound in &mut inbounds {
            // Find plans linked to this inbound
            // Find plans linked to this inbound OR to the parent node Generally
            let linked_plans = self.node_repo.get_linked_plans(node.id, inbound.id).await?;

            if linked_plans.is_empty() {
                continue;
            }

    
        let active_subs: Vec<(Option<String>, i64, Option<String>)> = self.store_service.get_active_subs_by_plans(&linked_plans).await?;
            
        info!("Found {} active subscriptions for inbound {}", active_subs.len(), inbound.tag);

        use crate::models::network::{InboundType, VlessClient, Hysteria2User};

        match serde_json::from_str::<InboundType>(&inbound.settings) {
            Ok(mut settings) => {
                match &mut settings {
                    InboundType::Vless(vless) => {
                        for sub in &active_subs {
                            if let (Some(uuid), tg_id, username) = (&sub.0, sub.1, &sub.2) {
                                let auth_name = tg_id.to_string();
                                let _display_name = username.clone().unwrap_or_default().replace("@", "");

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
                            if let (Some(uuid), tg_id, _) = (&sub.0, sub.1, &sub.2) {
                                let auth_name = tg_id.to_string();
                                
                                info!("🔑 Injecting HYSTERIA user: {} (Pass: {})", auth_name, uuid);
                                hy2.users.push(Hysteria2User {
                                    name: auth_name,
                                    password: uuid.replace("-", ""),
                                });
                            }
                        }
                    },
                    InboundType::AmneziaWg(awg) => {
                        use crate::models::network::AmneziaWgUser;
                        for sub in &active_subs {
                            if let (Some(uuid), tg_id, _) = (&sub.0, sub.1, &sub.2) {
                                let auth_name = tg_id.to_string();
                                
                                let client_priv = self.derive_awg_key(uuid);
                                let client_pub = self.priv_to_pub(&client_priv);
                                
                                info!("🔑 Injecting AMNEZIAWG user: {} (Public: {})", auth_name, client_pub);
                                awg.users.push(AmneziaWgUser {
                                    name: auth_name,
                                    private_key: client_priv,
                                    public_key: client_pub,
                                    preshared_key: None,
                                    client_ip: format!("10.10.0.{}", (tg_id % 250) + 2),
                                });
                            }
                        }
                    },
                InboundType::Trojan(trojan) => {
                        use crate::models::network::TrojanClient;
                        for sub in &active_subs {
                            if let (Some(uuid), tg_id, _) = (&sub.0, sub.1, &sub.2) {
                                let auth_name = tg_id.to_string();
                                trojan.clients.push(TrojanClient {
                                    password: uuid.clone(),
                                    email: auth_name,
                                });
                            }
                        }
                    },
                    InboundType::Tuic(tuic) => {
                         for sub in &active_subs {
                            if let (Some(uuid), tg_id, _) = (&sub.0, sub.1, &sub.2) {
                                let auth_name = tg_id.to_string();
                                
                                info!("🔑 Injecting TUIC user: {} (UUID: {})", auth_name, uuid);
                                tuic.users.push(crate::models::network::TuicUser {
                                    name: auth_name,
                                    uuid: uuid.clone(),
                                    password: uuid.replace("-", ""), 
                                });
                            }
                        }
                    }
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
        // 4. Generate Config
        let config = ConfigGenerator::generate_config(
            &node,
            inbounds
        );
        
        info!("Config generation successful for node {}", node_id);
        Ok((node, serde_json::to_value(&config)?))
    }

    /// Get all nodes (for admin UI)
    pub async fn get_all_nodes(&self) -> anyhow::Result<Vec<Node>> {
        self.node_repo.get_all_nodes().await.map_err(|e| e.into())
    }

    pub async fn notify_node_update(&self, node_id: i64) -> anyhow::Result<()> {
        info!("🔔 Node {} update notification triggered", node_id);
        // TODO: Implement actual notification logic (e.g. PubSub or direct push)
        Ok(())
    }

    pub async fn get_node_by_id(&self, node_id: i64) -> anyhow::Result<Node> {
        self.node_repo.get_node_by_id(node_id).await?
            .ok_or_else(|| anyhow::anyhow!("Node not found"))
    }

    pub async fn create_node(&self, name: &str, ip: &str, vpn_port: i32, auto_configure: bool) -> anyhow::Result<i64> {
        let token = uuid::Uuid::new_v4().to_string();
        
        // Handle pending IP with unique placeholder if empty
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
        };

        let id = self.node_repo.create_node(&node).await?;

        // Initialize default inbounds
        if let Err(e) = self.init_default_inbounds(id).await {
            error!("Failed to initialize inbounds for node {}: {}", id, e);
        }

        Ok(id)
    }

    pub async fn update_node(&self, id: i64, name: &str, ip: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE nodes SET name = ?, ip = ? WHERE id = ?")
            .bind(name)
            .bind(ip)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_node_enable(&self, id: i64) -> anyhow::Result<()> {
        self.node_repo.toggle_enabled(id).await?;
        Ok(())
    }

    pub async fn activate_node(&self, id: i64) -> anyhow::Result<()> {
        self.node_repo.update_status(id, "active").await.map_err(|e| e.into())
    }

    pub async fn delete_node(&self, id: i64) -> anyhow::Result<()> {
        // Manual Cleanup for non-cascading relations
        
        // Clear SNI Logs
        let _ = sqlx::query("DELETE FROM sni_rotation_log WHERE node_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await;

        // Unlink Subscriptions
        let _ = sqlx::query("UPDATE subscriptions SET node_id = NULL WHERE node_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await;

        // Delete the node (Cascades to inbounds)
        self.node_repo.delete_node(id).await.map_err(|e| e.into())
    }

    /// Derives a stable X25519 private key from a UUID string
    fn derive_awg_key(&self, uuid: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(uuid.as_bytes());
        hasher.update(b"amneziawg-key-salt");
        let result = hasher.finalize();
        
        let mut key = [0u8; 32];
        key.copy_from_slice(&result[..32]);
        
        // Clamp the key to be a valid X25519 private key
        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;
        
        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, key)
    }

    /// Converts an X25519 private key (base64) to a public key (base64)
    fn priv_to_pub(&self, priv_b64: &str) -> String {
        use x25519_dalek::{StaticSecret, PublicKey};
        
        let priv_bytes = base64::Engine::decode(&base64::prelude::BASE64_STANDARD, priv_b64).unwrap_or_default();
        if priv_bytes.len() != 32 {
            return "".to_string();
        }
        
        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(&priv_bytes);
        
        let secret = StaticSecret::from(key_arr);
        let public = PublicKey::from(&secret);
        
        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, public.as_bytes())
    }
}
