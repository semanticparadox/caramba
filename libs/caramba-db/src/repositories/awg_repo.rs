use crate::models::awg::{NodeAwg, NodeUserActivity, SubscriptionAwgKey};
use anyhow::Result;
use sqlx::PgPool;

/// Доступ к трём таблицам AmneziaWG: сервер на ноде, ключи подписок и
/// онлайн-активность, которую нода присылает в heartbeat.
#[derive(Clone)]
pub struct AwgRepository {
    pool: PgPool,
}

impl AwgRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_node_awg(&self, node_id: i64) -> Result<Option<NodeAwg>> {
        let row = sqlx::query_as::<_, NodeAwg>("SELECT * FROM node_awg WHERE node_id = $1")
            .bind(node_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    /// Создаёт строку ноды один раз. Повторный вызов ничего не перезаписывает:
    /// серверный ключ и параметры обфускации обязаны быть стабильными, иначе
    /// у всех уже выданных клиентов разом ломается хендшейк.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_node_awg_if_absent(&self, awg: &NodeAwg) -> Result<NodeAwg> {
        sqlx::query(
            r#"
            INSERT INTO node_awg (
                node_id, listen_port, private_key, public_key, address_cidr,
                jc, jmin, jmax, s1, s2, h1, h2, h3, h4, enabled
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
            ON CONFLICT (node_id) DO NOTHING
            "#,
        )
        .bind(awg.node_id)
        .bind(awg.listen_port)
        .bind(&awg.private_key)
        .bind(&awg.public_key)
        .bind(&awg.address_cidr)
        .bind(awg.jc)
        .bind(awg.jmin)
        .bind(awg.jmax)
        .bind(awg.s1)
        .bind(awg.s2)
        .bind(awg.h1)
        .bind(awg.h2)
        .bind(awg.h3)
        .bind(awg.h4)
        .bind(awg.enabled)
        .execute(&self.pool)
        .await?;

        self.get_node_awg(awg.node_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("node_awg row for node {} vanished", awg.node_id))
    }

    pub async fn set_node_awg_enabled(&self, node_id: i64, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE node_awg SET enabled = $1, updated_at = NOW() WHERE node_id = $2")
            .bind(enabled)
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Выдаёт ключи подписки, создавая строку при первом обращении.
    /// Значения детерминированы (см. AwgService), поэтому гонка двух
    /// параллельных запросов даёт одну и ту же строку.
    pub async fn upsert_subscription_key(
        &self,
        subscription_id: i64,
        public_key: &str,
        private_key: &str,
        allowed_ip: &str,
    ) -> Result<SubscriptionAwgKey> {
        sqlx::query(
            r#"
            INSERT INTO subscription_awg_keys (subscription_id, public_key, private_key, allowed_ip)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (subscription_id) DO UPDATE SET
                public_key = EXCLUDED.public_key,
                private_key = EXCLUDED.private_key,
                allowed_ip = EXCLUDED.allowed_ip
            "#,
        )
        .bind(subscription_id)
        .bind(public_key)
        .bind(private_key)
        .bind(allowed_ip)
        .execute(&self.pool)
        .await?;

        let row = sqlx::query_as::<_, SubscriptionAwgKey>(
            "SELECT * FROM subscription_awg_keys WHERE subscription_id = $1",
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Пакетная запись heartbeat.active_users. Пустой список это не «все
    /// офлайн», а «нода ничего не прислала», поэтому вызывающий такой список
    /// сюда не отдаёт.
    pub async fn upsert_user_activity(
        &self,
        node_id: i64,
        rows: &[(String, i64, i64, bool)],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let tags: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
        let rx: Vec<i64> = rows.iter().map(|r| r.1).collect();
        let tx: Vec<i64> = rows.iter().map(|r| r.2).collect();
        let online: Vec<bool> = rows.iter().map(|r| r.3).collect();

        sqlx::query(
            r#"
            INSERT INTO node_user_activity (node_id, user_tag, last_seen_at, online, rx_delta, tx_delta)
            SELECT $1, t.tag, NOW(), t.online, t.rx, t.tx
            FROM (
                SELECT unnest($2::text[])   AS tag,
                       unnest($3::bigint[]) AS rx,
                       unnest($4::bigint[]) AS tx,
                       unnest($5::bool[])   AS online
            ) t
            ON CONFLICT (node_id, user_tag) DO UPDATE SET
                last_seen_at = EXCLUDED.last_seen_at,
                online = EXCLUDED.online,
                rx_delta = EXCLUDED.rx_delta,
                tx_delta = EXCLUDED.tx_delta
            "#,
        )
        .bind(node_id)
        .bind(&tags)
        .bind(&rx)
        .bind(&tx)
        .bind(&online)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_online_on_node(&self, node_id: i64) -> Result<Vec<NodeUserActivity>> {
        let rows = sqlx::query_as::<_, NodeUserActivity>(
            "SELECT * FROM node_user_activity WHERE node_id = $1 AND online = TRUE ORDER BY user_tag",
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
