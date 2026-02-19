use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, error, info};

#[derive(Debug, Deserialize, Clone)]
pub struct DecoySettingsResponse {
    pub decoy: DecoySettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DecoySettings {
    pub enabled: bool,
    pub urls: Vec<String>,
    pub min_interval: u64,
    pub max_interval: u64,
}

pub struct DecoyService {
    client: Client,
    panel_url: String,
    token: String,
    current_settings: Option<DecoySettings>,
}

impl DecoyService {
    pub fn new(panel_url: String, token: String) -> Self {
        Self {
            client: Client::builder()
                .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            panel_url,
            token,
            current_settings: None,
        }
    }

    pub async fn run_loop(mut self) {
        info!("🎭 Decoy Traffic Service started");

        loop {
            // 1. Fetch Latest Settings
            if let Err(e) = self.refresh_settings().await {
                error!("Failed to fetch decoy settings: {}. Retrying in 60s.", e);
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }

            let settings = if let Some(s) = &self.current_settings {
                if !s.enabled || s.urls.is_empty() {
                    debug!("Decoy traffic disabled or no URLs. Sleeping 60s.");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
                s.clone()
            } else {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            };

            // 2. Calculate Sleep Time
            let delay = rand::random_range(settings.min_interval..=settings.max_interval);

            info!("🎭 Next decoy request in {} seconds...", delay);
            tokio::time::sleep(Duration::from_secs(delay)).await;

            // 3. Send Request
            let target_url = {
                let idx = rand::random_range(0..settings.urls.len());
                &settings.urls[idx]
            };

            info!("🎭 Sending decoy traffic to: {}", target_url);
            match self.client.get(target_url).send().await {
                Ok(resp) => {
                    debug!("Decoy Success: Status {}", resp.status());
                    // Discard body
                    let _ = resp.bytes().await;
                }
                Err(e) => error!("Decoy Request Failed: {}", e),
            }
        }
    }

    async fn refresh_settings(&mut self) -> anyhow::Result<()> {
        let url = format!("{}/api/v2/node/settings", self.panel_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Server returned {}", resp.status());
        }

        let json: DecoySettingsResponse = resp.json().await?;
        self.current_settings = Some(json.decoy);
        Ok(())
    }
}
