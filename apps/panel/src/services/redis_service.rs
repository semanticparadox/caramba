use anyhow::{Context, Result};
use bb8_redis::RedisConnectionManager;
use tracing::info;

#[derive(Clone)]
pub struct RedisService {
    pool: bb8::Pool<RedisConnectionManager>,
}

impl RedisService {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let manager = RedisConnectionManager::new(client);
        let pool = bb8::Pool::builder()
            .max_size(10)
            .build(manager)
            .await
            .context("Failed to create Redis pool")?;
        
        info!("✅ Redis connected successfully (Built-in bb8)");
        Ok(Self { pool })
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.pool.get().await.context("Failed to get Redis connection")?;
        let value: Option<String> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .context("Redis GET failed")?;
        Ok(value)
    }

    pub async fn ping(&self) -> Result<()> {
        let mut conn = self.pool.get().await.context("Failed to get Redis connection")?;
        let _: () = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .context("Redis PING failed")?;
        Ok(())
    }

    pub async fn set(&self, key: &str, value: &str, ttl_seconds: usize) -> Result<()> {
        let mut conn = self.pool.get().await.context("Failed to get Redis connection")?;
        let _: () = redis::cmd("SETEX")
            .arg(key)
            .arg(ttl_seconds)
            .arg(value)
            .query_async(&mut *conn)
            .await
            .context("Redis SETEX failed")?;
        Ok(())
    }

    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.pool.get().await.context("Failed to get Redis connection")?;
        let _: () = redis::cmd("DEL")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .context("Redis DEL failed")?;
        Ok(())
    }

    // --- Specific Caching Methods ---

    pub async fn cache_subscription(&self, sub_uuid: &str, config: &str) -> Result<()> {
        // Cache subscription config for 1 hour (3600s)
        let key = format!("sub_config:{}", sub_uuid);
        self.set(&key, config, 3600).await
    }

    pub async fn get_cached_subscription(&self, sub_uuid: &str) -> Result<Option<String>> {
        let key = format!("sub_config:{}", sub_uuid);
        self.get(&key).await
    }

    pub async fn invalidate_subscription(&self, sub_uuid: &str) -> Result<()> {
        let key = format!("sub_config:{}", sub_uuid);
        self.del(&key).await
    }

    // --- Rate Limiting ---

    pub async fn check_rate_limit(&self, key: &str, limit: usize, window_secs: usize) -> Result<bool> {
        let mut conn = self.pool.get().await.context("Failed to get Redis connection")?;
        
        // Simple Fixed Window: INCR key. If 1, set expiration.
        let count: usize = redis::cmd("INCR")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .context("Redis INCR failed")?;

        if count == 1 {
            let _: () = redis::cmd("EXPIRE")
                .arg(key)
                .arg(window_secs)
                .query_async(&mut *conn)
                .await
                .context("Redis EXPIRE failed")?;
        }

        Ok(count <= limit)
    }
    pub async fn get_connection(&self) -> Result<bb8::PooledConnection<'_, RedisConnectionManager>> {
        self.pool.get().await.context("Failed to get Redis connection")
    }
}
