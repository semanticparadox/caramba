use serde::{Deserialize, Serialize};
use std::fs;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendConfig {
    pub domain: String,
    pub panel_url: String,
    pub auth_token: String,
    pub region: String,
    pub listen_port: u16,
    pub geoip_db_path: Option<String>,
}

impl FrontendConfig {
    pub fn load() -> Result<Self> {
        // Try to load from /etc/caramba/frontend.toml first
        let config_paths = vec![
            "/etc/caramba/frontend.toml",
            "./frontend.toml",
        ];
        
        for path in config_paths {
            if let Ok(contents) = fs::read_to_string(path) {
                tracing::info!("Loading config from {}", path);
                return Ok(toml::from_str(&contents)?);
            }
        }
        
        // Fallback to environment variables
        tracing::info!("Loading config from environment");
        Ok(Self {
            domain: std::env::var("FRONTEND_DOMAIN")?,
            panel_url: std::env::var("PANEL_URL")?,
            auth_token: std::env::var("AUTH_TOKEN")?,
            region: std::env::var("REGION").unwrap_or_else(|_| "default".to_string()),
            listen_port: std::env::var("LISTEN_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            geoip_db_path: std::env::var("GEOIP_DB_PATH").ok(),
        })
    }
}
