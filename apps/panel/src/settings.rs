use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;
use anyhow::{Context, Result};
use tracing::info;


#[derive(Debug, Clone)]
pub struct SettingsService {
    pool: SqlitePool,
    cache: Arc<RwLock<HashMap<String, String>>>,
}

impl SettingsService {
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        let service = Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
        };
        
        service.reload_cache().await?;
        Ok(service)
    }

    pub async fn reload_cache(&self) -> Result<()> {
        info!("Reloading settings cache from database");
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch settings from DB")?;

        let mut cache = self.cache.write().await;
        cache.clear();
        for (key, value) in rows {
            cache.insert(key, value);
        }
        
        info!("Cache reloaded with {} items", cache.len());
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let cache = self.cache.read().await;
        cache.get(key).cloned()
    }

    pub async fn get_or_default(&self, key: &str, default: &str) -> String {
        self.get(key).await.unwrap_or_else(|| default.to_string())
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) 
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .context("Failed to update setting in DB")?;

        let mut cache = self.cache.write().await;
        cache.insert(key.to_string(), value.to_string());
        
        Ok(())
    }

    pub async fn set_multiple(&self, settings: HashMap<String, String>) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for (key, value) in &settings {
            let _ = sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?, ?) 
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP"
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .context(format!("Failed to update setting {}", key))?;
        }

        tx.commit().await?;

        let mut cache = self.cache.write().await;
        for (key, value) in settings {
            cache.insert(key, value);
        }

        Ok(())
    }
}
