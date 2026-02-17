use std::sync::Arc;
use std::time::Duration;
use sqlx::PgPool;
use anyhow::Result;
use crate::services::generator_service::GeneratorService;
use tracing::{info, error};
use chrono::{Utc, DateTime};

#[derive(sqlx::FromRow)]
struct PendingRotation {
    id: i64,
    last_rotated_at: Option<DateTime<Utc>>,
    created_at: Option<DateTime<Utc>>,
    renew_interval_mins: i32, 
}

pub struct RotationService {
    pool: PgPool,
    generator: Arc<GeneratorService>,
}

impl RotationService {
    pub fn new(pool: PgPool, generator: Arc<GeneratorService>) -> Self {
        Self { pool, generator }
    }

    pub async fn start(&self) {
        info!("Starting RotationService background worker...");
        let mut interval = tokio::time::interval(Duration::from_secs(600)); 

        loop {
            interval.tick().await;
            if let Err(e) = self.check_and_rotate_all().await {
                error!("Error during rotation check: {}", e);
            }
        }
    }

    async fn check_and_rotate_all(&self) -> Result<()> {
        let pending = sqlx::query_as::<_, PendingRotation>(
            r#"
            SELECT id, last_rotated_at, created_at, renew_interval_mins
            FROM inbounds 
            WHERE renew_interval_mins > 0 AND enable = TRUE
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let now = Utc::now();

        for row in pending {
            let last_val = row.last_rotated_at.or(row.created_at);
            
            if let Some(last_utc) = last_val {
                let mins_elapsed = (now - last_utc).num_minutes();
                
                if mins_elapsed >= row.renew_interval_mins as i64 {
                    info!("Inbound {} reached renewal interval ({}m). Rotating...", row.id, row.renew_interval_mins);
                    if let Err(e) = self.generator.rotate_inbound(row.id).await {
                        error!("Failed to rotate inbound {}: {}", row.id, e);
                    }
                }
            }
        }

        Ok(())
    }
}
