use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct PanelClient {
    client: Client,
    base_url: String,
    auth_token: String,
}

impl PanelClient {
    pub fn new(base_url: String, auth_token: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            auth_token,
        }
    }

    pub async fn get_subscription(&self, uuid: &str) -> Result<Subscription> {
        let url = format!("{}/api/internal/subscriptions/{}", self.base_url, uuid);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.auth_token)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json().await?)
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<InternalNode>> {
        let url = format!("{}/api/internal/nodes/active", self.base_url);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.auth_token)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json().await?)
    }

    pub async fn get_user_keys(&self, user_id: i64) -> Result<UserKeys> {
        let url = format!("{}/api/internal/users/{}/keys", self.base_url, user_id);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.auth_token)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json().await?)
    }

    pub async fn send_heartbeat(&self, domain: &str, stats: FrontendStats) -> Result<()> {
        let url = format!("{}/api/admin/frontends/{}/heartbeat", self.base_url, domain);

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.auth_token)
            .json(&stats)
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        // Compatibility fallback for hub/local mode where frontend registration is absent.
        if matches!(response.status().as_u16(), 401 | 403 | 404) {
            let legacy_url = format!("{}/api/internal/frontend/heartbeat", self.base_url);
            self.client
                .post(&legacy_url)
                .bearer_auth(&self.auth_token)
                .json(&stats)
                .send()
                .await?
                .error_for_status()?;
            return Ok(());
        }

        Err(anyhow::anyhow!(
            "frontend heartbeat failed with status {}",
            response.status()
        ))
    }
}

// Data structures (detailed for config generation)
#[derive(Debug, Serialize, Deserialize)]
pub struct Subscription {
    pub id: i64,
    pub user_id: i64,
    pub status: String,
    pub used_traffic: i64,
    pub subscription_uuid: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Node {
    pub id: i64,
    pub name: String,
    pub ip: String,
    pub vpn_port: i64,
    pub reality_pub: Option<String>,
    pub reality_priv: Option<String>, // Added for generator
    pub short_id: Option<String>,
    pub domain: Option<String>,
    pub country_code: Option<String>,
    pub is_relay: bool,             // Added
    pub relay_id: Option<i64>,      // Added
    pub join_token: Option<String>, // Added

    // config_* fields might be needed for blocking logic
    #[serde(default)]
    pub config_block_torrent: bool,
    #[serde(default)]
    pub config_block_ads: bool,
    #[serde(default)]
    pub config_block_porn: bool,

    #[serde(default)]
    pub reality_sni: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InternalNode {
    #[serde(flatten)]
    pub node: Node,
    pub inbounds: Vec<Inbound>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Inbound {
    pub id: i64,
    pub node_id: i64,
    pub tag: String,
    pub protocol: String,
    pub listen_ip: String,
    pub listen_port: i32,
    pub settings: String,
    pub stream_settings: String,
    pub enable: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserKeys {
    pub user_uuid: String,
    pub hy2_password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrontendStats {
    pub requests_count: u64,
    pub bandwidth_used: u64,
}
