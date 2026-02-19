use sqlx::PgPool;
use anyhow::Result;

pub struct ActivityService;

impl ActivityService {
    pub async fn log(pool: &PgPool, category: &str, event: &str) -> Result<()> {
        // Redirect legacy logging to new activity_log table
        // Map category -> action, event -> details
        sqlx::query("INSERT INTO activity_log (user_id, action, details, ip_address) VALUES (NULL, $1, $2, NULL)")
            .bind(category)
            .bind(event)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn log_tx<'a, E>(executor: E, user_id: Option<i64>, category: &str, event: &str) -> Result<()> 
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        // Schema shows: id, user_id, action (was category?), details (was event?), created_at
        // Wait, schema in migration 001 shows:
        // CREATE TABLE IF NOT EXISTS activity_log (id, user_id, action, details, ip_address, created_at)
        // But the existing log() function inserts into `panel_activities` (category, event)?
        // There seems to be TWO log tables or a mismatch.
        // Let's deduce based on existing code.
        // Existing code: "INSERT INTO panel_activities (category, event) VALUES (?, ?)"
        
        // However, in store_service.rs I called: 
        // log_tx(&mut *tx, Some(sub.user_id), "Refund", "details")
        
        // I should stick to `panel_activities` if that's what `log` uses, but `panel_activities` schema (inferred from log method) 
        // only has category/event. It might not have user_id.
        // Let's check the schema if possible, or just implement log_tx to match `log`.
        
        // If I use `panel_activities`, I can't store user_id easily unless I put it in details.
        // But `store_service` call expected (executor, user_id, category, event).
        
        // Warning: I might be mixing up `activity_log` (user centric) and `panel_activities` (system centric).
        // Let's look at migration 001 again.
        
        // For safety, I will implement log_tx to try using `activity_log` which DEFINITELY has user_id, 
        // OR `panel_activities` if `activity_log` is not used.
        // The error message says `log` uses `panel_activities`.
        
        // Let's check what `activity_log` table definition was.
        // Line 246 of migration: table activity_log (user_id, action, details).
        
        // I will implement `log_tx` to write to `activity_log` since that supports user_id.
        
        sqlx::query("INSERT INTO activity_log (user_id, action, details) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(category) // action
            .bind(event)    // details
            .execute(executor)
            .await?;
            
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_latest(pool: &PgPool, limit: i64) -> Result<Vec<caramba_db::models::activity::Activity>> {
        sqlx::query_as::<_, caramba_db::models::activity::Activity>(
            "SELECT id, action AS category, COALESCE(details, '') AS event, created_at FROM activity_log ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!(e))
    }
}
