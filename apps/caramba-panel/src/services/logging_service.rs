use sqlx::PgPool;
use anyhow::Result;
use serde::Serialize;

#[derive(sqlx::FromRow, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>, 
    pub username: Option<String>, 
}

pub struct LoggingService;

impl LoggingService {
    pub async fn log_system(pool: &PgPool, action: &str, details: &str) -> Result<()> {
        Self::log_internal(pool, None, action, Some(details), None).await
    }

    pub async fn log_user(pool: &PgPool, user_id: Option<i64>, action: &str, details: &str, ip: Option<&str>) -> Result<()> {
        Self::log_internal(pool, user_id, action, Some(details), ip).await
    }

    async fn log_internal(pool: &PgPool, user_id: Option<i64>, action: &str, details: Option<&str>, ip: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO activity_log (user_id, action, details, ip_address) VALUES ($1, $2, $3, $4)"
        )
        .bind(user_id)
        .bind(action)
        .bind(details)
        .bind(ip)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_logs(
        pool: &PgPool, 
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
        
        let mut bind_index = 1;
        if let Some(cat) = &category_filter {
            if !cat.is_empty() {
                query.push_str(&format!(" WHERE l.action = ${}", bind_index));
                bind_index += 1;
            }
        }

        query.push_str(&format!(" ORDER BY l.created_at DESC LIMIT ${} OFFSET ${}", bind_index, bind_index + 1));

        let mut q = sqlx::query_as::<_, LogEntry>(&query);

        if let Some(cat) = &category_filter {
            if !cat.is_empty() {
                q = q.bind(cat);
            }
        }
        
        q = q.bind(limit).bind(offset);

        let logs = q.fetch_all(pool).await?;
        Ok(logs)
    }

    pub async fn get_categories(pool: &PgPool) -> Result<Vec<String>> {
        let cats: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT action FROM activity_log ORDER BY action")
            .fetch_all(pool)
            .await?;
        Ok(cats.into_iter().map(|(s,)| s).collect())
    }
}
