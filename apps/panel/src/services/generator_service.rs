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
                "UPDATE inbounds SET protocol = ?, settings = ?, stream_settings = ?, remark = ? WHERE id = ?"
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

        // 1. Find Template
        if !inbound.tag.starts_with("tpl_") {
            return Err(anyhow::anyhow!("Inbound is not tied to a template"));
        }
        let tpl_id = inbound.tag[4..].parse::<i64>().context("Invalid template ID in tag")?;
        let template = sqlx::query_as::<_, InboundTemplate>("SELECT * FROM inbound_templates WHERE id = ?")
            .bind(tpl_id)
            .fetch_one(&self.pool)
            .await
            .context("Template not found for rotation")?;

        // 2. Resolve New Values
        let new_port = self.allocate_port(inbound.node_id, template.port_range_start, template.port_range_end).await?;
        
        let mut settings = template.settings_template.clone();
        let mut stream_settings = template.stream_settings_template.clone();
        
        // We probably want to keep the same UUID if it's already in the inbound settings 
        // to avoid breaking active connections immediately, 
        // OR we just use the same logic as ensure_inbound_exists.
        // Actually, for rotation, we usually ONLY want to change Port and SNI (the "outer" layers).
        
        // Extract old SNI for logging
        let old_sni = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&inbound.stream_settings) {
            json["reality_settings"]["server_names"][0]
                .as_str()
                .or_else(|| json["tls_settings"]["server_name"].as_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            "unknown".to_string()
        };

        // Re-interpolate
        // Re-use ensure_inbound_exists logic for substitution (simplified here or refactored into helper)
        if stream_settings.contains("{{pool_sni}}") {
            let pool_sni: Option<String> = sqlx::query_scalar(
                "SELECT domain FROM sni_pool WHERE is_active = 1 ORDER BY RANDOM() LIMIT 1"
            )
            .fetch_optional(&self.pool)
            .await?;
            if let Some(sni) = pool_sni {
                stream_settings = stream_settings.replace("{{pool_sni}}", &sni);
            }
        }
        
        if let Some(sni) = &node.reality_sni {
            stream_settings = stream_settings.replace("{{sni}}", sni);
        } else {
            stream_settings = stream_settings.replace("{{sni}}", "www.google.com");
        }

        if let Some(priv_key) = &node.reality_priv {
            stream_settings = stream_settings.replace("{{reality_private}}", priv_key);
        }

        // We use a fixed string replacement for UUID for now if we don't have the original one easily.
        // Better: maybe store the "resolved_uuid" somewhere or just generate a new one.
        // For now, let's just generate a new one like in ensure_inbound_exists 
        // UNLESS we want to stick to the old one. 
        // Let's stick to a NEW one for MAXIMUM rotation/obfuscation (security policy).
        let new_uuid = uuid::Uuid::new_v4().to_string();
        settings = settings.replace("{{uuid}}", &new_uuid);
        settings = settings.replace("{{port}}", &new_port.to_string());

        // 3. Update DB
        sqlx::query(
            "UPDATE inbounds SET listen_port = ?, settings = ?, stream_settings = ?, last_rotated_at = datetime('now') WHERE id = ?"
        )
        .bind(new_port)
        .bind(settings)
        .bind(&stream_settings)
        .bind(inbound_id)
        .execute(&self.pool)
        .await?;

        // 4. Log Rotation
        let new_sni = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stream_settings) {
             json["reality_settings"]["server_names"][0]
                .as_str()
                .or_else(|| json["tls_settings"]["server_name"].as_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            "unknown".to_string()
        };

        if old_sni != new_sni {
            let _ = sqlx::query(
                "INSERT INTO sni_rotation_log (node_id, old_sni, new_sni, reason) VALUES (?, ?, ?, ?)"
            )
            .bind(node.id)
            .bind(old_sni)
            .bind(new_sni)
            .bind("Periodic rotation")
            .execute(&self.pool)
            .await;
        }

        info!("Rotated inbound {} (Node {}) to port {} and updated SNI/UUID", inbound_id, node.id, new_port);
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
