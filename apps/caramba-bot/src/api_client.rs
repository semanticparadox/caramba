use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: String,
}

#[derive(Serialize, Deserialize)]
pub struct User {
    pub telegram_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub language_code: Option<String>,
    pub balance: f64,
    pub role: String,
}

#[derive(Serialize, Deserialize)]
pub struct Plan {
    pub id: i32,
    pub name: String,
    pub price: f64,
    pub duration_days: i32,
    pub traffic_limit_gb: Option<i32>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub status: String,
    pub expires_at: Option<String>, // ISO8601
    // ...
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
        let resp = self.client.get(&url)
            .header("X-Bot-Token", &self.token)
            .send()
            .await?;
        
        if !resp.status().is_success() {
             return Err(anyhow::anyhow!("Request failed: {}", resp.status()));
        }
        
        Ok(resp.json().await?)
    }

    pub async fn post<T: for<'de> Deserialize<'de>, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}/api/v2/bot{}", self.base_url, path);
        let resp = self.client.post(&url)
            .header("X-Bot-Token", &self.token)
            .json(body)
            .send()
            .await?;
            
        if !resp.status().is_success() {
             return Err(anyhow::anyhow!("Request failed: {}", resp.status()));
        }
        
        Ok(resp.json().await?)
    }

    pub async fn get_user(&self, telegram_id: i64) -> Result<Option<User>> {
        // Implement get user
        // Returning 404 means None
        let url = format!("{}/api/v2/bot/users/{}", self.base_url, telegram_id);
        let resp = self.client.get(&url)
            .header("X-Bot-Token", &self.token)
            .send()
            .await?;
            
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
             return Err(anyhow::anyhow!("Request failed: {}", resp.status()));
        }
        Ok(Some(resp.json().await?))
    }

    pub async fn create_user(&self, user: &User) -> Result<User> {
        self.post("/users", user).await
    }
    
    // Add more methods as needed by handlers
}
