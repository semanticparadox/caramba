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

    pub fn has_token(&self) -> bool {
        !self.token.trim().is_empty()
    }

    pub async fn poll_worker_update(
        &self,
        role: &str,
        worker_id: &str,
        current_version: &str,
    ) -> Result<WorkerUpdatePollResponse> {
        let url = format!(
            "{}/api/internal/workers/{}/updates/poll?worker_id={}&current_version={}",
            self.base_url,
            role,
            urlencoding::encode(worker_id),
            urlencoding::encode(current_version)
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Worker poll request failed: {}",
                resp.status()
            ));
        }

        Ok(resp.json().await?)
    }

    pub async fn report_worker_update(
        &self,
        role: &str,
        report: &WorkerUpdateReportRequest,
    ) -> Result<()> {
        let url = format!(
            "{}/api/internal/workers/{}/updates/report",
            self.base_url, role
        );
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(report)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Worker report request failed: {}",
                resp.status()
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerUpdatePollResponse {
    pub update: bool,
    pub target_version: Option<String>,
    pub asset_url: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerUpdateReportRequest {
    pub worker_id: String,
    pub current_version: String,
    pub target_version: String,
    pub status: String,
    pub message: Option<String>,
}
