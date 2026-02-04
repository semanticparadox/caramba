use sqlx::SqlitePool;
use anyhow::Result;
use chrono::Utc;

pub struct AnalyticsService;

#[allow(dead_code)]
impl AnalyticsService {
    /// Track a new user registration (increments daily_stats.new_users)
    pub async fn track_new_user(pool: &SqlitePool) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, new_users) VALUES (?, 1) 
             ON CONFLICT(date) DO UPDATE SET new_users = new_users + 1, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(today)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Track user activity (logins/interactions). Ensures unique DAU count.
    pub async fn track_active_user(pool: &SqlitePool, user_id: i64) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        
        // 1. Insert into unique user_daily_activity to ensure uniqueness
        let res = sqlx::query(
            "INSERT OR IGNORE INTO user_daily_activity (user_id, date) VALUES (?, ?)"
        )
        .bind(user_id)
        .bind(&today)
        .execute(pool)
        .await?;
        
        // 2. If inserted (rows_affected > 0), increment aggregate daily_stats
        if res.rows_affected() > 0 {
             sqlx::query(
                "INSERT INTO daily_stats (date, active_users) VALUES (?, 1) 
                 ON CONFLICT(date) DO UPDATE SET active_users = active_users + 1, updated_at = CURRENT_TIMESTAMP"
            )
            .bind(today)
            .execute(pool)
            .await?;
        }
        
        Ok(())
    }

    /// Track revenue (in cents)
    pub async fn track_revenue(pool: &SqlitePool, amount_cents: i64) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, total_revenue) VALUES (?, ?) 
             ON CONFLICT(date) DO UPDATE SET total_revenue = total_revenue + ?, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(&today)
        .bind(amount_cents)
        .bind(amount_cents)
        .execute(pool)
        .await?;
        Ok(())
    }
    
    /// Track order count
    pub async fn track_order(pool: &SqlitePool) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, total_orders) VALUES (?, 1) 
             ON CONFLICT(date) DO UPDATE SET total_orders = total_orders + 1, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(today)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update daily traffic usage
    pub async fn track_traffic(pool: &SqlitePool, bytes: i64) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, traffic_used) VALUES (?, ?) 
             ON CONFLICT(date) DO UPDATE SET traffic_used = traffic_used + ?, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(&today)
        .bind(bytes)
        .bind(bytes)
        .execute(pool)
        .await?;
        Ok(())
    }

    // --- Analytics Queries ---

    /// Get top 10 users by traffic usage (from subscriptions)
    pub async fn get_top_users(pool: &SqlitePool) -> Result<Vec<TopUser>> {
        let users = sqlx::query_as::<_, TopUser>(
            "SELECT 
                u.username, 
                SUM(s.used_traffic) as total_traffic 
             FROM users u
             JOIN subscriptions s ON u.id = s.user_id
             WHERE s.used_traffic > 0
             GROUP BY u.id
             ORDER BY total_traffic DESC
             LIMIT 10"
        )
        .fetch_all(pool)
        .await?;
        Ok(users)
    }

    /// Get traffic distribution by node
    pub async fn get_node_traffic_stats(pool: &SqlitePool) -> Result<Vec<NodeTraffic>> {
        let nodes = sqlx::query_as::<_, NodeTraffic>(
            "SELECT 
                n.name, 
                COALESCE(SUM(s.used_traffic), 0) as total_traffic
             FROM nodes n
             LEFT JOIN subscriptions s ON n.id = s.node_id
             WHERE n.status = 'active'
             GROUP BY n.id
             ORDER BY total_traffic DESC"
        )
        .fetch_all(pool)
        .await?;
        Ok(nodes)
    }

    /// Get daily traffic history for last 30 days
    pub async fn get_traffic_history(pool: &SqlitePool) -> Result<Vec<DailyTraffic>> {
        let history = sqlx::query_as::<_, DailyTraffic>(
            "SELECT 
                date, 
                traffic_used 
             FROM daily_stats 
             ORDER BY date DESC 
             LIMIT 30"
        )
        .fetch_all(pool)
        .await?;
        Ok(history)
    }
}

// --- Structs ---

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct TopUser {
    pub username: Option<String>,
    pub total_traffic: i64, // Used only for SQL mapping, will need formatting in UI
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct NodeTraffic {
    pub name: String,
    pub total_traffic: i64,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct DailyTraffic {
    pub date: String,
    pub traffic_used: i64,
}
