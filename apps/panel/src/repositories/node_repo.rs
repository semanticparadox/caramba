use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::node::{Node};
use crate::models::network::{Inbound};
use crate::models::groups::{NodeGroup, NodeGroupMember};

#[derive(Clone)]
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
            INSERT INTO nodes (name, ip, domain, country, city, flag, status, load_stats, check_stats_json, sort_order)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .fetch_one(&self.pool)
        .await?;
        
        Ok(id)
    }

    pub async fn update_node(&self, node: &Node) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE nodes 
            SET name=?, ip=?, domain=?, country=?, city=?, flag=?, status=?, load_stats=?, check_stats_json=?, sort_order=?
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
            INSERT INTO inbounds (node_id, tag, protocol, port, settings, stream_settings, enable, listen)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(node_id, tag) DO UPDATE SET
                protocol=excluded.protocol,
                port=excluded.port,
                settings=excluded.settings,
                stream_settings=excluded.stream_settings,
                enable=excluded.enable,
                listen=excluded.listen
            "#
        )
        .bind(inbound.node_id)
        .bind(&inbound.tag)
        .bind(&inbound.protocol)
        .bind(inbound.port)
        .bind(&inbound.settings)
        .bind(&inbound.stream_settings)
        .bind(inbound.enable)
        .bind(&inbound.listen)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    // ==================== GROUPS (NODES) ====================

    pub async fn get_all_groups(&self) -> Result<Vec<Group>> {
        sqlx::query_as::<_, Group>("SELECT * FROM groups ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch groups")
    }

    pub async fn get_group_nodes(&self, group_id: i64) -> Result<Vec<i64>> {
        sqlx::query_scalar("SELECT node_id FROM group_nodes WHERE group_id = ?")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group nodes")
    }
    
    /// Get distinct active nodes belonging to a set of group IDs
    pub async fn get_active_nodes_by_groups(&self, group_ids: &[i64]) -> Result<Vec<Node>> {
        if group_ids.is_empty() {
            return Ok(Vec::new());
        }
        
        let query = format!(
            "SELECT DISTINCT n.* FROM nodes n
             JOIN group_nodes gn ON gn.node_id = n.id
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
            WHERE (pi.plan_id = ? OR pn.plan_id = ?) AND i.enable = 1
            "#
        )
        .bind(plan_id)
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch inbounds for plan")
    }
}
