use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::sni::{SniPoolItem, SniBlacklistItem};

#[derive(Debug, Clone)]
pub struct SniRepository {
    pool: SqlitePool,
}

impl SniRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_all_snis(&self) -> Result<Vec<SniPoolItem>> {
        sqlx::query_as::<_, SniPoolItem>("SELECT * FROM sni_pool WHERE discovered_by_node_id IS NULL OR is_premium = 1 ORDER BY domain ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all global SNIs")
    }

    pub async fn get_active_snis(&self) -> Result<Vec<SniPoolItem>> {
        sqlx::query_as::<_, SniPoolItem>("SELECT * FROM sni_pool WHERE is_active = 1 AND (discovered_by_node_id IS NULL OR is_premium = 1) ORDER BY domain ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch active global SNIs")
    }

    pub async fn get_snis_by_node(&self, node_id: i64) -> Result<Vec<SniPoolItem>> {
        sqlx::query_as::<_, SniPoolItem>("SELECT * FROM sni_pool WHERE discovered_by_node_id = ? ORDER BY domain ASC")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch SNIs by node")
    }

    pub async fn add_sni(&self, domain: &str, tier: i32, notes: Option<&str>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO sni_pool (domain, tier, notes) VALUES (?, ?, ?) RETURNING id"
        )
        .bind(domain)
        .bind(tier)
        .bind(notes)
        .fetch_one(&self.pool)
        .await
        .context("Failed to add SNI to pool")?;
        
        Ok(id)
    }

    pub async fn delete_sni(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM sni_pool WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete SNI from pool")?;
        
        Ok(())
    }

    pub async fn toggle_sni_active(&self, id: i64, active: bool) -> Result<()> {
        sqlx::query("UPDATE sni_pool SET is_active = ? WHERE id = ?")
            .bind(active)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to toggle SNI activity")?;
        
        Ok(())
    }

    pub async fn get_recent_logs(&self, limit: i64) -> Result<Vec<crate::models::sni_log::SniRotationLog>> {
        sqlx::query_as::<_, crate::models::sni_log::SniRotationLog>(
            r#"
            SELECT l.*, n.name as node_name
            FROM sni_rotation_log l
            LEFT JOIN nodes n ON l.node_id = n.id
            ORDER BY l.rotated_at DESC
            LIMIT ?
            "#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch rotation logs")
    }
    pub async fn get_blacklisted_snis(&self) -> Result<Vec<SniBlacklistItem>> {
        sqlx::query_as::<_, SniBlacklistItem>("SELECT * FROM sni_blacklist ORDER BY blocked_at DESC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch blacklisted SNIs")
    }
}
