//! Репозиторий подневной истории трафика для standalone-приложения.
//!
//! Питает график трафика во Flutter-клиенте (`GET /api/v2/app/traffic`).
//! Источник записи — единственная точка приёма счётчиков от узла
//! (`api/v2/node.rs`), которая отдаёт ОДИН счётчик байт на пользователя без
//! разделения upload/download. Поэтому `record_usage` пишет объём в `down_bytes`
//! (доминирующее направление), а `up_bytes` остаётся 0 до тех пор, пока агент
//! не начнёт рапортовать раздельные счётчики.

use anyhow::Result;
use sqlx::PgPool;

/// Одна точка графика: день (UTC) + накопленные за день байты по направлениям.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct DailyTrafficPoint {
    /// День в формате `YYYY-MM-DD` (UTC).
    pub day: chrono::NaiveDate,
    pub up_bytes: i64,
    pub down_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct TrafficRepository {
    pool: PgPool,
}

impl TrafficRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Накапливает дельты трафика за сегодняшний день (UTC) для пользователя.
    ///
    /// UPSERT по (user_id, day): новая строка либо инкремент существующей.
    /// Узел отдаёт суммарный счётчик, поэтому `down` — это весь объём, `up` = 0.
    /// Метод идемпотентен только в смысле атомарности инкремента: повторный
    /// вызов с теми же байтами добавит их снова (дедуп — забота вызывающего,
    /// как и для `subscriptions.used_traffic`).
    pub async fn record_usage(&self, user_id: i64, up_bytes: i64, down_bytes: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO app_traffic_daily (user_id, day, up_bytes, down_bytes, updated_at)
            VALUES ($1, (NOW() AT TIME ZONE 'UTC')::date, $2, $3, CURRENT_TIMESTAMP)
            ON CONFLICT (user_id, day) DO UPDATE
                SET up_bytes   = app_traffic_daily.up_bytes   + EXCLUDED.up_bytes,
                    down_bytes = app_traffic_daily.down_bytes + EXCLUDED.down_bytes,
                    updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(user_id)
        .bind(up_bytes)
        .bind(down_bytes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Пакетная запись дельт для набора пользователей за сегодняшний день (UTC).
    ///
    /// Используется горячей точкой приёма трафика (`api/v2/node.rs`), где за один
    /// тик приходят счётчики сразу для многих пользователей. Один запрос через
    /// `unnest()` вместо N отдельных INSERT — в духе bulk-UPDATE подписок там же.
    /// `down_bytes` несёт весь объём (узел не разделяет направления), up = 0.
    pub async fn record_usage_bulk(&self, user_ids: &[i64], down_bytes: &[i64]) -> Result<()> {
        if user_ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO app_traffic_daily (user_id, day, up_bytes, down_bytes, updated_at)
            SELECT c.user_id, (NOW() AT TIME ZONE 'UTC')::date, 0, c.bytes, CURRENT_TIMESTAMP
            FROM (
                SELECT unnest($1::bigint[]) AS user_id,
                       unnest($2::bigint[]) AS bytes
            ) c
            ON CONFLICT (user_id, day) DO UPDATE
                SET down_bytes = app_traffic_daily.down_bytes + EXCLUDED.down_bytes,
                    updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(user_ids)
        .bind(down_bytes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Подневная история трафика пользователя за последние `days` дней (UTC).
    ///
    /// Возвращает только дни, по которым есть записи (без заполнения нулями —
    /// клиент сам строит ось дат). Сортировка по возрастанию дня для удобной
    /// отрисовки линии слева направо.
    pub async fn get_history(&self, user_id: i64, days: i64) -> Result<Vec<DailyTrafficPoint>> {
        let rows = sqlx::query_as::<_, DailyTrafficPoint>(
            r#"
            SELECT day, up_bytes, down_bytes
            FROM app_traffic_daily
            WHERE user_id = $1
              AND day >= (NOW() AT TIME ZONE 'UTC')::date - ($2::int - 1)
            ORDER BY day ASC
            "#,
        )
        .bind(user_id)
        .bind(days as i32)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
