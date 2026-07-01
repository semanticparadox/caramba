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

    /// Записывает настройку на панель и обновляет локальный кэш.
    ///
    /// Раньше этот метод писал ТОЛЬКО в кэш (значение терялось при рестарте и
    /// не было видно панели/клиенту). Теперь он сперва персистит через
    /// `api.set_setting` (POST /api/v2/bot/settings/{key}); кэш обновляется лишь
    /// при успешной записи, чтобы кэш не расходился с базой панели.
    pub async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.api.set_setting(key, value).await?;
        let mut cache = self.cache.write().await;
        cache.insert(key.to_string(), value.to_string());
        Ok(())
    }

    /// Пишет значение ТОЛЬКО в локальный кэш, без обращения к панели.
    /// Используется для эфемерных, не-персистентных значений вроде
    /// `bot_username` (footer), которые панель не принимает на запись.
    /// Это прежнее поведение `set` до того, как `set` стал персистить brand_*.
    pub async fn set_local(&self, key: &str, value: &str) {
        let mut cache = self.cache.write().await;
        cache.insert(key.to_string(), value.to_string());
    }

    /// Сбрасывает кэш одного ключа, чтобы следующий `get` сходил на панель.
    /// Нужно, когда значение могло измениться вне бота (например, в панели).
    pub async fn invalidate(&self, key: &str) {
        self.cache.write().await.remove(key);
    }
}
