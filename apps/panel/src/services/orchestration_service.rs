use sqlx::SqlitePool;
use tracing::{info, error};
use std::sync::Arc;


// Removed unused Subscription import
use crate::models::node::Node;
use crate::singbox::{ConfigGenerator};
use crate::services::store_service::StoreService;


use crate::repositories::node_repo::NodeRepository;

#[derive(Debug, Clone)]
pub struct OrchestrationService {
    pub pool: SqlitePool,
    pub node_repo: NodeRepository,
    _infrastructure_service: Arc<crate::services::infrastructure_service::InfrastructureService>,
    store_service: Arc<StoreService>,
}

impl OrchestrationService {
    pub fn new(
        pool: SqlitePool, 
        store_service: Arc<StoreService>,
        infrastructure_service: Arc<crate::services::infrastructure_service::InfrastructureService>,
    ) -> Self {
        let node_repo = NodeRepository::new(pool.clone());
        Self { pool, node_repo, store_service, _infrastructure_service: infrastructure_service }
    }
    /// Initializes default inbounds by applying Group Templates
    pub async fn init_default_inbounds(&self, node_id: i64) -> anyhow::Result<()> {
        info!("Initializing inbounds for node {} via templates", node_id);
        
        // Fetch node for SNI defaults
        let node = self.node_repo.get_node_by_id(node_id).await?
            .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
            
        // 1. Get Node's Groups
        let group_ids: Vec<i64> = sqlx::query_scalar("SELECT group_id FROM node_group_members WHERE node_id = ?")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await?;
            
        if group_ids.is_empty() {
             info!("Node {} has no groups, skipping template application", node_id);
             return Ok(());
        }

        // 2. Fetch Templates for these groups (Active Only)
        let mut templates = Vec::new();
        for gid in &group_ids {
            let mut group_templates = self.node_repo.get_templates_for_group(*gid).await?;
            templates.append(&mut group_templates);
        }
        
        // 3. Bootstrap Defaults if NO templates found (Fresh Install Scenario)
        if templates.is_empty() {
            info!("No templates found for node groups. Bootstrapping default templates for 'Default' group...");
            // Find the Default Group ID
            let default_group = self.node_repo.get_group_by_name("Default").await?;
            if let Some(group) = default_group {
                self.bootstrap_default_templates(group.id).await?;
                // Re-fetch templates
                templates = self.node_repo.get_templates_for_group(group.id).await?;
            }
        }
        
        // 4. Instantiate Inbounds from Templates
        for template in templates {
            self.instantiate_inbound_from_template(&node, &template).await?;
        }
        
        // Link to default plan (ID=1) - for legacy compatibility
        self.node_repo.link_node_inbounds_to_plan(1, node_id).await?;

        Ok(())
    }

    /// Helper to bootstrap default templates
    async fn bootstrap_default_templates(&self, group_id: i64) -> anyhow::Result<()> {
        // VLESS Reality
        let vless_settings = r#"{"clients":[],"decryption":"none"}"#;
        let vless_stream = r#"{"network":"tcp","security":"reality","reality_settings":{"show":false,"xver":0,"dest":"drive.google.com:443","server_names":["drive.google.com"]}}"#;
        self.create_template("VLESS Reality", "vless", vless_settings, vless_stream, group_id, 10000).await?;

        // Hysteria 2
        let hy2_settings = r#"{"users":[],"up_mbps":100,"down_mbps":100,"masquerade":"file:///opt/exarobot/apps/panel/assets/masquerade"}"#;
        let hy2_stream = r#"{"network":"udp","security":"tls","tls_settings":{"server_name":"drive.google.com"}}"#;
        self.create_template("Hysteria 2", "hysteria2", hy2_settings, hy2_stream, group_id, 8443).await?;

        // TUIC
        let tuic_settings = r#"{"users":[],"congestion_control":"cubic","auth_timeout":"3s","heartbeat":"10s"}"#;
        let tuic_stream = r#"{"network":"udp","security":"tls","tls_settings":{"server_name":"www.google.com"}}"#;
        self.create_template("TUIC v5", "tuic", tuic_settings, tuic_stream, group_id, 9443).await?;

        // AmneziaWG
        let awg_settings = r#"{"users":[],"listen_port":51820}"#; // Key generation happens on instantiation
        self.create_template("AmneziaWG", "amneziawg", awg_settings, "{}", group_id, 51820).await?;

        Ok(())
    }

    async fn create_template(&self, name: &str, protocol: &str, settings: &str, stream: &str, group_id: i64, port: i64) -> anyhow::Result<i64> {
        let res = sqlx::query("INSERT INTO inbound_templates (name, protocol, settings_template, stream_settings_template, target_group_id, port_range_start, port_range_end, is_active, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 1, CURRENT_TIMESTAMP)")
            .bind(name)
            .bind(protocol)
            .bind(settings)
            .bind(stream)
            .bind(group_id)
            .bind(port)
            .bind(port) 
            .execute(&self.pool)
            .await?;
        Ok(res.last_insert_rowid())
    }

    async fn instantiate_inbound_from_template(&self, node: &Node, template: &crate::models::groups::InboundTemplate) -> anyhow::Result<()> {
        info!("Instantiating template '{}' for node {}", template.name, node.id);
        
        let port = template.port_range_start; 

        let mut settings_json = template.settings_template.clone();
        let mut stream_json = template.stream_settings_template.clone();

        // 0. Placeholder Replacement
        let sni = node.reality_sni.as_deref()
            .or(node.domain.as_deref())
            .unwrap_or("");
        let domain = node.domain.as_deref().unwrap_or("");
        let pbk = node.reality_pub.as_deref().unwrap_or("");
        let sid = node.short_id.as_deref().unwrap_or("");

        settings_json = settings_json
            .replace("{{SNI}}", sni)
            .replace("{{DOMAIN}}", domain)
            .replace("{{REALITY_PBK}}", pbk)
            .replace("{{REALITY_SID}}", sid);

        stream_json = stream_json
            .replace("{{SNI}}", sni)
            .replace("{{DOMAIN}}", domain)
            .replace("{{REALITY_PBK}}", pbk)
            .replace("{{REALITY_SID}}", sid);

        if template.protocol == "vless" {
             // Generate Reality Keys
             let (priv_key, pub_key, short_id) = self.generate_reality_keys()?;
             
             if node.reality_priv.is_none() {
                 let mut updated_node = node.clone();
                 updated_node.reality_priv = Some(priv_key.clone());
                 updated_node.reality_pub = Some(pub_key.clone());
                 updated_node.short_id = Some(short_id.clone());
                 self.node_repo.update_node(&updated_node).await?;
             }
             
             let node_updated = self.node_repo.get_node_by_id(node.id).await?.unwrap(); 
             let pkey = node_updated.reality_priv.unwrap_or_default();
             let pubkey = node_updated.reality_pub.unwrap_or_default();
             let sid = node_updated.short_id.unwrap_or_default();
             
             if let Ok(mut stream_obj) = serde_json::from_str::<crate::models::network::StreamSettings>(&stream_json) {
                 if let Some(reality) = &mut stream_obj.reality_settings {
                     reality.private_key = pkey;
                     reality.public_key = Some(pubkey);
                     reality.short_ids = vec![sid];
                 }
                 stream_json = serde_json::to_string(&stream_obj)?;
             }
        } else if template.protocol == "amneziawg" {
            let (priv_key, pub_key) = self.generate_wireguard_keys()?;
            let (jc, jmin, jmax, s1, s2, h1, h2, h3, h4) = self.generate_awg_params();
            
            if let Ok(mut awg_obj) = serde_json::from_str::<crate::models::network::AmneziaWgSettings>(&settings_json) {
                awg_obj.private_key = priv_key;
                awg_obj.public_key = pub_key;
                awg_obj.jc = jc;
                awg_obj.jmin = jmin;
                awg_obj.jmax = jmax;
                awg_obj.s1 = s1;
                awg_obj.s2 = s2;
                awg_obj.h1 = h1;
                awg_obj.h2 = h2;
                awg_obj.h3 = h3;
                awg_obj.h4 = h4;
                settings_json = serde_json::to_string(&awg_obj)?;
            }
        }

        let inbound = crate::models::network::Inbound {
            id: 0,
            node_id: node.id,
            tag: format!("tpl_{}", template.id),
            protocol: template.protocol.clone(),
            listen_port: port,
            listen_ip: "0.0.0.0".to_string(),
            settings: settings_json,
            stream_settings: stream_json,
            remark: Some(format!("Template: {}", template.name)), 
            enable: true,
            last_rotated_at: None,
            created_at: None,
        };
        
        self.node_repo.upsert_inbound(&inbound).await?;
        Ok(())
    }

    fn generate_reality_keys(&self) -> anyhow::Result<(String, String, String)> {
        use std::process::Command;
        
        let output = Command::new("sing-box")
            .args(&["generate", "reality-keypair"])
            .output()
            .map_err(|e| anyhow::anyhow!("sing-box generate error: {}", e))?;
            
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
        
        let short_id = hex::encode(&rand::random::<[u8; 8]>());
        Ok((priv_key, pub_key, short_id))
    }

    fn generate_wireguard_keys(&self) -> anyhow::Result<(String, String)> {
        use std::process::Command;
        let output = Command::new("sing-box")
            .args(&["generate", "wireguard-keypair"])
            .output()
            .map_err(|e| anyhow::anyhow!("sing-box awg generate error: {}", e))?;
            
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
        Ok((priv_key, pub_key))
    }

    fn generate_awg_params(&self) -> (u16, u16, u16, u16, u16, u32, u32, u32, u32) {
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

        use crate::models::network::{InboundType, VlessClient, Hysteria2User, NaiveUser};

        // Parse as Value first to handle missing 'protocol' tag in legacy/broken data
        let mut settings_value: serde_json::Value = serde_json::from_str(&inbound.settings).unwrap_or(serde_json::Value::Null);
        
        if let Some(obj) = settings_value.as_object_mut() {
            if !obj.contains_key("protocol") {
                // Determine protocol from inbound.protocol if missing in JSON
                // InboundType expects lowercase tags matching the protocol name
                obj.insert("protocol".to_string(), serde_json::Value::String(inbound.protocol.clone().to_lowercase()));
            }
        }

        match serde_json::from_value::<InboundType>(settings_value) {
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
                                    name: Some(auth_name),
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
                                    name: Some(auth_name),
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
                                    email: Some(auth_name),
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
                                    name: Some(auth_name),
                                    uuid: uuid.clone(),
                                    password: uuid.replace("-", ""), 
                                });
                            }
                        }
                    },
                    InboundType::Naive(naive) => {
                         for sub in &active_subs {
                            if let (Some(uuid), tg_id, _) = (&sub.0, sub.1, &sub.2) {
                                let auth_name = tg_id.to_string();
                                
                                info!("🔑 Injecting NAIVE user: {} (Pass: {})", auth_name, uuid);
                                naive.users.push(NaiveUser {
                                    username: auth_name,
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
        let config = ConfigGenerator::generate_config(
            &node,
            inbounds
        );
        
        info!("Config generation successful for node {}", node_id);
        Ok((node, serde_json::to_value(&config)?))
    }


    pub async fn notify_node_update(&self, node_id: i64) -> anyhow::Result<()> {
        info!("🔔 Node {} update notification triggered", node_id);
        // TODO: Implement actual notification logic (e.g. PubSub or direct push)
        Ok(())
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
