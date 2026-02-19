use crate::services::orchestration_service::OrchestrationService;
use crate::services::security_service::SecurityService;
use anyhow::{Context, Result};
use caramba_db::models::groups::InboundTemplate;
use caramba_db::models::node::Node;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct GeneratorService {
    pool: PgPool,
    security_service: Arc<SecurityService>,
    orchestration_service: Arc<OrchestrationService>,
    pubsub: Arc<crate::services::pubsub_service::PubSubService>,
}

impl GeneratorService {
    pub fn new(
        pool: PgPool,
        security_service: Arc<SecurityService>,
        orchestration_service: Arc<OrchestrationService>,
        pubsub: Arc<crate::services::pubsub_service::PubSubService>,
    ) -> Self {
        Self {
            pool,
            security_service,
            orchestration_service,
            pubsub,
        }
    }

    /// Syncs inbounds for all nodes in a specific group based on active templates.
    pub async fn sync_group_inbounds(&self, group_id: i64) -> Result<()> {
        // 1. Get Templates for this group
        let templates = sqlx::query_as::<_, InboundTemplate>(
            "SELECT * FROM inbound_templates WHERE target_group_id = $1 AND is_active = TRUE",
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
            WHERE ngm.group_id = $1
            "#,
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;

        info!(
            "🔄 Syncing {} templates to {} nodes in group {}",
            templates.len(),
            nodes.len(),
            group_id
        );

        for node in &nodes {
            for template in &templates {
                match self.ensure_inbound_exists(node, template).await {
                    Ok(_) => info!(
                        "✅ Inbound for template {} synced to node {} ({})",
                        template.id, node.id, node.name
                    ),
                    Err(e) => error!(
                        "❌ Failed to sync template {} to node {}: {}",
                        template.id, node.id, e
                    ),
                }
            }
            // Notify node to update config
            let _ = self
                .pubsub
                .publish(&format!("node_events:{}", node.id), "update")
                .await;
        }

        Ok(())
    }

    /// Ensures a node has an inbound matching the template.
    async fn ensure_inbound_exists(&self, node: &Node, template: &InboundTemplate) -> Result<()> {
        let tag = format!("tpl_{}", template.id);

        let existing_inbound: Option<(i64, i64)> =
            sqlx::query_as("SELECT id, listen_port FROM inbounds WHERE node_id = $1 AND tag = $2")
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
            // Check compliance with current template range
            let port_in_range = existing_port >= template.port_range_start
                && existing_port <= template.port_range_end;

            let port = if port_in_range {
                existing_port
            } else {
                info!(
                    "Inbound {} port {} is outside template range {}-{}, updating...",
                    tag, existing_port, template.port_range_start, template.port_range_end
                );
                let new_port = if template.port_range_end > template.port_range_start {
                    self.allocate_port(node.id, template.port_range_start, template.port_range_end)
                        .await
                        .unwrap_or(template.port_range_start)
                } else {
                    template.port_range_start
                };
                new_port
            };

            settings = settings.replace("{{port}}", &port.to_string());

            if stream_settings.contains("{{reality_private}}") {
                if let Some(priv_key) = &node.reality_priv {
                    stream_settings = stream_settings.replace("{{reality_private}}", priv_key);
                } else {
                    warn!("Node {} missing Reality keys for template", node.id);
                }
            }

            if stream_settings.contains("{{pool_sni}}") || stream_settings.contains("{{sni}}") {
                let best_sni = self
                    .security_service
                    .get_best_sni_for_node(node.id)
                    .await
                    .unwrap_or_else(|_| "www.google.com".to_string());
                stream_settings = stream_settings.replace("{{pool_sni}}", &best_sni);
                stream_settings = stream_settings.replace("{{sni}}", &best_sni);
            }

            sqlx::query(
                "UPDATE inbounds SET protocol = $1, settings = $2, stream_settings = $3, remark = $4, listen_port = $5, enable = TRUE WHERE id = $6"
            )
            .bind(&template.protocol)
            .bind(&settings)
            .bind(&stream_settings)
            .bind(&template.name)
            .bind(port)
            .bind(id)
            .execute(&self.pool)
            .await?;

            return Ok(());
        }

        let port = self
            .allocate_port(node.id, template.port_range_start, template.port_range_end)
            .await?;
        settings = settings.replace("{{port}}", &port.to_string());

        if stream_settings.contains("{{reality_private}}") {
            if let Some(priv_key) = &node.reality_priv {
                stream_settings = stream_settings.replace("{{reality_private}}", priv_key);
            } else {
                warn!("Node {} missing Reality keys for template", node.id);
            }
        }

        if stream_settings.contains("{{pool_sni}}") || stream_settings.contains("{{sni}}") {
            let best_sni = self
                .security_service
                .get_best_sni_for_node(node.id)
                .await
                .unwrap_or_else(|_| "www.google.com".to_string());
            stream_settings = stream_settings.replace("{{pool_sni}}", &best_sni);
            stream_settings = stream_settings.replace("{{sni}}", &best_sni);
        }

        sqlx::query(
            r#"
            INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, remark, enable)
            VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)
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
        let (mut start, mut end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        start = start.clamp(1, 65535);
        end = end.clamp(1, 65535);
        if start > end {
            return Err(anyhow::anyhow!(
                "Invalid port range for node {}: {}-{}",
                node_id,
                start,
                end
            ));
        }

        let used_ports: Vec<i64> =
            sqlx::query_scalar("SELECT listen_port FROM inbounds WHERE node_id = $1")
                .bind(node_id)
                .fetch_all(&self.pool)
                .await?;

        use rand::Rng;
        let mut rng = rand::rng();

        for _ in 0..100 {
            let p = rng.random_range(start..=end);
            if !used_ports.contains(&p) {
                return Ok(p);
            }
        }

        Err(anyhow::anyhow!(
            "Failed to allocate port for node {}",
            node_id
        ))
    }

    pub async fn rotate_inbound(&self, inbound_id: i64) -> Result<()> {
        let inbound = sqlx::query_as::<_, caramba_db::models::network::Inbound>(
            "SELECT * FROM inbounds WHERE id = $1",
        )
        .bind(inbound_id)
        .fetch_one(&self.pool)
        .await
        .context("Inbound not found")?;

        let node = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = $1")
            .bind(inbound.node_id)
            .fetch_one(&self.pool)
            .await
            .context("Node not found")?;

        if !inbound.tag.starts_with("tpl_") {
            return Err(anyhow::anyhow!("Inbound is not tied to a template"));
        }

        let template = if let Some(remark) = &inbound.remark {
            sqlx::query_as::<_, InboundTemplate>("SELECT * FROM inbound_templates WHERE name = $1")
                .bind(remark)
                .fetch_optional(&self.pool)
                .await?
        } else {
            None
        };

        let template =
            template.ok_or_else(|| anyhow::anyhow!("Template not found for rotation"))?;

        let new_port = self
            .orchestration_service
            .allocate_port(
                inbound.node_id,
                template.port_range_start,
                template.port_range_end,
            )
            .await?;

        let new_sni = self
            .security_service
            .get_best_sni_for_node(node.id)
            .await
            .unwrap_or_else(|_| "www.google.com".to_string());

        let mut settings = template.settings_template.clone();
        let mut stream_settings = template.stream_settings_template.clone();

        let domain = node.domain.as_deref().unwrap_or("");
        let pbk = node.reality_pub.as_deref().unwrap_or("");
        let sid = node.short_id.as_deref().unwrap_or("");

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
            .replace("{{pool_sni}}", &new_sni)
            .replace("{{sni}}", &new_sni)
            .replace("{{DOMAIN}}", domain)
            .replace("{{REALITY_PBK}}", pbk)
            .replace("{{REALITY_SID}}", sid);

        if template.protocol == "vless" || template.protocol == "naive" {
            if let Some(pkey) = &node.reality_priv {
                stream_settings = stream_settings.replace("{{reality_private}}", pkey);
            }
        }

        sqlx::query(
            "UPDATE inbounds SET listen_port = $1, settings = $2, stream_settings = $3, last_rotated_at = CURRENT_TIMESTAMP WHERE id = $4"
        )
        .bind(new_port)
        .bind(settings)
        .bind(&stream_settings)
        .bind(inbound_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn rotate_group_inbounds(&self, group_id: i64) -> Result<()> {
        let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(
            r#"
            SELECT i.* FROM inbounds i
            JOIN nodes n ON i.node_id = n.id
            JOIN node_group_members ngm ON n.id = ngm.node_id
            WHERE ngm.group_id = $1 AND i.tag LIKE 'tpl_%'
            "#,
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
