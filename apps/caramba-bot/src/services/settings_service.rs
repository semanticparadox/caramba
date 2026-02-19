use crate::api_client::ApiClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct SettingsService {
    api: ApiClient,
    cache: Arc<RwLock<HashMap<String, String>>>,
}

impl SettingsService {
    pub fn new(api: ApiClient) -> Self {
        Self {
            api,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let cache = self.cache.read().await;
        if let Some(val) = cache.get(key) {
            return Some(val.clone());
        }
        drop(cache);

        // Fetch from API
        match self.api.get::<String>(&format!("/settings/{}", key)).await {
            Ok(val) => {
                let mut cache = self.cache.write().await;
                cache.insert(key.to_string(), val.clone());
                Some(val)
            }
            Err(_) => None,
        }
    }

    pub async fn get_or_default(&self, key: &str, default: &str) -> String {
        self.get(key).await.unwrap_or_else(|| default.to_string())
    }

    pub async fn set(&self, key: &str, value: &str) {
        let mut cache = self.cache.write().await;
        cache.insert(key.to_string(), value.to_string());
    }
}
