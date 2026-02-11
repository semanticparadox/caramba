use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::node::{Node};
use crate::models::network::{Inbound};
use crate::models::groups::{NodeGroup, NodeGroupMember, PlanGroup};

#[derive(Debug, Clone)]
pub struct NodeRepository {
    pool: SqlitePool,
}

impl NodeRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ==================== NODES ====================

    pub async fn get_all_nodes(&self) -> Result<Vec<Node>> {
        sqlx::query_as::<_, Node>("SELECT * FROM nodes ORDER BY sort_order ASC, name ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all nodes")
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<Node>> {
        sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE status = 'active' ORDER BY sort_order ASC, name ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch active nodes")
    }

    pub async fn get_node_by_id(&self, id: i64) -> Result<Option<Node>> {
        sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch node by ID")
    }
    
    pub async fn get_active_node_ids(&self) -> Result<Vec<i64>> {
         sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active'")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch active node IDs")
    }

    pub async fn create_node(&self, node: &Node) -> Result<i64> {
        let id = sqlx::query_scalar(
            r#"
            INSERT INTO nodes (
                name, ip, domain, country, city, flag, 
                status, load_stats, check_stats_json, sort_order,
                join_token, vpn_port, auto_configure, is_enabled,
                reality_pub, reality_priv, short_id, reality_sni,
                relay_id
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#
        )
        .bind(&node.name)
        .bind(&node.ip)
        .bind(&node.domain)
        .bind(&node.country)
        .bind(&node.city)
        .bind(&node.flag)
        .bind(&node.status)
        .bind(&node.load_stats)
        .bind(&node.check_stats_json)
        .bind(node.sort_order)
        .bind(&node.join_token)
        .bind(node.vpn_port)
        .bind(node.auto_configure)
        .bind(node.is_enabled)
        .bind(&node.reality_pub)
        .bind(&node.reality_priv)
        .bind(&node.short_id)
        .bind(&node.reality_sni)
        .bind(node.relay_id)
        .fetch_one(&self.pool)
        .await?;
        
        Ok(id)
    }

    pub async fn update_node(&self, node: &Node) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE nodes 
            SET name=?, ip=?, domain=?, country=?, city=?, flag=?, status=?, load_stats=?, check_stats_json=?, sort_order=?,
                join_token=?, vpn_port=?, auto_configure=?, is_enabled=?, reality_sni=?, relay_id=?
            WHERE id=?
            "#
        )
        .bind(&node.name)
        .bind(&node.ip)
        .bind(&node.domain)
        .bind(&node.country)
        .bind(&node.city)
        .bind(&node.flag)
        .bind(&node.status)
        .bind(&node.load_stats)
        .bind(&node.check_stats_json)
        .bind(node.sort_order)
        .bind(&node.join_token)
        .bind(node.vpn_port)
        .bind(node.auto_configure)
        .bind(node.is_enabled)
        .bind(&node.reality_sni)
        .bind(node.relay_id)
        .bind(node.id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    // ==================== INBOUNDS ====================

    pub async fn get_inbounds_by_node(&self, node_id: i64) -> Result<Vec<Inbound>> {
        sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds WHERE node_id = ?")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch inbounds for node")
    }
    
    /// Get all distinct inbounds (legacy/generic usage)
    pub async fn get_all_inbounds(&self) -> Result<Vec<Inbound>> {
         sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all inbounds")
    }

    pub async fn upsert_inbound(&self, inbound: &Inbound) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, enable, listen_ip)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(node_id, tag) DO UPDATE SET
                protocol=excluded.protocol,
                listen_port=excluded.listen_port,
                settings=excluded.settings,
                stream_settings=excluded.stream_settings,
                enable=excluded.enable,
                listen_ip=excluded.listen_ip
            "#
        )
        .bind(inbound.node_id)
        .bind(&inbound.tag)
        .bind(&inbound.protocol)
        .bind(inbound.listen_port)
        .bind(&inbound.settings)
        .bind(&inbound.stream_settings)
        .bind(inbound.enable)
        .bind(&inbound.listen_ip)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    // ==================== GROUPS (NODES) ====================

    pub async fn get_all_groups(&self) -> Result<Vec<NodeGroup>> {
        sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch node groups")
    }

    pub async fn get_group_nodes(&self, group_id: i64) -> Result<Vec<i64>> {
        sqlx::query_scalar("SELECT node_id FROM node_group_members WHERE group_id = ?")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group nodes")
    }

    /// Get full membership records for a group
    pub async fn get_group_members(&self, group_id: i64) -> Result<Vec<NodeGroupMember>> {
        sqlx::query_as::<_, NodeGroupMember>("SELECT * FROM node_group_members WHERE group_id = ?")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group members")
    }

    /// Get plan-group associations for a plan
    pub async fn get_plan_groups(&self, plan_id: i64) -> Result<Vec<PlanGroup>> {
        sqlx::query_as::<_, PlanGroup>("SELECT * FROM plan_groups WHERE plan_id = ?")
            .bind(plan_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch plan groups")
    }
    
    /// Get distinct active nodes belonging to a set of group IDs
    pub async fn get_active_nodes_by_groups(&self, group_ids: &[i64]) -> Result<Vec<Node>> {
        if group_ids.is_empty() {
            return Ok(Vec::new());
        }
        
        let query = format!(
            "SELECT DISTINCT n.* FROM nodes n
             JOIN node_group_members gn ON gn.node_id = n.id
             WHERE n.status = 'active' AND gn.group_id IN ({})
             ORDER BY n.sort_order ASC",
            group_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",")
        );
        
        let mut q = sqlx::query_as::<_, Node>(&query);
        for id in group_ids {
            q = q.bind(id);
        }
        
        q.fetch_all(&self.pool).await.context("Failed to fetch nodes by groups")
    }
    
    // ==================== BUSINESS LOGIC queries ====================

    pub async fn get_nodes_for_plan(&self, plan_id: i64) -> Result<Vec<Node>> {
        // Try getting nodes via groups
        let nodes = sqlx::query_as::<_, Node>(
            r#"
            SELECT DISTINCT n.* 
            FROM nodes n
            JOIN node_group_members ngm ON n.id = ngm.node_id
            JOIN plan_groups pg ON ngm.group_id = pg.group_id
            WHERE pg.plan_id = ? AND n.status = 'active'
            ORDER BY n.sort_order ASC
            "#
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await?;
        
        if !nodes.is_empty() {
             return Ok(nodes);
        }
        
        // Fallback: Check if plan has ANY groups
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plan_groups WHERE plan_id = ?")
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;
            
        if count == 0 {
            // Legacy: Return all active nodes
            return self.get_active_nodes().await;
        }
        
        // Plan has groups but no nodes found (e.g. empty groups), return empty.
        Ok(Vec::new())
    }

    pub async fn get_inbounds_for_plan(&self, plan_id: i64) -> Result<Vec<Inbound>> {
        // Fetch inbounds linked via plan_inbounds OR via plan_nodes (indirectly via node)
        // This unifies the logic found in generate_subscription_links
        sqlx::query_as::<_, Inbound>(
            r#"
            SELECT DISTINCT i.* FROM inbounds i
            LEFT JOIN plan_inbounds pi ON pi.inbound_id = i.id
            LEFT JOIN plan_nodes pn ON pn.node_id = i.node_id
            LEFT JOIN node_group_members ngm ON ngm.node_id = i.node_id
            LEFT JOIN plan_groups pg ON pg.group_id = ngm.group_id
            WHERE (pi.plan_id = ? OR pn.plan_id = ? OR pg.plan_id = ?) AND i.enable = 1
            "#
        )
        .bind(plan_id)
        .bind(plan_id)
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch inbounds for plan")
    }

    pub async fn delete_node(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM nodes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE nodes SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_enabled(&self, id: i64) -> Result<bool> {
        let current: bool = sqlx::query_scalar("SELECT is_enabled FROM nodes WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        
        let new_val = !current;
        sqlx::query("UPDATE nodes SET is_enabled = ? WHERE id = ?")
            .bind(new_val)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(new_val)
    }

    pub async fn get_linked_plans(&self, node_id: i64, inbound_id: i64) -> Result<Vec<i64>> {
        let plans: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT plan_id FROM plan_inbounds WHERE inbound_id = ?
            UNION
            SELECT plan_id FROM plan_nodes WHERE node_id = ?
            UNION
            SELECT pg.plan_id 
            FROM plan_groups pg
            JOIN node_group_members ngm ON pg.group_id = ngm.group_id
            WHERE ngm.node_id = ?
            "#
        )
        .bind(inbound_id)
        .bind(node_id)
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(plans)
    }

    pub async fn link_inbound_to_plan(&self, plan_id: i64, inbound_id: i64) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO plan_inbounds (plan_id, inbound_id) VALUES (?, ?)")
            .bind(plan_id)
            .bind(inbound_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn link_node_inbounds_to_plan(&self, plan_id: i64, node_id: i64) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO plan_inbounds (plan_id, inbound_id) SELECT ?, id FROM inbounds WHERE node_id = ?")
            .bind(plan_id)
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all inbound templates for a specific group
    pub async fn get_templates_for_group(&self, group_id: i64) -> Result<Vec<crate::models::groups::InboundTemplate>> {
        sqlx::query_as::<_, crate::models::groups::InboundTemplate>("SELECT * FROM inbound_templates WHERE target_group_id = ? AND is_active = 1")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch templates for group")
    }
}
