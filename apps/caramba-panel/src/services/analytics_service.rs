use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AnalyticsService {
    pool: PgPool,
}

#[derive(serde::Serialize)]
pub struct SystemStats {
    pub active_nodes: i64,
    pub total_users: i64,
    pub active_subs: i64,
    pub total_revenue: f64,
    pub total_traffic_bytes: i64,
    pub total_traffic_30d_bytes: i64,
}

impl AnalyticsService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_system_stats(&self) -> Result<SystemStats> {
        let active_nodes =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM nodes WHERE status = 'active'")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
        let total_users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let active_subs = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM subscriptions WHERE status = 'active'",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let revenue_cents = sqlx::query_scalar::<_, i64>(
            "SELECT SUM(total_amount) FROM orders WHERE status = 'completed'",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let total_revenue = revenue_cents as f64 / 100.0;

        let total_traffic_bytes =
            sqlx::query_scalar::<_, i64>("SELECT SUM(total_ingress + total_egress) FROM nodes")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);

        // Честное 30-дневное окно. Раньше сюда подставлялся тот же алл-тайм
        // счётчик узлов с комментарием «Placeholder», и подпись «30d» в
        // админке врала тем сильнее, чем дольше жили узлы.
        let total_traffic_30d_bytes = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT SUM(up_bytes + down_bytes)::bigint
             FROM app_traffic_daily
             WHERE day >= CURRENT_DATE - 29",
        )
        .fetch_one(&self.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

        Ok(SystemStats {
            active_nodes,
            total_users,
            active_subs,
            total_revenue,
            total_traffic_bytes,
            total_traffic_30d_bytes,
        })
    }

    /// Track a new user registration (increments daily_stats.new_users)
    pub async fn track_new_user(pool: &PgPool) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, new_users) VALUES ($1, 1) 
             ON CONFLICT(date) DO UPDATE SET new_users = daily_stats.new_users + 1, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(today)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Track user activity (logins/interactions). Ensures unique DAU count.
    pub async fn track_active_user(pool: &PgPool, user_id: i64) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();

        // 1. Insert into unique user_daily_activity to ensure uniqueness
        let res = sqlx::query(
            "INSERT INTO user_daily_activity (user_id, date) VALUES ($1, $2) ON CONFLICT DO NOTHING"
        )
        .bind(user_id)
        .bind(&today)
        .execute(pool)
        .await?;

        // 2. If inserted (rows_affected > 0), increment aggregate daily_stats
        if res.rows_affected() > 0 {
            sqlx::query(
                "INSERT INTO daily_stats (date, active_users) VALUES ($1, 1) 
                 ON CONFLICT(date) DO UPDATE SET active_users = daily_stats.active_users + 1, updated_at = CURRENT_TIMESTAMP"
            )
            .bind(today)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Track revenue (in cents)
    pub async fn track_revenue(pool: &PgPool, amount_cents: i64) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, total_revenue) VALUES ($1, $2) 
             ON CONFLICT(date) DO UPDATE SET total_revenue = daily_stats.total_revenue + $3, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(&today)
        .bind(amount_cents)
        .bind(amount_cents)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Track order count
    pub async fn track_order(pool: &PgPool) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, total_orders) VALUES ($1, 1) 
             ON CONFLICT(date) DO UPDATE SET total_orders = daily_stats.total_orders + 1, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(today)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Инкремент `daily_stats.traffic_used`.
    ///
    /// ВНИМАНИЕ: вызовов у этого метода нет ни одного, и поэтому колонка на
    /// проде пустая. Графики трафика читают `app_traffic_daily`
    /// (`get_traffic_history`) — не возвращайте `daily_stats` в источники,
    /// не включив сначала эту запись. Остальные счётчики `daily_stats`
    /// (new_users/active_users/orders) живые, поэтому таблица остаётся.
    pub async fn track_traffic(pool: &PgPool, bytes: i64) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        sqlx::query(
            "INSERT INTO daily_stats (date, traffic_used) VALUES ($1, $2) 
             ON CONFLICT(date) DO UPDATE SET traffic_used = daily_stats.traffic_used + $3, updated_at = CURRENT_TIMESTAMP"
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
    pub async fn get_top_users(&self) -> Result<Vec<TopUser>> {
        let users = sqlx::query_as::<_, TopUser>(
            "SELECT 
                u.username, 
                SUM(s.used_traffic) as total_traffic 
             FROM users u
             JOIN subscriptions s ON u.id = s.user_id
             WHERE s.used_traffic > 0
             GROUP BY u.id
             ORDER BY total_traffic DESC
             LIMIT 10",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    /// Трафик по узлам за 30 дней — из снапшотов `node_traffic_snapshots`.
    ///
    /// Раньше здесь суммировался накопительный `subscriptions.used_traffic` по
    /// `subscriptions.node_id`. Этот столбец бывает NULL и указывает максимум
    /// на один узел из нескольких, обслуживающих план, — для мультинодовых
    /// планов разбивка была структурно неверной, а числа росли вечно, потому
    /// что счётчик накопительный.
    ///
    /// Суммируются только ПОЛОЖИТЕЛЬНЫЕ шаги между соседними замерами:
    /// переустановка узла обнуляет счётчики, и «последний минус первый» после
    /// рестарта показал бы минус.
    pub async fn get_node_traffic_stats(&self) -> Result<Vec<NodeTraffic>> {
        let nodes = sqlx::query_as::<_, NodeTraffic>(
            "SELECT n.name,
                    COALESCE(t.total_traffic, 0)::bigint AS total_traffic
             FROM nodes n
             LEFT JOIN (
                 SELECT node_id, SUM(step)::bigint AS total_traffic
                 FROM (
                     SELECT node_id,
                            GREATEST(total_ingress - LAG(total_ingress)
                                OVER (PARTITION BY node_id ORDER BY ts), 0)
                            + GREATEST(total_egress - LAG(total_egress)
                                OVER (PARTITION BY node_id ORDER BY ts), 0) AS step
                     FROM node_traffic_snapshots
                     WHERE ts > NOW() - INTERVAL '30 days'
                 ) steps
                 WHERE step IS NOT NULL
                 GROUP BY node_id
             ) t ON t.node_id = n.id
             WHERE n.status = 'active'
             ORDER BY total_traffic DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(nodes)
    }

    /// Подневный трафик за 30 дней — из `app_traffic_daily`.
    ///
    /// Источником был `daily_stats.traffic_used`, но инкремента этой колонки в
    /// коде нет ни одного (`track_traffic` ниже не вызывается ниоткуда), и на
    /// проде таблица пустая — график Traffic History был мёртв с рождения.
    /// `app_traffic_daily` наполняется каждым heartbeat узла и уже питает
    /// график в приложении.
    ///
    /// Порядок DESC сохранён: вызывающие в dashboard/analytics разворачивают
    /// массив у себя.
    pub async fn get_traffic_history(&self) -> Result<Vec<DailyTraffic>> {
        let history = sqlx::query_as::<_, DailyTraffic>(
            "SELECT TO_CHAR(day, 'YYYY-MM-DD') AS date,
                    SUM(up_bytes + down_bytes)::bigint AS traffic_used
             FROM app_traffic_daily
             WHERE day >= CURRENT_DATE - 29
             GROUP BY day
             ORDER BY day DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(history)
    }

    /// Подневный трафик ОДНОГО пользователя — для карточки user_details.
    /// Тот же источник, что и график в приложении, чтобы админ и пользователь
    /// видели одни и те же цифры.
    pub async fn get_user_traffic_history(
        &self,
        user_id: i64,
        days: i32,
    ) -> Result<Vec<DailyTraffic>> {
        let days = days.clamp(1, 365);
        let history = sqlx::query_as::<_, DailyTraffic>(
            "SELECT TO_CHAR(day, 'YYYY-MM-DD') AS date,
                    (up_bytes + down_bytes)::bigint AS traffic_used
             FROM app_traffic_daily
             WHERE user_id = $1 AND day >= CURRENT_DATE - ($2::int - 1)
             ORDER BY day DESC",
        )
        .bind(user_id)
        .bind(days)
        .fetch_all(&self.pool)
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
