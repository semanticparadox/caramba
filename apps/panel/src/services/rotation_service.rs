use std::sync::Arc;
use std::time::Duration;
use sqlx::SqlitePool;
use anyhow::Result;
use crate::services::generator_service::GeneratorService;
use tracing::{info, error};
use chrono::{Utc, DateTime, NaiveDateTime};

#[derive(sqlx::FromRow)]
struct PendingRotation {
    id: i64,
    last_rotated_at: Option<NaiveDateTime>,
    created_at: Option<NaiveDateTime>,
    renew_interval_hours: i64,
}

pub struct RotationService {
    pool: SqlitePool,
    generator: Arc<GeneratorService>,
}

impl RotationService {
    pub fn new(pool: SqlitePool, generator: Arc<GeneratorService>) -> Self {
        Self { pool, generator }
    }

    pub async fn start(&self) {
        info!("Starting RotationService background worker...");
        let mut interval = tokio::time::interval(Duration::from_secs(600)); // Every 10 minutes

        loop {
            interval.tick().await;
            if let Err(e) = self.check_and_rotate_all().await {
                error!("Error during rotation check: {}", e);
            }
        }
    }

    async fn check_and_rotate_all(&self) -> Result<()> {
        // Find inbounds that need rotation
        // Join with templates to get renewal interval
        let pending = sqlx::query_as::<_, PendingRotation>(
            r#"
            SELECT i.id, i.last_rotated_at, i.created_at, t.renew_interval_hours
            FROM inbounds i
            JOIN inbound_templates t ON i.tag = 'tpl_' || t.id
            WHERE t.renew_interval_hours > 0 AND t.is_active = 1
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let now = Utc::now();

        for row in pending {
            let last_val = row.last_rotated_at.or(row.created_at);
            
            if let Some(last_date) = last_val {
                // Conver to Utc
                let last_utc: DateTime<Utc> = DateTime::from_naive_utc_and_offset(last_date, Utc);
                let hours_elapsed = (now - last_utc).num_hours();
                
                if hours_elapsed >= row.renew_interval_hours {
                    info!("Inbound {} reached renewal interval ({}h). Rotating...", row.id, row.renew_interval_hours);
                    if let Err(e) = self.generator.rotate_inbound(row.id).await {
                        error!("Failed to rotate inbound {}: {}", row.id, e);
                    }
                }
            }
        }

        Ok(())
    }
}
