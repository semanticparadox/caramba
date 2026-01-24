use sqlx::SqlitePool;
use anyhow::Result;

pub struct ActivityService;

impl ActivityService {
    pub async fn log(pool: &SqlitePool, category: &str, event: &str) -> Result<()> {
        sqlx::query("INSERT INTO panel_activities (category, event) VALUES (?, ?)")
            .bind(category)
            .bind(event)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_latest(pool: &SqlitePool, limit: i64) -> Result<Vec<crate::models::activity::Activity>> {
        sqlx::query_as::<_, crate::models::activity::Activity>(
            "SELECT id, category, event, created_at FROM panel_activities ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!(e))
    }
}
