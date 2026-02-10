use std::time::Duration;
use sqlx::SqlitePool;
use crate::repositories::sni_repo::SniRepository;
use tracing::{info, warn, error};
use reqwest::Client;

pub struct SniMonitor {
    pool: SqlitePool,
    sni_repo: SniRepository,
    client: Client,
}

impl SniMonitor {
    pub fn new(pool: SqlitePool, sni_repo: SniRepository) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            // We want to verify the domain itself, so we use actual TLS
            .build()
            .unwrap_or_default();
            
        Self { pool, sni_repo, client }
    }

    pub async fn start(&self) {
        info!("Starting SNIMonitor service...");
        let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Every hour

        loop {
            interval.tick().await;
            if let Err(e) = self.check_all_snis().await {
                error!("Error during SNI health check: {}", e);
            }
        }
    }

    async fn check_all_snis(&self) -> Result<(), anyhow::Error> {
        let snis = self.sni_repo.get_all_snis().await?;
        
        for mut sni in snis {
            let is_healthy = self.check_domain(&sni.domain).await;
            
            let health_change = if is_healthy { 5 } else { -20 };
            sni.health_score = (sni.health_score + health_change).clamp(0, 100);
            
            // Auto-disable if health is too low
            let should_be_active = sni.health_score > 30;
            
            sqlx::query(
                "UPDATE sni_pool SET health_score = ?, is_active = ?, last_check = datetime('now') WHERE id = ?"
            )
            .bind(sni.health_score)
            .bind(should_be_active)
            .bind(sni.id)
            .execute(&self.pool)
            .await?;
            
            if !is_healthy {
                warn!("SNI domain {} failed health check. New health: {}", sni.domain, sni.health_score);
            }
        }
        
        Ok(())
    }

    async fn check_domain(&self, domain: &str) -> bool {
        // Try HTTPS HEAD request
        let url = format!("https://{}", domain);
        match self.client.head(&url).send().await {
            Ok(res) => {
                // If we get any response (even 404 or 403), the TLS handshake succeeded
                // and the domain is reachable at the SNI level.
                res.status().is_success() || res.status().is_redirection() || res.status().as_u16() >= 400
            }
            Err(_) => false,
        }
    }
}
