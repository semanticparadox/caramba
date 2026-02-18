use sqlx::PgPool;
use tracing::{info, error, warn};
use std::sync::Arc;


// Removed unused Subscription import
use caramba_db::models::node::Node;
use crate::singbox::{ConfigGenerator, RelayAuthMode};
use crate::services::store_service::StoreService;


use caramba_db::repositories::node_repo::NodeRepository;
use base64::Engine;

// Added import
use crate::services::pubsub_service::PubSubService;

#[derive(Debug, Clone)]
pub struct OrchestrationService {
    pub pool: PgPool,
    pub node_repo: NodeRepository,
    security_service: Arc<crate::services::security_service::SecurityService>,
    store_service: Arc<StoreService>,
    pubsub_service: Arc<PubSubService>,
}

impl OrchestrationService {
    pub fn new(
        pool: PgPool, 
        store_service: Arc<StoreService>,
        security_service: Arc<crate::services::security_service::SecurityService>,
        pubsub_service: Arc<PubSubService>,
    ) -> Self {
        let node_repo = NodeRepository::new(pool.clone());
        Self { 
            pool, 
            node_repo, 
            store_service, 
            security_service,
            pubsub_service,
        }
    }
    
    // ... (init_default_inbounds and other methods remain unchanged)

    pub async fn notify_node_update(&self, node_id: i64) -> anyhow::Result<()> {
        info!("🔔 Node {} update notification triggered", node_id);
        
        // Publish to Redis channel node_events:{node_id}
        // The payload is arbitrary JSON, matching what poll_updates expects (or just a kick)
        // poll_updates converts any message to {"update": true}
        
        let channel = format!("node_events:{}", node_id);
        let payload = serde_json::json!({ "update": true }).to_string();
        
        if let Err(e) = self.pubsub_service.publish(&channel, &payload).await {
            error!("Failed to publish node update for {}: {}", node_id, e);
            // Don't fail the request, just log error? 
            // Better to return error if critical, but update triggers are often best-effort.
            // Let's return error to be safe so caller knows it failed.
            return Err(e);
        }
        
        info!("✅ PubSub: Published update signal to {}", channel);
        Ok(())
    }
    /// Initializes default inbounds by applying Group Templates
    pub async fn init_default_inbounds(&self, node_id: i64) -> anyhow::Result<()> {
        info!("Initializing inbounds for node {} via templates", node_id);
        
        // Fetch node for SNI defaults
        let node = self.node_repo.get_node_by_id(node_id).await?
            .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
            
        // 1. Get Node's Groups
        let group_ids: Vec<i64> = sqlx::query_scalar("SELECT group_id FROM node_group_members WHERE node_id = $1")
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
            let group_templates = self.node_repo.get_templates_for_group(*gid).await?;
            templates.extend(group_templates);
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
        let vless_stream = r#"{"network":"tcp","security":"reality","reality_settings":{"show":false,"xver":0,"dest":"drive.google.com:443","server_names":["drive.google.com"],"private_key":"","short_ids":[""]}}"#;
        self.create_template("VLESS Reality", "vless", vless_settings, vless_stream, group_id, 10000).await?;

        Ok(())
    }

    async fn create_template(&self, name: &str, protocol: &str, settings: &str, stream: &str, group_id: i64, port: i64) -> anyhow::Result<i64> {
        let id: i64 = sqlx::query_scalar("INSERT INTO inbound_templates (name, protocol, settings_template, stream_settings_template, target_group_id, port_range_start, port_range_end, is_active, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, CURRENT_TIMESTAMP) RETURNING id")
            .bind(name)
            .bind(protocol)
            .bind(settings)
            .bind(stream)
            .bind(group_id)
            .bind(port)
            .bind(port) 
            .fetch_one(&self.pool)
            .await?;
        Ok(id)
    }


    async fn instantiate_inbound_from_template(&self, node: &Node, template: &caramba_db::models::groups::InboundTemplate) -> anyhow::Result<()> {
        info!("Instantiating template '{}' for node {}", template.name, node.id);
        
        // 0. Placeholder Replacement
        // 0.1 Allocate Port if needed (Dynamic Port Allocation)
        let mut port = template.port_range_start;
        if template.port_range_end > template.port_range_start {
            if let Ok(new_port) = self.allocate_port(node.id, template.port_range_start, template.port_range_end).await {
                port = new_port;
            }
        }

        // 0.2 Smart SNI selection
        let best_sni = self.security_service.get_best_sni_for_node(node.id).await.unwrap_or_else(|_| "www.google.com".to_string());
        
        let sni = if best_sni == "www.google.com" || best_sni == "drive.google.com" {
             node.reality_sni.as_deref()
                .or(node.domain.as_deref())
                .unwrap_or(&best_sni)
        } else {
            &best_sni
        };

        let domain = node.domain.as_deref().unwrap_or("");
        let pbk = node.reality_pub.as_deref().unwrap_or("");
        let sid = node.short_id.as_deref().unwrap_or("");

        let mut settings_json = template.settings_template.clone();
        let mut stream_json = template.stream_settings_template.clone();

        settings_json = settings_json
            .replace("{{SNI}}", sni)
            .replace("{{port}}", &port.to_string())
            .replace("{{DOMAIN}}", domain)
            .replace("{{REALITY_PBK}}", pbk)
            .replace("{{REALITY_SID}}", sid);

        stream_json = stream_json
            .replace("{{SNI}}", sni)
            .replace("{{port}}", &port.to_string())
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
             
             if let Ok(mut stream_obj) = serde_json::from_str::<caramba_db::models::network::StreamSettings>(&stream_json) {
                 if let Some(reality) = &mut stream_obj.reality_settings {
                     reality.private_key = pkey;
                     reality.public_key = Some(pubkey);
                     reality.short_ids = vec![sid];
                 }
                 stream_json = serde_json::to_string(&stream_obj)?;
             }
        } else if template.protocol == "naive" {
             // Inject Reality Keys for Naive
             let node_updated = self.node_repo.get_node_by_id(node.id).await?.unwrap(); 
             if let (Some(pkey), Some(pubkey), Some(sid)) = (node_updated.reality_priv, node_updated.reality_pub, node_updated.short_id) {
                 if let Ok(mut stream_obj) = serde_json::from_str::<caramba_db::models::network::StreamSettings>(&stream_json) {
                     if let Some(reality) = &mut stream_obj.reality_settings {
                         reality.private_key = pkey;
                         reality.public_key = Some(pubkey);
                         reality.short_ids = vec![sid];
                     }
                     stream_json = serde_json::to_string(&stream_obj)?;
                 }
             }
        } else if template.protocol == "amneziawg" {
            let (priv_key, pub_key) = self.generate_wireguard_keys()?;
            let (jc, jmin, jmax, s1, s2, h1, h2, h3, h4) = self.generate_awg_params();
            
            if let Ok(mut awg_obj) = serde_json::from_str::<caramba_db::models::network::AmneziaWgSettings>(&settings_json) {
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

        let inbound = caramba_db::models::network::Inbound {
            id: 0,
            node_id: node.id,
            tag: format!("tpl_{}", template.name.to_lowercase().replace(' ', "_")),
            protocol: template.protocol.clone(),
            listen_port: port,
            listen_ip: "0.0.0.0".to_string(),
            settings: settings_json,
            stream_settings: stream_json,
            remark: Some(template.name.clone()), 
            enable: true,
            renew_interval_mins: template.renew_interval_mins,
            port_range_start: template.port_range_start,
            port_range_end: template.port_range_end,
            last_rotated_at: None,
            created_at: None,
        };
        
        self.node_repo.upsert_inbound(&inbound).await?;
        Ok(())
    }

    fn generate_reality_keys(&self) -> anyhow::Result<(String, String, String)> {
        use x25519_dalek::{StaticSecret, PublicKey};
        
        // Generate 32 random bytes for the private key
        let bytes = rand::random::<[u8; 32]>();
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        
        // Use URL_SAFE_NO_PAD engine for sing-box 1.12+ compatibility
        let priv_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret.to_bytes()).trim().to_string();
        let pub_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.as_bytes()).trim().to_string();
        
        let short_id = hex::encode(&rand::random::<[u8; 8]>()).trim().to_string();
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

    pub async fn allocate_port(&self, node_id: i64, start: i64, end: i64) -> anyhow::Result<i64> {
        // Find used ports
        let used_ports: Vec<i64> = sqlx::query_scalar(
            "SELECT listen_port FROM inbounds WHERE node_id = $1"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        // Use rand 0.9 rng
        use rand::Rng;
        let mut rng = rand::rng();
        
        for _ in 0..100 {
            let p = rng.random_range(start..=end);
            if !used_ports.contains(&p) {
                return Ok(p);
            }
        }
        
        Err(anyhow::anyhow!("Failed to allocate port for node {} in range {}-{}", node_id, start, end))
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
        info!("RESETTING inbounds for node {}", node_id);
        
        // Manual Cascade: Delete linked plan_inbounds first
        // This ensures that even if SQLite Foreign Key support is disabled/broken, we don't get constraint errors
        sqlx::query("DELETE FROM plan_inbounds WHERE inbound_id IN (SELECT id FROM inbounds WHERE node_id = $1)")
            .bind(node_id)
            .execute(&self.pool)
            .await?;

        // Delete existing inbounds to force regeneration with fresh keys
        sqlx::query("DELETE FROM inbounds WHERE node_id = $1")
            .bind(node_id)
            .execute(&self.pool)
            .await?;
            
        // Recreate default inbounds with fresh keys
        self.init_default_inbounds(node_id).await
    }

    /// Generates Node Config JSON without applying it (Internal)
    pub async fn generate_node_config_json(&self, node_id: i64) -> anyhow::Result<(caramba_db::models::node::Node, serde_json::Value)> {
        info!("Step 1: Fetching node details for ID: {}", node_id);
        // 1. Fetch node details
        let node: Node = sqlx::query_as("SELECT * FROM nodes WHERE id = $1")
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
        info!("Step 2: Found {} inbounds for node {} ({})", inbounds.len(), node_id, node.name);

        // 2.5 Lazy Initialization & Key Validation/Scrubbing
        let mut node = node;
        let mut needs_node_update = false;
        
        let current_priv = node.reality_priv.as_deref().unwrap_or("").trim().to_string();
        
        // If it was just whitespace, we need to update it immediately to prevent byte 0 errors
        if node.reality_priv.as_deref().unwrap_or("") != current_priv {
            node.reality_priv = Some(current_priv.clone());
            needs_node_update = true;
        }

        let is_invalid_key = current_priv.is_empty() || 
                             current_priv.len() < 43 || 
                             base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&current_priv).is_err();

        if is_invalid_key {
             // Check if any inbound uses reality
             let has_reality = inbounds.iter().any(|i| i.stream_settings.contains("reality"));
             if has_reality {
                 info!("✨ Lazily generating or HARD-FIXING Reality keys for node {}", node.id);
                 if let Ok((priv_key, pub_key, short_id)) = self.generate_reality_keys() {
                     node.reality_priv = Some(priv_key);
                     node.reality_pub = Some(pub_key);
                     node.short_id = Some(short_id);
                     needs_node_update = true;
                 }
             }
        }
        
        if needs_node_update {
            self.node_repo.update_node(&node).await?;
        }

        // We do this for ALL inbounds to ensure no {{placeholders}} leak into the generator
        if let Some(_new_sni) = &node.reality_sni {
             // ... existing SNI logic ...
        }

        let pkey = node.reality_priv.as_deref().unwrap_or("").trim();
        let pubkey = node.reality_pub.as_deref().unwrap_or("").trim();
        let sid = node.short_id.as_deref().unwrap_or("").trim();
        let domain = node.domain.as_deref().unwrap_or("");
        
        for inbound in &mut inbounds {
            // runtime replacement of leftovers
            if inbound.stream_settings.contains("{{") {
                inbound.stream_settings = inbound.stream_settings
                    .replace("{{reality_private}}", pkey)
                    .replace("{{REALITY_PBK}}", pubkey)
                    .replace("{{REALITY_SID}}", sid)
                    .replace("{{DOMAIN}}", domain);
                    
                if let Some(sni) = &node.reality_sni {
                     inbound.stream_settings = inbound.stream_settings.replace("{{sni}}", sni);
                } else {
                     // Best effort fallback
                     inbound.stream_settings = inbound.stream_settings.replace("{{sni}}", "www.google.com");
                }
            }

            // Also check settings for UUID placeholders just in case
            // ...
        
            // Try to parse stream settings for specific logic
            if let Ok(mut stream) = serde_json::from_str::<caramba_db::models::network::StreamSettings>(&inbound.stream_settings) {
                 // Check if Reality is enabled and SNI override is needed
                 if let Some(reality) = &mut stream.reality_settings {
                     // If we have a specific SNI set on the node, enforce it
                     if let Some(new_sni) = &node.reality_sni {
                         reality.server_names = vec![new_sni.clone()];
                         reality.dest = format!("{}:443", new_sni);
                     }
                     // Ensure keys are present if they were missing or empty
                     if reality.private_key.len() < 10 && !pkey.is_empty() {
                         reality.private_key = pkey.to_string();
                         reality.public_key = Some(pubkey.to_string());
                         reality.short_ids = vec![sid.to_string()];
                     }
                     
                     if let Ok(new_json) = serde_json::to_string(&stream) {
                        inbound.stream_settings = new_json;
                     }
                 }
            }
        }

        info!("Step 3: Injecting users for {} inbounds", inbounds.len());
        for inbound in &mut inbounds {
            if !inbound.enable {
                warn!("⚠️ Inbound {} is DISABLED, skipping user injection", inbound.tag);
                continue;
            }

            // Find plans linked to this inbound
            let linked_plans = self.node_repo.get_linked_plans(node.id, inbound.id).await?;

            if linked_plans.is_empty() {
                warn!("⚠️ Inbound {} has NO linked plans for node {}, users will be empty", inbound.tag, node_id);
            }
            
            // Fetch users only if we have plans
            let active_subs = if linked_plans.is_empty() {
                Vec::new()
            } else {
                self.store_service.get_active_subs_by_plans(&linked_plans).await.unwrap_or_default()
            };
            
            info!("Found {} active subscriptions for inbound {}", active_subs.len(), inbound.tag);

        use caramba_db::models::network::{InboundType, VlessClient, Hysteria2User, NaiveUser};

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
                            if let (sub_id, Some(uuid), _tg_id, _username) = (sub.0, &sub.1, sub.2, &sub.3) {
                                // Use user_{sub_id} to match TrafficService expectation
                                let auth_name = format!("user_{}", sub_id);

                                info!("🔑 Injecting VLESS user: {} (UUID: {})", auth_name, uuid);
                                // Parse stream settings to check for TCP + Reality/TLS
                                let stream_json: serde_json::Value = serde_json::from_str(&inbound.stream_settings).unwrap_or(serde_json::Value::Null);
                                let network = stream_json.get("network").and_then(|v| v.as_str()).unwrap_or("");
                                let security = stream_json.get("security").and_then(|v| v.as_str()).unwrap_or("");

                                vless.clients.push(VlessClient {
                                    id: uuid.clone(),
                                    email: auth_name,
                                    // Only apply flow for TCP + REALITY/TLS
                                    flow: if network == "tcp" && (security == "reality" || security == "tls") {
                                        "xtls-rprx-vision".to_string()
                                    } else {
                                        "".to_string()
                                    },
                                });
                            }
                        }
                    },
                    InboundType::Hysteria2(hy2) => {
                         for sub in &active_subs {
                            if let (sub_id, Some(uuid), _, _) = (sub.0, &sub.1, sub.2, &sub.3) {
                                let auth_name = format!("user_{}", sub_id);
                                
                                info!("🔑 Injecting HYSTERIA user: {} (Pass: {})", auth_name, uuid);
                                hy2.users.push(Hysteria2User {
                                    name: Some(auth_name),
                                    password: uuid.replace("-", ""),
                                });
                            }
                        }
                    },
                    InboundType::AmneziaWg(awg) => {
                        use caramba_db::models::network::AmneziaWgUser;
                        for sub in &active_subs {
                            if let (sub_id, Some(uuid), tg_id, _) = (sub.0, &sub.1, sub.2, &sub.3) {
                                let auth_name = format!("user_{}", sub_id);
                                
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
                        use caramba_db::models::network::TrojanClient;
                        for sub in &active_subs {
                            if let (sub_id, Some(uuid), _, _) = (sub.0, &sub.1, sub.2, &sub.3) {
                                let auth_name = format!("user_{}", sub_id);
                                trojan.clients.push(TrojanClient {
                                    password: uuid.clone(),
                                    email: Some(auth_name),
                                });
                            }
                        }
                    },
                    InboundType::Tuic(tuic) => {
                         for sub in &active_subs {
                            if let (sub_id, Some(uuid), _, _) = (sub.0, &sub.1, sub.2, &sub.3) {
                                let auth_name = format!("user_{}", sub_id);
                                
                                info!("🔑 Injecting TUIC user: {} (UUID: {})", auth_name, uuid);
                                tuic.users.push(caramba_db::models::network::TuicUser {
                                    name: Some(auth_name),
                                    uuid: uuid.clone(),
                                    password: uuid.replace("-", ""), 
                                });
                            }
                        }
                    },
                    InboundType::Naive(naive) => {
                         for sub in &active_subs {
                            if let (sub_id, Some(uuid), _, _) = (sub.0, &sub.1, sub.2, &sub.3) {
                                let auth_name = format!("user_{}", sub_id);
                                
                                info!("🔑 Injecting NAIVE user: {} (Pass: {})", auth_name, uuid);
                                naive.users.push(NaiveUser {
                                    username: auth_name,
                                    password: uuid.replace("-", ""),
                                });
                            }
                        }
                    },
                    InboundType::Shadowsocks(ss) => {
                         for sub in &active_subs {
                            if let (sub_id, Some(uuid), _, _) = (sub.0, &sub.1, sub.2, &sub.3) {
                                let auth_name = format!("user_{}", sub_id);
                                
                                info!("🔑 Injecting SHADOWSOCKS user: {} (Pass: {})", auth_name, uuid);
                                ss.users.push(caramba_db::models::network::ShadowsocksUser {
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

        // 3.5 Fetch Relay Logic Context
        let mut relay_target_node: Option<Node> = None;
        let mut relay_target_inbound: Option<caramba_db::models::network::Inbound> = None;
        if let Some(target_id) = node.relay_id {
            if node.is_relay {
                 relay_target_node = self.node_repo.get_node_by_id(target_id).await.unwrap_or(None);
                 if relay_target_node.is_some() {
                     let mut target_inbounds = self
                         .node_repo
                         .get_inbounds_by_node(target_id)
                         .await
                         .unwrap_or_default();
                     target_inbounds.sort_by_key(|i| i.listen_port);
                     relay_target_inbound = target_inbounds
                         .into_iter()
                         .find(|i| i.enable && i.protocol.eq_ignore_ascii_case("shadowsocks"));
                 }
            }
        }

        let relay_clients = self.node_repo.get_relay_clients(node.id).await.unwrap_or_default();
        if !relay_clients.is_empty() {
            info!("Context: Node {} has {} relay clients", node.id, relay_clients.len());
        }

        let relay_auth_mode_raw = self
            .store_service
            .get_setting("relay_auth_mode")
            .await
            .unwrap_or(None);
        let relay_auth_mode = RelayAuthMode::from_setting(relay_auth_mode_raw.as_deref());

        info!("Step 4: generating final sing-box config JSON");
        // 4. Generate Config
        let config = ConfigGenerator::generate_config(
            &node,
            inbounds,
            relay_target_node,
            relay_target_inbound,
            relay_clients,
            relay_auth_mode,
        );
        
        // Validate Config
        // This ensures we never serve a broken configuration to a node
        // If validation fails, we return an error, which should result in a 500/Retaining old config on client side
        if let Err(e) = ConfigGenerator::validate_config(&config) {
            error!("❌ Generated config for node {} FAILED VALIDATION: {}", node_id, e);
            return Err(e);
        }
        info!("✅ Config validation passed for node {}", node_id);
        
        info!("Config generation successful for node {}", node_id);
        Ok((node, serde_json::to_value(&config)?))
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
