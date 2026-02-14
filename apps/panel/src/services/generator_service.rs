use sqlx::SqlitePool;
use anyhow::{Result, Context};
use crate::models::groups::InboundTemplate;
use crate::models::node::Node;
use tracing::{info, warn, error};
use std::sync::Arc;
use crate::services::security_service::SecurityService;
use crate::services::orchestration_service::OrchestrationService;

#[derive(Clone)]
pub struct GeneratorService {
    pool: SqlitePool,
    security_service: Arc<SecurityService>,
    orchestration_service: Arc<OrchestrationService>,
}

impl GeneratorService {
    pub fn new(
        pool: SqlitePool, 
        security_service: Arc<SecurityService>,
        orchestration_service: Arc<OrchestrationService>,
    ) -> Self {
        Self { 
            pool, 
            security_service,
            orchestration_service
        }
    }

    /// Syncs inbounds for all nodes in a specific group based on active templates.
    pub async fn sync_group_inbounds(&self, group_id: i64) -> Result<()> {
        // 1. Get Templates for this group
        let templates = sqlx::query_as::<_, InboundTemplate>(
            "SELECT * FROM inbound_templates WHERE target_group_id = ? AND is_active = 1"
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;

        if templates.is_empty() {
            warn!("⚠️ No active templates found for group {}", group_id);
            return Ok(());
        }

        // 2. Get Nodes in this group
        let nodes = sqlx::query_as::<_, Node>(
            r#"
            SELECT n.* FROM nodes n
            JOIN node_group_members ngm ON n.id = ngm.node_id
            WHERE ngm.group_id = ?
            "#
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;

        info!("🔄 Syncing {} templates to {} nodes in group {}", templates.len(), nodes.len(), group_id);

        for node in nodes {
            for template in &templates {
                match self.ensure_inbound_exists(&node, template).await {
                    Ok(_) => info!("✅ Inbound for template {} synced to node {} ({})", template.id, node.id, node.name),
                    Err(e) => error!("❌ Failed to sync template {} to node {}: {}", template.id, node.id, e),
                }
            }
        }

        Ok(())
    }

    /// Ensures a node has an inbound matching the template.
    /// Uses `tag` as a unique identifier (e.g. "template_{id}").
    async fn ensure_inbound_exists(&self, node: &Node, template: &InboundTemplate) -> Result<()> {
        let tag = format!("tpl_{}", template.id);

        let existing_inbound: Option<(i64, i64)> = sqlx::query_as(
            "SELECT id, listen_port FROM inbounds WHERE node_id = ? AND tag = ?"
        )
        .bind(node.id)
        .bind(&tag)
        .fetch_optional(&self.pool)
        .await?;

        // 3. Resolve Placeholders
        let mut settings = template.settings_template.clone();
        let mut stream_settings = template.stream_settings_template.clone();
        
        let new_uuid = uuid::Uuid::new_v4().to_string();
        settings = settings.replace("{{uuid}}", &new_uuid);
        
         if let Some((id, existing_port)) = existing_inbound {
            // Inbound exists. Update it to match template!
            // We preserve the existing port to avoid breaking clients.
            let port = existing_port;
            settings = settings.replace("{{port}}", &port.to_string());
            
             // Reality Key Generation / SNI Logic (Re-run to ensure consistency or updates)
             if stream_settings.contains("{{reality_private}}") {
                 if let Some(priv_key) = &node.reality_priv {
                     stream_settings = stream_settings.replace("{{reality_private}}", priv_key);
                 } else {
                     warn!("Node {} missing Reality keys for template", node.id);
                 }
             }
             
             // SNI
            if stream_settings.contains("{{pool_sni}}") {
                let pool_sni: Option<String> = sqlx::query_scalar(
                    "SELECT domain FROM sni_pool WHERE is_active = 1 ORDER BY RANDOM() LIMIT 1"
                )
                .fetch_optional(&self.pool)
                .await?;
                
                if let Some(sni) = pool_sni {
                    stream_settings = stream_settings.replace("{{pool_sni}}", &sni);
                } else {
                    let fallback = node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string());
                    stream_settings = stream_settings.replace("{{pool_sni}}", &fallback);
                }
            }
    
            if let Some(sni) = &node.reality_sni {
                stream_settings = stream_settings.replace("{{sni}}", sni);
            } else {
                stream_settings = stream_settings.replace("{{sni}}", "www.google.com");
            }

            // Update DB
            sqlx::query(
                "UPDATE inbounds SET protocol = ?, settings = ?, stream_settings = ?, remark = ?, enable = 1 WHERE id = ?"
            )
            .bind(&template.protocol)
            .bind(&settings)
            .bind(&stream_settings)
            .bind(&template.name)
            .bind(id)
            .execute(&self.pool)
            .await?;
            
            // info!("Updated inbound '{}' (ID {}) for node {}", template.name, id, node.id);
            return Ok(());
        }

        // New Inbound Logic
        let port = self.allocate_port(node.id, template.port_range_start, template.port_range_end).await?;
        settings = settings.replace("{{port}}", &port.to_string());
        
        // Reality Key Generation if needed (placeholder {{reality_private}})
        if stream_settings.contains("{{reality_private}}") {
             // Generate X25519 key (simplistic placeholder logic for now)
             // In real internal logic we use a helper. 
             // For now let's assume the node has keys or we generate new ones.
             if let Some(priv_key) = &node.reality_priv {
                 stream_settings = stream_settings.replace("{{reality_private}}", priv_key);
             } else {
                 // Fallback or generate new?
                 warn!("Node {} missing Reality keys for template", node.id);
             }
        }
        
        // SNI
        if stream_settings.contains("{{pool_sni}}") {
            let pool_sni: Option<String> = sqlx::query_scalar(
                "SELECT domain FROM sni_pool WHERE is_active = 1 ORDER BY RANDOM() LIMIT 1"
            )
            .fetch_optional(&self.pool)
            .await?;
            
            if let Some(sni) = pool_sni {
                stream_settings = stream_settings.replace("{{pool_sni}}", &sni);
            } else {
                // Fallback to node SNI if pool is empty
                let fallback = node.reality_sni.clone().unwrap_or_else(|| "www.google.com".to_string());
                stream_settings = stream_settings.replace("{{pool_sni}}", &fallback);
            }
        }

        if let Some(sni) = &node.reality_sni {
            stream_settings = stream_settings.replace("{{sni}}", sni);
        } else {
            stream_settings = stream_settings.replace("{{sni}}", "www.google.com");
        }

        // 4. Insert Inbound
        sqlx::query(
            r#"
            INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, remark, enable)
            VALUES (?, ?, ?, ?, ?, ?, ?, 1)
            "#
        )
        .bind(node.id)
        .bind(&tag)
        .bind(&template.protocol)
        .bind(port)
        .bind(&settings)
        .bind(&stream_settings)
        .bind(&template.name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn allocate_port(&self, node_id: i64, start: i64, end: i64) -> Result<i64> {
        // Find used ports
        let used_ports: Vec<i64> = sqlx::query_scalar(
            "SELECT listen_port FROM inbounds WHERE node_id = ?"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        // Simple random trial
        // In production, might want a smarter allocator
        use rand::Rng;
        let mut rng = rand::rng();
        
        for _ in 0..100 {
            let p = rng.random_range(start..=end);
            if !used_ports.contains(&p) {
                return Ok(p);
            }
        }
        
        Err(anyhow::anyhow!("Failed to allocate port for node {}", node_id))
    }

    pub async fn rotate_inbound(&self, inbound_id: i64) -> Result<()> {
        let inbound = sqlx::query_as::<_, crate::models::network::Inbound>("SELECT * FROM inbounds WHERE id = ?")
            .bind(inbound_id)
            .fetch_one(&self.pool)
            .await
            .context("Inbound not found")?;

        let node = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = ?")
            .bind(inbound.node_id)
            .fetch_one(&self.pool)
            .await
            .context("Node not found")?;

        // 1. Identify Template for base settings
        if !inbound.tag.starts_with("tpl_") {
             return Err(anyhow::anyhow!("Inbound is not tied to a template"));
        }
        // Tag could be tpl_name or tpl_id. Let's see how it was generated.
        // In orchestration_service, it was format!("tpl_{}", template.name.to_lowercase().replace(' ', "_"))
        // Wait, in generator_service it was format!("tpl_{}", template.id)
        // I should probably find the template by name or ID.
        // Let's use name since that's what's in remark.
        let template = if let Some(remark) = &inbound.remark {
             sqlx::query_as::<_, InboundTemplate>("SELECT * FROM inbound_templates WHERE name = ?")
                .bind(remark)
                .fetch_optional(&self.pool)
                .await?
        } else {
            None
        };

        let template = template.ok_or_else(|| anyhow::anyhow!("Template not found for rotation"))?;

        // 2. Allocate New Port
        let new_port = self.orchestration_service.allocate_port(inbound.node_id, inbound.port_range_start, inbound.port_range_end).await?;
        
        // 3. Select New SNI
        let new_sni = self.security_service.get_best_sni_for_node(node.id).await.unwrap_or_else(|_| "www.google.com".to_string());
        
        // 4. Re-interpolate settings
        let mut settings = template.settings_template.clone();
        let mut stream_settings = template.stream_settings_template.clone();

        // Placeholders
        let domain = node.domain.as_deref().unwrap_or("");
        let pbk = node.reality_pub.as_deref().unwrap_or("");
        let sid = node.short_id.as_deref().unwrap_or("");

        // Old settings might have clients we want to preserve? 
        // Or we regenerate for maximum rotation. 
        // User said "уникальный конфиг", so let's regenerate UUID too if placeholder exists.
        let new_uuid = uuid::Uuid::new_v4().to_string();

        settings = settings
            .replace("{{SNI}}", &new_sni)
            .replace("{{port}}", &new_port.to_string())
            .replace("{{uuid}}", &new_uuid)
            .replace("{{DOMAIN}}", domain)
            .replace("{{REALITY_PBK}}", pbk)
            .replace("{{REALITY_SID}}", sid);

        stream_settings = stream_settings
            .replace("{{SNI}}", &new_sni)
            .replace("{{port}}", &new_port.to_string())
            .replace("{{pool_sni}}", &new_sni) // Backward compat
            .replace("{{sni}}", &new_sni)      // Backward compat
            .replace("{{DOMAIN}}", domain)
            .replace("{{REALITY_PBK}}", pbk)
            .replace("{{REALITY_SID}}", sid);

        // Inject Reality keys for vless/naive
        if template.protocol == "vless" || template.protocol == "naive" {
            if let Some(pkey) = &node.reality_priv {
                stream_settings = stream_settings.replace("{{reality_private}}", pkey);
            }
        }

        // 5. Update DB
        sqlx::query(
            "UPDATE inbounds SET listen_port = ?, settings = ?, stream_settings = ?, last_rotated_at = datetime('now') WHERE id = ?"
        )
        .bind(new_port)
        .bind(settings)
        .bind(&stream_settings)
        .bind(inbound_id)
        .execute(&self.pool)
        .await?;

        info!("🔄 Rotated inbound '{}' (Node {}) to port {} and SNI {}", template.name, node.id, new_port, new_sni);
        Ok(())
    }

    pub async fn rotate_group_inbounds(&self, group_id: i64) -> Result<()> {
        let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(
            r#"
            SELECT i.* FROM inbounds i
            JOIN nodes n ON i.node_id = n.id
            JOIN node_group_members ngm ON n.id = ngm.node_id
            WHERE ngm.group_id = ? AND i.tag LIKE 'tpl_%'
            "#
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;

        for inbound in inbounds {
            if let Err(e) = self.rotate_inbound(inbound.id).await {
                warn!("Failed to rotate inbound {}: {}", inbound.id, e);
            }
        }
        
        Ok(())
    }
}
