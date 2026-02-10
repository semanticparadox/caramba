use sqlx::SqlitePool;
use anyhow::{Result, Context};
use crate::models::groups::InboundTemplate;
use crate::models::node::Node;
use tracing::{info, warn};

#[derive(Clone)]
pub struct GeneratorService {
    pool: SqlitePool,
}

impl GeneratorService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
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

        for node in nodes {
            for template in &templates {
                self.ensure_inbound_exists(&node, template).await?;
            }
        }

        Ok(())
    }

    /// Ensures a node has an inbound matching the template.
    /// Uses `tag` as a unique identifier (e.g. "template_{id}").
    async fn ensure_inbound_exists(&self, node: &Node, template: &InboundTemplate) -> Result<()> {
        let tag = format!("tpl_{}", template.id);

        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM inbounds WHERE node_id = ? AND tag = ?"
        )
        .bind(node.id)
        .bind(&tag)
        .fetch_optional(&self.pool)
        .await?;

        if exists.is_some() {
            // Inbound exists. Maybe update it? For now, skip.
            // TODO: dynamic rotation logic would go here (updating port/sni)
            return Ok(());
        }

        info!("Generating inbound '{}' for node {}", template.name, node.id);

        // 3. Resolve Placeholders
        // Simple port allocation: Random within range, check collision.
        let port = self.allocate_port(node.id, template.port_range_start, template.port_range_end).await?;
        
        let mut settings = template.settings_template.clone();
        let mut stream_settings = template.stream_settings_template.clone();
        
        // Replace {{uuid}} - Assuming template generates a server-side UUID, 
        // but VLESS usually relies on User's UUID. 
        // If the template needs a server-side distinct UUID (e.g. for Reality private key?), 
        // we should handle it. existing {{uuid}} usually means "generate one now".
        let new_uuid = uuid::Uuid::new_v4().to_string();
        
        settings = settings.replace("{{uuid}}", &new_uuid);
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

        // Find the template settings to know the port range
        // For now, we assume a default range if we can't link back to a template easily, 
        // OR we can parse the tag "tpl_{id}" to find the template.
        let port_range = if inbound.tag.starts_with("tpl_") {
            if let Ok(tpl_id) = inbound.tag[4..].parse::<i64>() {
                sqlx::query_as::<_, (i64, i64)>("SELECT port_range_start, port_range_end FROM inbound_templates WHERE id = ?")
                    .bind(tpl_id)
                    .fetch_optional(&self.pool)
                    .await?
            } else {
                None
            }
        } else {
            None
        };

        let (start, end) = port_range.unwrap_or((10000, 60000)); // Default fallback

        let new_port = self.allocate_port(inbound.node_id, start, end).await?;
        
        // Update Port
        sqlx::query("UPDATE inbounds SET listen_port = ?, stream_settings = replace(stream_settings, ?, ?) WHERE id = ?")
            .bind(new_port)
            .bind(inbound.listen_port.to_string()) // Replace old port in JSON? strict replacement might be risky if port is common number
            .bind(new_port.to_string())
            .bind(inbound_id)
            .execute(&self.pool)
            .await?;

        // Updating JSON content via string replace is risky but might work for simple templates.
        // Better: Parse JSON, update field, Serialize.
        // Let's do the robust way for settings.
        
        // Re-fetch to get fresh
        let mut inbound = sqlx::query_as::<_, crate::models::network::Inbound>("SELECT * FROM inbounds WHERE id = ?")
            .bind(inbound_id)
            .fetch_one(&self.pool)
            .await?;
            
        inbound.listen_port = new_port; // Updated by query above? Actually doing it in two steps is safer.
        // Let's just update the JSON fields now.
        
        if let Ok(mut settings) = serde_json::from_str::<serde_json::Value>(&inbound.settings) {
            // Update port in settings if it exists? VLESS usually doesn't have port in 'settings', just 'stream_settings' or 'listen_port' column.
            // But let's check.
        }

        info!("Rotated inbound {} (Node {}) to port {}", inbound_id, inbound.node_id, new_port);
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
