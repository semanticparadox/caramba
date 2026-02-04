use sqlx::SqlitePool;
use anyhow::Result;
use serde::Serialize;

#[derive(sqlx::FromRow, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: String, // String for simplicity in display, or Chrono DateTime
    // Join fields (optional, if we join with users table)
    pub username: Option<String>, 
}

pub struct LoggingService;

impl LoggingService {
    /// Log a system event (no user associated)
    pub async fn log_system(pool: &SqlitePool, action: &str, details: &str) -> Result<()> {
        Self::log_internal(pool, None, action, Some(details), None).await
    }

    /// Log a user event
    pub async fn log_user(pool: &SqlitePool, user_id: Option<i64>, action: &str, details: &str, ip: Option<&str>) -> Result<()> {
        Self::log_internal(pool, user_id, action, Some(details), ip).await
    }

    async fn log_internal(pool: &SqlitePool, user_id: Option<i64>, action: &str, details: Option<&str>, ip: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO activity_log (user_id, action, details, ip_address) VALUES (?, ?, ?, ?)"
        )
        .bind(user_id)
        .bind(action)
        .bind(details)
        .bind(ip)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Fetch logs with optional filtering
    /// We join with users table to get usernames for display
    pub async fn get_logs(
        pool: &SqlitePool, 
        limit: i64, 
        offset: i64,
        category_filter: Option<String>,
    ) -> Result<Vec<LogEntry>> {
        let mut query = String::from(
            "SELECT 
                l.id, l.user_id, l.action, l.details, l.ip_address, l.created_at,
                u.username
             FROM activity_log l
             LEFT JOIN users u ON l.user_id = u.id"
        );
        
        
        if let Some(cat) = &category_filter {
            if !cat.is_empty() {
                query.push_str(" WHERE l.action = ?");
            }
        }

        query.push_str(" ORDER BY l.created_at DESC LIMIT ? OFFSET ?");

        let mut q = sqlx::query_as::<_, LogEntry>(&query);

        if let Some(cat) = &category_filter {
            if !cat.is_empty() {
                q = q.bind(cat);
            }
        }
        
        // LIMIT and OFFSET binding
        q = q.bind(limit).bind(offset);

        let logs = q.fetch_all(pool).await?;
        Ok(logs)
    }

    /// Get distinct categories for filter dropdown
    pub async fn get_categories(pool: &SqlitePool) -> Result<Vec<String>> {
        let cats: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT action FROM activity_log ORDER BY action")
            .fetch_all(pool)
            .await?;
        Ok(cats.into_iter().map(|(s,)| s).collect())
    }
}
