use sqlx::PgPool;
use anyhow::{Result, Context};
use crate::models::api_key::ApiKey;

#[derive(Clone, Debug)]
pub struct ApiKeyRepository {
    pool: PgPool,
}

impl ApiKeyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, name: &str, key: &str, max_uses: Option<i64>) -> Result<ApiKey> {
        let rec = sqlx::query_as::<_, ApiKey>(
            r#"
            INSERT INTO api_keys (name, key, type, max_uses)
            VALUES ($1, $2, 'enrollment', $3)
            RETURNING id, key, name, type as "key_type", max_uses, current_uses, is_active, expires_at, created_at, created_by
            "#
        )
        .bind(name)
        .bind(key)
        .bind(max_uses)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create API key")?;

        Ok(rec)
    }

    pub async fn get_all(&self) -> Result<Vec<ApiKey>> {
        let recs = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT id, key, name, type as "key_type", max_uses, current_uses, is_active, expires_at, created_at, created_by 
            FROM api_keys
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch API keys")?;

        Ok(recs)
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query(
            "DELETE FROM api_keys WHERE id = $1"
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to delete API key")?;

        Ok(())
    }
}
