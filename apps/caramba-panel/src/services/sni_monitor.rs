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

enum DomainHealth {
    Healthy,
    Unhealthy(String),
    Blacklist(String),
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
            let health = self.check_domain(&sni.domain).await;
            let should_be_active = match &health {
                DomainHealth::Healthy => {
                    sni.health_score = (sni.health_score + 5).clamp(0, 100);
                    sni.health_score > 30
                }
                DomainHealth::Unhealthy(_) => {
                    sni.health_score = (sni.health_score - 20).clamp(0, 100);
                    sni.health_score > 30
                }
                DomainHealth::Blacklist(reason) => {
                    sni.health_score = 0;
                    if let Err(e) = self.auto_blacklist_domain(&sni.domain, reason).await {
                        warn!(
                            "Failed to auto-blacklist domain {} (reason: {}): {}",
                            sni.domain, reason, e
                        );
                    }
                    false
                }
            };

            sqlx::query(
                "UPDATE sni_pool SET health_score = $1, is_active = $2, last_check = CURRENT_TIMESTAMP WHERE id = $3"
            )
            .bind(sni.health_score)
            .bind(should_be_active)
            .bind(sni.id)
            .execute(&self.pool)
            .await?;

            match &health {
                DomainHealth::Healthy => {}
                DomainHealth::Unhealthy(reason) => {
                    warn!(
                        "SNI domain {} failed health check ({}). New health: {}",
                        sni.domain, reason, sni.health_score
                    );
                }
                DomainHealth::Blacklist(reason) => {
                    warn!("SNI domain {} auto-blacklisted: {}", sni.domain, reason);
                }
            }
        }

        Ok(())
    }

    async fn auto_blacklist_domain(&self, domain: &str, reason: &str) -> Result<(), anyhow::Error> {
        sqlx::query(
            "INSERT INTO sni_blacklist (domain, reason) VALUES ($1, $2) ON CONFLICT (domain) DO NOTHING",
        )
        .bind(domain)
        .bind(format!("SNI monitor: {}", reason))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn check_domain(&self, domain: &str) -> DomainHealth {
        let url = format!("https://{}", domain);
        match self
            .client
            .get(&url)
            .header("Range", "bytes=0-2048")
            .send()
            .await
        {
            Ok(res) => {
                let status_obj = res.status();
                let status = status_obj.as_u16();
                if matches!(status, 401 | 403 | 451) {
                    return DomainHealth::Blacklist(format!("HTTP {}", status));
                }

                if status_obj.is_success() {
                    if let Ok(body) = res.text().await {
                        let body = body.to_ascii_lowercase();
                        const BODY_DENY_MARKERS: &[&str] = &[
                            "access denied",
                            "forbidden",
                            "not authorized",
                            "permission denied",
                            "security check",
                            "request blocked",
                        ];
                        if BODY_DENY_MARKERS.iter().any(|marker| body.contains(marker)) {
                            return DomainHealth::Blacklist(
                                "response body indicates access denied".to_string(),
                            );
                        }
                    }
                }

                if status_obj.is_success()
                    || status_obj.is_redirection()
                    || status_obj.is_client_error()
                    || status_obj.is_server_error()
                {
                    return DomainHealth::Healthy;
                }

                DomainHealth::Unhealthy(format!("unexpected HTTP status {}", status))
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                let is_tls = msg.contains("certificate")
                    || msg.contains("tls")
                    || msg.contains("handshake")
                    || msg.contains("dns name")
                    || msg.contains("unknown issuer")
                    || msg.contains("self signed");

                if is_tls {
                    DomainHealth::Blacklist("TLS/certificate validation failed".to_string())
                } else {
                    DomainHealth::Unhealthy(format!("request error: {}", e))
                }
            }
        }
    }
}
