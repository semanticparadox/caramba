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
        let has_favorites: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sni_pool WHERE is_favorite = TRUE AND is_active = TRUE)",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false);

        let query = if has_favorites {
            "SELECT domain FROM sni_pool
             WHERE domain != $1 AND is_active = TRUE AND is_favorite = TRUE
             ORDER BY health_score DESC, RANDOM()
             LIMIT 1"
        } else if premium_only {
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

    /// Strict SNI Selection Hierarchy: Pinned -> Favorites -> High Health Verified Scans -> Relay Whitelist
    pub async fn get_best_sni_for_node(&self, node_id: i64) -> Result<String> {
        // 1. Absolute Priority: Pinned node-specific SNIs
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

        // 2. Global Favorites
        let favorite_sni: Option<String> = sqlx::query_scalar(
            "SELECT domain FROM sni_pool WHERE is_favorite = TRUE AND is_active = TRUE ORDER BY health_score DESC, RANDOM() LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(sni) = favorite_sni {
            return Ok(sni);
        }

        // 3. Internal Relay Rule: Use ONLY verified Roskomnadzor whitelist
        // We use country_code 'RU' or is_relay flag.
        let node_info: Option<(bool, Option<String>)> =
            sqlx::query_as("SELECT is_relay, country_code FROM nodes WHERE id = $1")
                .bind(node_id)
                .fetch_optional(&self.pool)
                .await?;

        if let Some((is_relay, country_code)) = node_info {
            let is_ru = country_code.unwrap_or_default().to_uppercase() == "RU";
            if is_relay || is_ru {
                // Russian internal relay logic: Pick from Premium (Whitelist)
                return self.get_next_sni("", 1, true).await;
            }
        }

        // 4. Verified Scanner Results: Pick highest-health SNI discovered by this node
        let node_sni: Option<String> = sqlx::query_scalar(
            "SELECT domain FROM sni_pool WHERE discovered_by_node_id = $1 AND is_active = TRUE ORDER BY health_score DESC LIMIT 1"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(sni) = node_sni {
            return Ok(sni);
        }

        // 5. Fallback: Global Verified pool (Not hardcoded gosuslugi)
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

        // 3. Update Node (и фиксируем метку ротации для circuit-breaker)
        sqlx::query(
            "UPDATE nodes SET reality_sni = $1, last_sni_rotation = NOW() WHERE id = $2",
        )
        .bind(&next_sni)
        .bind(node_id)
        .execute(&self.pool)
        .await?;

        // 4. Blocklist the old SNI if the rotation is due to a validation failure
        if reason.starts_with("Validation failed:") || reason.starts_with("Auto-Heal:") {
            let sni_repo =
                caramba_db::repositories::sni_repo::SniRepository::new(self.pool.clone());
            if let Err(e) = sni_repo.add_to_blocklist(&current_sni, reason).await {
                tracing::warn!("Failed to add SNI '{}' to blocklist: {}", current_sni, e);
            }
        }

        // 5. Log
        let log = self
            .log_sni_rotation(node_id, &current_sni, &next_sni, reason)
            .await?;

        Ok((current_sni, next_sni, log.id))
    }
}
