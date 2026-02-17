use sqlx::PgPool;
use anyhow::{Context, Result};
use crate::models::node::{Node};
use crate::models::network::{Inbound};
use crate::models::groups::{NodeGroup, NodeGroupMember, PlanGroup};

#[derive(Debug, Clone)]
pub struct NodeRepository {
    pool: PgPool,
}

impl NodeRepository {
    pub fn new(pool: PgPool) -> Self {
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
        sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = $1")
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

    pub async fn get_relay_clients(&self, node_id: i64) -> Result<Vec<Node>> {
        sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE relay_id = $1 AND status = 'active'")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch relay clients")
    }

    pub async fn create_node(&self, node: &Node) -> Result<i64> {
        let id = sqlx::query_scalar(
            r#"
            INSERT INTO nodes (
                name, ip, domain, country, city, flag, 
                status, load_stats, check_stats_json, sort_order,
                join_token, vpn_port, auto_configure, is_enabled,
                reality_pub, reality_priv, short_id, reality_sni,
                relay_id, doomsday_password
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
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
        .bind(&node.doomsday_password)
        .fetch_one(&self.pool)
        .await?;
        
        Ok(id)
    }

    pub async fn update_node(&self, node: &Node) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE nodes 
            SET name=$1, ip=$2, domain=$3, country=$4, city=$5, flag=$6, status=$7, load_stats=$8, check_stats_json=$9, sort_order=$10,
                join_token=$11, vpn_port=$12, auto_configure=$13, is_enabled=$14, 
                reality_pub=$15, reality_priv=$16, short_id=$17, reality_sni=$18, 
                relay_id=$19, doomsday_password=$20
            WHERE id=$21
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
        .bind(&node.doomsday_password)
        .bind(node.id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    // ==================== INBOUNDS ====================

    pub async fn get_inbounds_by_node(&self, node_id: i64) -> Result<Vec<Inbound>> {
        sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds WHERE node_id = $1")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch inbounds for node")
    }
    
    pub async fn get_all_inbounds(&self) -> Result<Vec<Inbound>> {
         sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all inbounds")
    }

    pub async fn upsert_inbound(&self, inbound: &Inbound) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, enable, listen_ip, remark)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT(node_id, listen_port) DO UPDATE SET
                tag=excluded.tag,
                protocol=excluded.protocol,
                settings=excluded.settings,
                stream_settings=excluded.stream_settings,
                enable=excluded.enable,
                listen_ip=excluded.listen_ip,
                remark=excluded.remark
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
        .bind(&inbound.remark)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    pub async fn get_inbound_by_id(&self, id: i64) -> Result<Option<Inbound>> {
        sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch inbound by ID")
    }

    pub async fn delete_inbound_by_id(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM inbounds WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_inbound_templates(&self) -> Result<Vec<crate::models::groups::InboundTemplate>> {
        sqlx::query_as::<_, crate::models::groups::InboundTemplate>("SELECT * FROM inbound_templates WHERE is_active = TRUE")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all inbound templates")
    }

    // ==================== GROUPS (NODES) ====================

    pub async fn get_all_groups(&self) -> Result<Vec<NodeGroup>> {
        sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch node groups")
    }

    pub async fn get_group_nodes(&self, group_id: i64) -> Result<Vec<i64>> {
        sqlx::query_scalar("SELECT node_id FROM node_group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group nodes")
    }

    pub async fn get_group_members(&self, group_id: i64) -> Result<Vec<NodeGroupMember>> {
        sqlx::query_as::<_, NodeGroupMember>("SELECT * FROM node_group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group members")
    }

    pub async fn get_plan_groups(&self, plan_id: i64) -> Result<Vec<PlanGroup>> {
        sqlx::query_as::<_, PlanGroup>("SELECT * FROM plan_groups WHERE plan_id = $1")
            .bind(plan_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch plan groups")
    }
    
    pub async fn get_active_nodes_by_groups(&self, group_ids: &[i64]) -> Result<Vec<Node>> {
        if group_ids.is_empty() {
            return Ok(Vec::new());
        }
        
        sqlx::query_as::<_, Node>(
            r#"
            SELECT DISTINCT n.* FROM nodes n
            JOIN node_group_members gn ON gn.node_id = n.id
            WHERE n.status = 'active' AND gn.group_id = ANY($1)
            ORDER BY n.sort_order ASC
            "#
        )
        .bind(group_ids)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch nodes by groups")
    }

    pub async fn create_group(&self, name: &str, description: Option<&str>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO node_groups (name, description, created_at) VALUES ($1, $2, CURRENT_TIMESTAMP) RETURNING id"
        )
        .bind(name)
        .bind(description)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create node group")?;
        Ok(id)
    }

    pub async fn get_group_by_name(&self, name: &str) -> Result<Option<crate::models::groups::NodeGroup>> {
        sqlx::query_as::<_, crate::models::groups::NodeGroup>("SELECT * FROM node_groups WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch group by name")
    }

    pub async fn add_node_to_group(&self, node_id: i64, group_id: i64) -> Result<()> {
        sqlx::query("INSERT INTO node_group_members (node_id, group_id, created_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT DO NOTHING")
            .bind(node_id)
            .bind(group_id)
            .execute(&self.pool)
            .await
            .context("Failed to add node to group")?;
        Ok(())
    }
    
    pub async fn get_groups_by_node(&self, node_id: i64) -> Result<Vec<crate::models::groups::NodeGroup>> {
        sqlx::query_as::<_, crate::models::groups::NodeGroup>(
            "SELECT g.* FROM node_groups g JOIN node_group_members m ON m.group_id = g.id WHERE m.node_id = $1"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch node groups")
    }

    // ==================== BUSINESS LOGIC queries ====================

    pub async fn get_nodes_for_plan(&self, plan_id: i64) -> Result<Vec<Node>> {
        let nodes = sqlx::query_as::<_, Node>(
            r#"
            SELECT DISTINCT n.* 
            FROM nodes n
            JOIN node_group_members ngm ON n.id = ngm.node_id
            JOIN plan_groups pg ON ngm.group_id = pg.group_id
            WHERE pg.plan_id = $1 AND n.status = 'active'
            ORDER BY n.sort_order ASC
            "#
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await?;
        
        if !nodes.is_empty() {
             return Ok(nodes);
        }
        
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plan_groups WHERE plan_id = $1")
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;
            
        if count == 0 {
            return self.get_active_nodes().await;
        }
        
        Ok(Vec::new())
    }

    pub async fn get_inbounds_for_plan(&self, plan_id: i64) -> Result<Vec<Inbound>> {
        sqlx::query_as::<_, Inbound>(
            r#"
            SELECT DISTINCT i.* FROM inbounds i
            LEFT JOIN plan_inbounds pi ON pi.inbound_id = i.id
            LEFT JOIN plan_nodes pn ON pn.node_id = i.node_id
            LEFT JOIN node_group_members ngm ON ngm.node_id = i.node_id
            LEFT JOIN plan_groups pg ON pg.group_id = ngm.group_id
            WHERE (pi.plan_id = $1 OR pn.plan_id = $2 OR pg.plan_id = $3) AND i.enable = TRUE
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
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE nodes SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_enabled(&self, id: i64) -> Result<bool> {
        let current: bool = sqlx::query_scalar("SELECT is_enabled FROM nodes WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        
        let new_val = !current;
        sqlx::query("UPDATE nodes SET is_enabled = $1 WHERE id = $2")
            .bind(new_val)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(new_val)
    }

    pub async fn get_linked_plans(&self, node_id: i64, inbound_id: i64) -> Result<Vec<i64>> {
        let plans: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT plan_id FROM plan_inbounds WHERE inbound_id = $1
            UNION
            SELECT plan_id FROM plan_nodes WHERE node_id = $2
            UNION
            SELECT pg.plan_id 
            FROM plan_groups pg
            JOIN node_group_members ngm ON pg.group_id = ngm.group_id
            WHERE ngm.node_id = $3
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
        sqlx::query("INSERT INTO plan_inbounds (plan_id, inbound_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(plan_id)
            .bind(inbound_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn link_node_inbounds_to_plan(&self, plan_id: i64, node_id: i64) -> Result<()> {
        sqlx::query("INSERT INTO plan_inbounds (plan_id, inbound_id) SELECT $1, id FROM inbounds WHERE node_id = $2 ON CONFLICT DO NOTHING")
            .bind(plan_id)
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_templates_for_group(&self, group_id: i64) -> Result<Vec<crate::models::groups::InboundTemplate>> {
        sqlx::query_as::<_, crate::models::groups::InboundTemplate>("SELECT * FROM inbound_templates WHERE target_group_id = $1 AND is_active = TRUE")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch templates for group")
    }
}
