use sqlx::PgPool;
use anyhow::{Context, Result};
use crate::models::sni::{SniPoolItem, SniBlacklistItem};

#[derive(Debug, Clone)]
pub struct SniRepository {
    pool: PgPool,
}

impl SniRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_all_snis(&self) -> Result<Vec<SniPoolItem>> {
        sqlx::query_as::<_, SniPoolItem>("SELECT * FROM sni_pool WHERE discovered_by_node_id IS NULL OR is_premium = TRUE ORDER BY domain ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all global SNIs")
    }

    pub async fn get_active_snis(&self) -> Result<Vec<SniPoolItem>> {
        sqlx::query_as::<_, SniPoolItem>("SELECT * FROM sni_pool WHERE is_active = TRUE AND (discovered_by_node_id IS NULL OR is_premium = TRUE) ORDER BY domain ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch active global SNIs")
    }

    pub async fn get_snis_by_node(&self, node_id: i64) -> Result<Vec<SniPoolItem>> {
        sqlx::query_as::<_, SniPoolItem>("SELECT * FROM sni_pool WHERE discovered_by_node_id = $1 ORDER BY domain ASC")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch SNIs by node")
    }

    pub async fn add_sni(&self, domain: &str, tier: i32, notes: Option<&str>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO sni_pool (domain, tier, notes) VALUES ($1, $2, $3) RETURNING id"
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
        sqlx::query("DELETE FROM sni_pool WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete SNI from pool")?;
        
        Ok(())
    }

    pub async fn toggle_sni_active(&self, id: i64, active: bool) -> Result<()> {
        sqlx::query("UPDATE sni_pool SET is_active = $1 WHERE id = $2")
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
            LIMIT $1
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

    pub async fn seed_default_global_pool_if_empty(&self) -> Result<()> {
        let global_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sni_pool WHERE discovered_by_node_id IS NULL OR is_premium = TRUE"
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to count global SNI pool entries")?;

        if global_count > 0 {
            return Ok(());
        }

        let defaults = [
            ("gosuslugi.ru", 0, "Public Services"),
            ("www.cloudflare.com", 1, "CDN baseline"),
            ("www.microsoft.com", 1, "Enterprise baseline"),
            ("www.apple.com", 1, "Consumer baseline"),
            ("www.amazon.com", 1, "Global baseline"),
        ];

        for (domain, tier, notes) in defaults {
            sqlx::query(
                "INSERT INTO sni_pool (domain, tier, notes, is_active) VALUES ($1, $2, $3, TRUE) ON CONFLICT (domain) DO NOTHING"
            )
            .bind(domain)
            .bind(tier)
            .bind(notes)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to seed default SNI domain '{}'", domain))?;
        }

        Ok(())
    }
}
