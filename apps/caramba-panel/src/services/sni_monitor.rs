use caramba_db::repositories::sni_repo::SniRepository;
use reqwest::Client;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{error, info, warn};

pub struct SniMonitor {
    pool: PgPool,
    sni_repo: SniRepository,
    client: Client,
}

impl SniMonitor {
    pub fn new(pool: PgPool, sni_repo: SniRepository) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self {
            pool,
            sni_repo,
            client,
        }
    }

    pub async fn start(&self) {
        info!("Starting SNIMonitor service...");
        let mut interval = tokio::time::interval(Duration::from_secs(3600));

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

            let should_be_active = sni.health_score > 30;

            sqlx::query(
                "UPDATE sni_pool SET health_score = $1, is_active = $2, last_check = CURRENT_TIMESTAMP WHERE id = $3"
            )
            .bind(sni.health_score)
            .bind(should_be_active)
            .bind(sni.id)
            .execute(&self.pool)
            .await?;

            if !is_healthy {
                warn!(
                    "SNI domain {} failed health check. New health: {}",
                    sni.domain, sni.health_score
                );
            }
        }

        Ok(())
    }

    async fn check_domain(&self, domain: &str) -> bool {
        let url = format!("https://{}", domain);
        match self.client.head(&url).send().await {
            Ok(res) => {
                res.status().is_success()
                    || res.status().is_redirection()
                    || res.status().as_u16() >= 400
            }
            Err(_) => false,
        }
    }
}
