use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Таймаут для всех запросов к панели.
/// Без таймаута зависший панель блокирует tokio-воркер навсегда.
const API_TIMEOUT_SECS: u64 = 15;

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: String,
}

impl ApiClient {
    pub fn new(base_url: String, token: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(API_TIMEOUT_SECS))
            .build()
            .expect("Failed to build reqwest client");
        Self {
            client,
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

    /// DELETE on /api/v2/bot{path}. Used for idempotent removals (cart clear,
    /// session kill, etc.). Returns Ok on any 2xx; body intentionally ignored.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = format!("{}/api/v2/bot{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .header("X-Bot-Token", &self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Request failed: {}", resp.status()));
        }
        Ok(())
    }

    pub fn has_token(&self) -> bool {
        !self.token.trim().is_empty()
    }

    /// Начисляет signup-бонусы после регистрации пользователя по реферальной ссылке.
    /// Возвращает (referrer_bonus_cents, referred_bonus_cents).
    pub async fn apply_referral_signup_bonus(
        &self,
        referrer_id: i64,
        referred_user_id: i64,
    ) -> Result<(i64, i64)> {
        #[derive(Deserialize)]
        struct SignupBonusResponse {
            referrer_bonus_cents: i64,
            referred_bonus_cents: i64,
        }

        let body = serde_json::json!({
            "referrer_id": referrer_id,
            "referred_user_id": referred_user_id,
        });

        let resp: SignupBonusResponse = self.post("/referral/signup-bonus", &body).await?;
        Ok((resp.referrer_bonus_cents, resp.referred_bonus_cents))
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
