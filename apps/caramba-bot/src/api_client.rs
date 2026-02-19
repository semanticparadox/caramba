use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: String,
}

impl ApiClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            token,
        }
    }

    pub async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}/api/v2/bot{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("X-Bot-Token", &self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Request failed: {}", resp.status()));
        }

        Ok(resp.json().await?)
    }

    pub async fn post<T: for<'de> Deserialize<'de>, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}/api/v2/bot{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .header("X-Bot-Token", &self.token)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Request failed: {}", resp.status()));
        }

        Ok(resp.json().await?)
    }

    // Add more methods as needed by handlers
}
