use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::SniRotationLog;

#[derive(Debug, Clone)]
pub struct SecurityService {
    pool: SqlitePool,
}

impl SecurityService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_next_sni(&self, current_sni: &str, tier: i32) -> Result<String> {
        let sni: Option<String> = sqlx::query_scalar(
            "SELECT domain FROM sni_pool 
             WHERE domain != ? AND tier <= ? AND is_active = 1
             ORDER BY health_score DESC, RANDOM()
             LIMIT 1"
        )
        .bind(current_sni)
        .bind(tier)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get next SNI")?;
        
        Ok(sni.unwrap_or_else(|| "www.google.com".to_string()))
    }

    /// Prefer SNIs discovered by THIS node, fallback to global popular ones
    pub async fn get_best_sni_for_node(&self, node_id: i64) -> Result<String> {
        // 1. Try node-specific SNI
        let node_sni: Option<String> = sqlx::query_scalar(
            "SELECT domain FROM sni_pool WHERE discovered_by_node_id = ? AND is_active = 1 ORDER BY health_score DESC LIMIT 1"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(sni) = node_sni {
            return Ok(sni);
        }

        // 2. Fallback to global best
        self.get_next_sni("", 1).await
    }

    pub async fn log_sni_rotation(
        &self, 
        node_id: i64, 
        old_sni: &str, 
        new_sni: &str, 
        reason: &str
    ) -> Result<SniRotationLog> {
        let log = sqlx::query_as::<_, SniRotationLog>(
            "INSERT INTO sni_rotation_log (node_id, old_sni, new_sni, reason)
             VALUES (?, ?, ?, ?)
             RETURNING id, node_id, old_sni, new_sni, reason, rotated_at"
        )
        .bind(node_id)
        .bind(old_sni)
        .bind(new_sni)
        .bind(reason)
        .fetch_one(&self.pool)
        .await
        .context("Failed to log SNI rotation")?;

        Ok(log)
    }

    pub async fn rotate_node_sni(&self, node_id: i64, reason: &str) -> Result<(String, String, i64)> {
        // 1. Get current SNI
        let current_sni: Option<String> = sqlx::query_scalar("SELECT reality_sni FROM nodes WHERE id = ?")
            .bind(node_id)
            .fetch_optional(&self.pool)
            .await?;
            
        let current_sni = current_sni.unwrap_or_else(|| "www.google.com".to_string());

        // 2. Get Next SNI
        let next_sni = self.get_next_sni(&current_sni, 1).await?;
        
        if next_sni == current_sni {
            return Err(anyhow::anyhow!("No other SNI available"));
        }

        // 3. Update Node
        sqlx::query("UPDATE nodes SET reality_sni = ? WHERE id = ?")
            .bind(&next_sni)
            .bind(node_id)
            .execute(&self.pool)
            .await?;

        // 4. Log
        let log = self.log_sni_rotation(node_id, &current_sni, &next_sni, reason).await?;
        
        Ok((current_sni, next_sni, log.id))
    }
}
