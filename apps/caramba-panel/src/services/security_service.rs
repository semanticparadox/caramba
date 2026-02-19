use anyhow::{Context, Result};
use caramba_db::models::sni_log::SniRotationLog;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct SecurityService {
    pool: PgPool,
}

impl SecurityService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_next_sni(
        &self,
        current_sni: &str,
        tier: i32,
        premium_only: bool,
    ) -> Result<String> {
        let query = if premium_only {
            "SELECT domain FROM sni_pool 
             WHERE domain != $1 AND tier <= $2 AND is_active = TRUE AND is_premium = TRUE
             ORDER BY health_score DESC, RANDOM()
             LIMIT 1"
        } else {
            "SELECT domain FROM sni_pool 
             WHERE domain != $1 AND tier <= $2 AND is_active = TRUE
             ORDER BY health_score DESC, RANDOM()
             LIMIT 1"
        };

        let sni: Option<String> = sqlx::query_scalar(query)
            .bind(current_sni)
            .bind(tier)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to get next SNI")?;

        Ok(sni.unwrap_or_else(|| "www.google.com".to_string()))
    }

    /// Prefer pinned SNIs, then SNIs discovered by THIS node, fallback to global ones
    pub async fn get_best_sni_for_node(&self, node_id: i64) -> Result<String> {
        // 1. Try Pinned SNIs
        let pinned_sni: Option<String> = sqlx::query_scalar(
            r#"
            SELECT s.domain FROM sni_pool s
            JOIN node_pinned_snis nps ON s.id = nps.sni_id
            WHERE nps.node_id = $1 AND s.is_active = TRUE
            ORDER BY s.health_score DESC, RANDOM()
            LIMIT 1
            "#,
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(sni) = pinned_sni {
            return Ok(sni);
        }

        let is_relay: bool = sqlx::query_scalar("SELECT is_relay FROM nodes WHERE id = $1")
            .bind(node_id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(false);

        if is_relay {
            return self.get_next_sni("", 1, true).await;
        }

        // 2. Try node-specific discovered SNIs
        let node_sni: Option<String> = sqlx::query_scalar(
            "SELECT domain FROM sni_pool WHERE discovered_by_node_id = $1 AND is_active = TRUE ORDER BY health_score DESC LIMIT 1"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(sni) = node_sni {
            return Ok(sni);
        }

        // 3. Fallback to global best
        self.get_next_sni("", 1, false).await
    }

    pub async fn log_sni_rotation(
        &self,
        node_id: i64,
        old_sni: &str,
        new_sni: &str,
        reason: &str,
    ) -> Result<SniRotationLog> {
        let log = sqlx::query_as::<_, SniRotationLog>(
            "INSERT INTO sni_rotation_log (node_id, old_sni, new_sni, reason)
             VALUES ($1, $2, $3, $4)
             RETURNING id, node_id, old_sni, new_sni, reason, rotated_at",
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

    pub async fn rotate_node_sni(
        &self,
        node_id: i64,
        reason: &str,
    ) -> Result<(String, String, i64)> {
        // 1. Get current SNI and relay status
        let node_data: Option<(Option<String>, bool)> =
            sqlx::query_as("SELECT reality_sni, is_relay FROM nodes WHERE id = $1")
                .bind(node_id)
                .fetch_optional(&self.pool)
                .await?;

        let (current_sni, is_relay) = node_data.unwrap_or((None, false));
        let current_sni = current_sni.unwrap_or_else(|| "www.google.com".to_string());

        // 2. Get Next SNI (Pinned -> Premium for Relays -> Global)
        let pinned_sni: Option<String> = sqlx::query_scalar(
            r#"
            SELECT s.domain FROM sni_pool s
            JOIN node_pinned_snis nps ON s.id = nps.sni_id
            WHERE nps.node_id = $1 AND s.domain != $2 AND s.is_active = TRUE
            ORDER BY s.health_score DESC, RANDOM()
            LIMIT 1
            "#,
        )
        .bind(node_id)
        .bind(&current_sni)
        .fetch_optional(&self.pool)
        .await?;

        let next_sni = if let Some(sni) = pinned_sni {
            sni
        } else {
            self.get_next_sni(&current_sni, 1, is_relay).await?
        };

        if next_sni == current_sni {
            return Err(anyhow::anyhow!("No other SNI available"));
        }

        // 3. Update Node
        sqlx::query("UPDATE nodes SET reality_sni = $1 WHERE id = $2")
            .bind(&next_sni)
            .bind(node_id)
            .execute(&self.pool)
            .await?;

        // 4. Log
        let log = self
            .log_sni_rotation(node_id, &current_sni, &next_sni, reason)
            .await?;

        Ok((current_sni, next_sni, log.id))
    }
}
