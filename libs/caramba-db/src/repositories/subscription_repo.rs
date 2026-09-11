use crate::models::store::{Subscription, SubscriptionWithDetails};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;
use std::net::IpAddr;

/// Статус вытесненной подписки: её заменили другой, живой, а не потеряли по
/// сроку. Отличать от `'expired'` обязательно — истечение возвращает человека
/// на бесплатный план и лечится продлением, а вытеснение означает, что доступ
/// уже обслуживает другая строка.
pub const STATUS_SUPERSEDED: &str = "superseded";

/// ЕДИНЫЙ порядок выбора активной подписки пользователя.
///
/// Требует, чтобы в запросе `subscriptions` был под алиасом `s`, а `plans` —
/// под алиасом `p` (LEFT JOIN, план может быть удалён).
///
/// Почему именно так. Раньше выбор писался четырьмя разными способами, и самый
/// частый — `ORDER BY expires_at DESC` — всегда выигрывала бесплатная подписка:
/// она создаётся с `expires_at = '9999-12-31'`, то есть дальше любой купленной.
/// Пользователь платил, а бот, ссылки и учёт трафика продолжали обслуживать
/// бесплатную строку. Поэтому первый ключ — платность, а второй —
/// `expires_at ASC`: среди равных важнее та, что сгорает раньше (её потеря
/// заметна человеку, «вечную» он не заметит). `id` — только для устойчивости
/// порядка при полном равенстве.
pub const ACTIVE_SUBSCRIPTION_ORDER_SQL: &str =
    "ORDER BY COALESCE(p.is_free, FALSE) ASC, s.expires_at ASC, s.id ASC";

/// Готовый запрос «активная подписка пользователя» ($1 — users.id).
/// Один текст на всю панель: расхождение этого запроса и есть тот баг, ради
/// которого [`ACTIVE_SUBSCRIPTION_ORDER_SQL`] вынесен в константу.
pub const ACTIVE_SUBSCRIPTION_BY_USER_SQL: &str = "SELECT s.* FROM subscriptions s \
     LEFT JOIN plans p ON p.id = s.plan_id \
     WHERE s.user_id = $1 AND s.status = 'active' \
     ORDER BY COALESCE(p.is_free, FALSE) ASC, s.expires_at ASC, s.id ASC \
     LIMIT 1";

/// Все активные подписки пользователя под блокировкой строк ($1 — users.id),
/// в том же порядке приоритета.
///
/// Отдельный текст от [`ACTIVE_SUBSCRIPTION_BY_USER_SQL`] по требованию
/// PostgreSQL: `FOR UPDATE` нельзя применять к нулевой стороне внешнего
/// соединения, поэтому платность плана берётся коррелированным подзапросом, а
/// не LEFT JOIN. Блокировка обязательна: две параллельные выдачи иначе обе
/// увидят «активной нет» и вставят по строке — ровно тот дубль, который мы и
/// убираем.
pub const ACTIVE_SUBSCRIPTIONS_FOR_UPDATE_SQL: &str = "SELECT s.* FROM subscriptions s \
     WHERE s.user_id = $1 AND s.status = 'active' \
     ORDER BY COALESCE((SELECT p.is_free FROM plans p WHERE p.id = s.plan_id), FALSE) ASC, \
              s.expires_at ASC, s.id ASC \
     FOR UPDATE";

/// Одна строка-кандидат для выбора главной активной подписки.
/// Существует ради тестируемости: правило приоритета обязано быть одним и тем
/// же в SQL и в любом коде, который сортирует подписки в памяти.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveSubscriptionCandidate {
    pub id: i64,
    pub is_free: bool,
    pub expires_at: DateTime<Utc>,
}

/// Главная активная подписка по тому же правилу, что и
/// [`ACTIVE_SUBSCRIPTION_ORDER_SQL`].
pub fn pick_primary_subscription(
    candidates: &[ActiveSubscriptionCandidate],
) -> Option<ActiveSubscriptionCandidate> {
    candidates
        .iter()
        .min_by_key(|c| (c.is_free, c.expires_at, c.id))
        .copied()
}

#[derive(Debug, Clone)]
pub struct SubscriptionRepository {
    pool: PgPool,
}

impl SubscriptionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by ID")
    }

    /// Tx-aware вариант: читает подписку по id внутри переданной транзакции.
    pub async fn get_by_id_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: i64,
    ) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .context("Failed to fetch subscription by ID (tx)")
    }

    pub async fn get_by_uuid(&self, uuid: &str) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE subscription_uuid = $1",
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch subscription by UUID")
    }

    /// Главная активная подписка пользователя — единственное правило выбора на
    /// всю систему, см. [`ACTIVE_SUBSCRIPTION_ORDER_SQL`].
    pub async fn get_active_by_user(&self, user_id: i64) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>(ACTIVE_SUBSCRIPTION_BY_USER_SQL)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch active subscription for user")
    }

    /// Tx-aware вариант: читает активную подписку пользователя внутри транзакции.
    /// Используется там, где чтение должно происходить в рамках той же транзакции,
    /// что и последующее изменение (например, в extend_subscription).
    pub async fn get_active_by_user_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: i64,
    ) -> Result<Option<Subscription>> {
        sqlx::query_as::<_, Subscription>(ACTIVE_SUBSCRIPTION_BY_USER_SQL)
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await
            .context("Failed to fetch active subscription for user (tx)")
    }

    pub async fn get_active_by_plan(&self, plan_id: i64) -> Result<Vec<Subscription>> {
        sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE plan_id = $1 AND status = 'active'",
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active subscriptions for plan")
    }

    pub async fn get_all_by_user(&self, user_id: i64) -> Result<Vec<SubscriptionWithDetails>> {
        sqlx::query_as::<_, SubscriptionWithDetails>(
            "SELECT s.*, p.name as plan_name, p.description as plan_description, p.traffic_limit_gb 
             FROM subscriptions s 
             JOIN plans p ON s.plan_id = p.id
             LEFT JOIN nodes n ON s.node_id = n.id
             WHERE s.user_id = $1
             ORDER BY s.created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions")
    }

    /// План главной активной подписки. Порядок тот же, что и в
    /// [`get_active_by_user`]: раньше здесь не было ORDER BY вообще, и при двух
    /// активных строках план выбирался как придётся — то есть ноды могли выдать
    /// человеку доступ по бесплатному плану сразу после оплаты.
    pub async fn get_active_plan_id_by_user(&self, user_id: i64) -> Result<Option<i64>> {
        sqlx::query_scalar(
            "SELECT s.plan_id FROM subscriptions s \
             LEFT JOIN plans p ON p.id = s.plan_id \
             WHERE s.user_id = $1 AND s.status = 'active' \
             ORDER BY COALESCE(p.is_free, FALSE) ASC, s.expires_at ASC, s.id ASC \
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch active plan ID")
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        user_id: i64,
        plan_id: i64,
        vless_uuid: &str,
        sub_uuid: &str,
        expires_at: DateTime<Utc>,
        status: &str,
        note: Option<&str>,
    ) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, note, created_at, is_trial)
            VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP, FALSE)
            RETURNING id
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(expires_at)
        .bind(status)
        .bind(note)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create subscription")?;

        Ok(id)
    }

    /// Tx-aware вариант: создаёт подписку внутри переданной транзакции.
    /// Атомарность со списанием баланса обеспечивается единой транзакцией вызывающей стороны.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: i64,
        plan_id: i64,
        vless_uuid: &str,
        sub_uuid: &str,
        expires_at: DateTime<Utc>,
        status: &str,
        note: Option<&str>,
    ) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, note, created_at, is_trial)
            VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP, FALSE)
            RETURNING id
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(sub_uuid)
        .bind(expires_at)
        .bind(status)
        .bind(note)
        .fetch_one(&mut **tx)
        .await
        .context("Failed to create subscription (tx)")?;

        Ok(id)
    }

    pub async fn update_expiry(&self, id: i64, new_expiry: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = $1, status = 'active' WHERE id = $2")
            .bind(new_expiry)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Tx-aware вариант: обновляет дату истечения подписки внутри транзакции.
    pub async fn update_expiry_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: i64,
        new_expiry: DateTime<Utc>,
    ) -> Result<()> {
        // Продление = новый расчётный период: сбрасываем used_traffic, иначе
        // оплативший продление пользователь, уже упершийся в квоту, будет
        // мгновенно снова заблокирован квота-энфорсментом.
        sqlx::query(
            "UPDATE subscriptions SET expires_at = $1, status = 'active', used_traffic = 0 WHERE id = $2",
        )
        .bind(new_expiry)
        .bind(id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn extend_expiry_days(&self, id: i64, days: i64) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = expires_at + ($1 * interval '1 day') WHERE id = $2")
            .bind(days)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn expire_family_subs(&self, parent_id: i64) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE user_id = $1 AND note = 'Family' AND status = 'active'")
            .bind(parent_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_by_plan_id(&self, plan_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE plan_id = $1")
            .bind(plan_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status_and_expiry(
        &self,
        id: i64,
        status: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions SET status = $1, expires_at = $2, used_traffic = 0 WHERE id = $3",
        )
        .bind(status)
        .bind(expires_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn toggle_auto_renew(&self, id: i64) -> Result<bool> {
        // auto_renew — тип BOOLEAN в PostgreSQL, читаем напрямую как Option<bool>
        let current: bool = sqlx::query_scalar::<_, Option<bool>>(
            "SELECT auto_renew FROM subscriptions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?
        .unwrap_or(false);

        let new_value = !current;

        sqlx::query("UPDATE subscriptions SET auto_renew = $1 WHERE id = $2")
            .bind(new_value)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(new_value)
    }

    pub async fn toggle_auto_renewal(&self, id: i64) -> Result<bool> {
        self.toggle_auto_renew(id).await
    }

    pub async fn update_alerts_sent(&self, id: i64, alerts_json: &str) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET alerts_sent = $1 WHERE id = $2")
            .bind(alerts_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_expiring_auto_renewals(&self) -> Result<Vec<(i64, i64, i64, String, i64)>> {
        // auto_renew — BOOLEAN: используем COALESCE(s.auto_renew, FALSE) = TRUE
        let subs = sqlx::query_as::<_, (i64, i64, i64, String, i64)>(
            "SELECT s.id, s.user_id, s.plan_id, p.name, u.balance
             FROM subscriptions s
             JOIN users u ON s.user_id = u.id
             JOIN plans p ON s.plan_id = p.id
             WHERE COALESCE(s.auto_renew, FALSE) = TRUE
             AND s.status = 'active'
             AND s.expires_at BETWEEN CURRENT_TIMESTAMP AND CURRENT_TIMESTAMP + interval '1 day'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(subs)
    }

    pub async fn get_active_with_traffic_limit(&self) -> Result<Vec<(i64, i64, i64, i64, String)>> {
        let subs = sqlx::query_as::<_, (i64, i64, i64, i64, String)>(
            "SELECT s.id, s.user_id, s.used_traffic, p.traffic_limit_gb, COALESCE(s.alerts_sent, '[]') 
             FROM subscriptions s
             JOIN plans p ON s.plan_id = p.id
             WHERE s.status = 'active' AND COALESCE(p.traffic_limit_gb, 0) > 0",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(subs)
    }

    pub async fn get_device_limit(&self, sub_id: i64) -> Result<Option<i32>> {
        sqlx::query_scalar(
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = $1"
        )
        .bind(sub_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get device limit")
    }

    pub async fn update_family_sub(
        &self,
        id: i64,
        expires_at: DateTime<Utc>,
        plan_id: i64,
        node_id: Option<i64>,
    ) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = $1, plan_id = $2, node_id = $3, status = 'active', note = 'Family' WHERE id = $4")
            .bind(expires_at)
            .bind(plan_id)
            .bind(node_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Активные подписки указанных планов в том виде, в каком
    /// `orchestration_service` впрыскивает пользователей в конфиги нод:
    /// `(subscription_id, vless_uuid, client_identity, username)`.
    ///
    /// Третий элемент — идентичность клиента, а НЕ обязательно Telegram id.
    /// `users.tg_id` в схеме nullable (проверено на проде:
    /// `information_schema.columns` → `is_nullable = YES`), и у аккаунтов,
    /// созданных email-регистрацией (`POST /api/v2/app/register`), он NULL.
    /// Раньше колонка декодировалась как non-optional `i64`: первая же
    /// подписка email-пользователя превращала весь запрос в `Err`, вызывающая
    /// сторона глушила ошибку в пустой список и публиковала на ноды конфиг
    /// без единого клиента — то есть отключала всех платных подписчиков
    /// разом и без самовосстановления.
    ///
    /// Поэтому tg_id читается как `Option` и, когда его нет, подменяется
    /// стабильным суррогатом — см. [`config_client_identity`]. Молча выбросить
    /// такого пользователя из выборки нельзя: он обязан попасть в конфиг ноды,
    /// иначе оплаченный (пусть и бесплатный) доступ просто не заработает —
    /// это тот же баг, что мы чиним, только этажом ниже.
    pub async fn get_active_subs_by_plans(
        &self,
        plan_ids: &[i64],
    ) -> Result<Vec<(i64, Option<String>, i64, Option<String>)>> {
        if plan_ids.is_empty() {
            return Ok(Vec::new());
        }

        // u.id выбирается специально: он нужен как источник суррогатной
        // идентичности, когда tg_id отсутствует.
        let rows = sqlx::query_as::<_, (i64, Option<String>, Option<i64>, i64, Option<String>)>(
            r#"
            SELECT s.id, s.vless_uuid, u.tg_id, u.id, u.username
            FROM subscriptions s
            JOIN users u ON s.user_id = u.id
            WHERE LOWER(s.status) = 'active' AND s.plan_id = ANY($1)
            "#,
        )
        .bind(plan_ids)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active subs by plans")?;

        Ok(rows
            .into_iter()
            .map(|(sub_id, vless_uuid, tg_id, user_id, username)| {
                (
                    sub_id,
                    vless_uuid,
                    config_client_identity(tg_id, user_id),
                    username,
                )
            })
            .collect())
    }

    pub async fn update_ips(&self, sub_id: i64, ips: Vec<String>) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let node_ip_match_sql = r#"
            n.ip = $2
            OR split_part(n.ip, ':', 1) = $2
            OR regexp_replace(n.ip, '^::ffff:', '') = regexp_replace($2, '^::ffff:', '')
            OR regexp_replace(split_part(n.ip, ':', 1), '^::ffff:', '') = regexp_replace($2, '^::ffff:', '')
        "#;

        // Remove invalid/self-referential rows that should never be counted as client devices.
        sqlx::query(&format!(
            "DELETE FROM subscription_ip_tracking
             WHERE subscription_id = $1
               AND (
                    client_ip = ''
                    OR client_ip = '0.0.0.0'
                    OR EXISTS (
                        SELECT 1
                        FROM nodes n
                        WHERE {}
                    )
               )",
            node_ip_match_sql.replace("$2", "subscription_ip_tracking.client_ip")
        ))
        .bind(sub_id)
        .execute(&mut *tx)
        .await?;

        let mut dedup = HashSet::new();
        for ip in ips.into_iter().filter_map(|ip| normalize_client_ip(&ip)) {
            if !dedup.insert(ip.clone()) {
                continue;
            }

            sqlx::query(&format!(
                "INSERT INTO subscription_ip_tracking (subscription_id, client_ip, last_seen_at)
                 SELECT $1, $2, CURRENT_TIMESTAMP
                 WHERE NOT EXISTS (SELECT 1 FROM nodes n WHERE {})
                 ON CONFLICT (subscription_id, client_ip)
                 DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at",
                node_ip_match_sql
            ))
            .bind(sub_id)
            .bind(ip)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_active_ips(
        &self,
        sub_id: i64,
    ) -> Result<Vec<crate::models::store::SubscriptionIpTracking>> {
        // Явно включаем user_agent — поле присутствует в SubscriptionIpTracking
        sqlx::query_as::<_, crate::models::store::SubscriptionIpTracking>(
            "SELECT id, subscription_id, client_ip, user_agent, last_seen_at FROM subscription_ip_tracking WHERE subscription_id = $1 ORDER BY last_seen_at DESC",
        )
        .bind(sub_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active IPs")
    }
}

/// Идентичность клиента для конфигов нод: Telegram id, если он есть, иначе
/// стабильный суррогат `-users.id`.
///
/// Почему именно отрицательное пространство. Идентичность здесь используется
/// как ключ во всех генераторах пользователей sing-box: тег `user_{id}`
/// (`services::user_tag`), пароль Hysteria2 `{id}:{uuid}`, адрес клиента
/// AmneziaWG и ключ дедупликации. Значение обязано быть (1) стабильным между
/// перегенерациями конфига и (2) неспособным совпасть с чужим Telegram id,
/// иначе трафик и разрывы соединений уедут не тому пользователю. `users.id` —
/// положительный bigserial PK, он стабилен; Telegram id пользователей всегда
/// положительные (на проде минимум ~9.5e7, ноль строк с `tg_id <= 0`), так что
/// отрицательная область гарантированно свободна.
///
/// Плата за это осознанная: `user_tag::parse_user_tag` вернёт отрицательное
/// число, `SELECT ... WHERE tg_id = ANY(...)` его не найдёт, и учёт трафика
/// такого клиента уйдёт в счётчик unresolved с предупреждением в логе. Это
/// строго лучше двух альтернатив — выкинуть клиента из конфига (доступ не
/// работает вообще) или переиспользовать положительное пространство
/// (трафик и kill-switch попадут в чужой аккаунт).
pub fn config_client_identity(tg_id: Option<i64>, user_id: i64) -> i64 {
    match tg_id {
        Some(id) => id,
        None => -user_id,
    }
}

/// Читает идентичность клиента из строки `users` — ЕДИНСТВЕННЫЙ допустимый
/// способ получить её на клиентской половине (генераторы ссылок подписки,
/// internal-эндпоинт ключей).
///
/// Существует ради симметрии с [`get_active_subs_by_plans`]: серверная
/// половина берёт `u.tg_id` уже как `Option` и прогоняет через
/// [`config_client_identity`], а клиентская до сих пор читала колонку сама и
/// каждый раз заново решала, что делать с NULL. Два независимых решения —
/// это и есть расхождение, из-за которого нода ждёт `-42:uuid`, а
/// пользователю выдаётся `0:uuid`, и аутентификация молча не проходит.
///
/// Отдельно про декодирование: `SELECT tg_id` в non-optional `i64` даёт не
/// «ноль», а `Err(UnexpectedNull)` — любой `.unwrap_or(0)` после такого
/// запроса мёртв, а вызывающая сторона получает 500 вместо ссылки.
///
/// Отсутствие строки `users` — не штатная ситуация, а нарушение
/// `subscriptions_user_id_fkey` (FOREIGN KEY ... ON DELETE CASCADE), поэтому
/// здесь честная ошибка, а не выдуманная идентичность: выдуманная означала бы
/// клиентскую ссылку для пользователя, которого нода не знает.
pub async fn config_client_identity_for_user(pool: &PgPool, user_id: i64) -> Result<i64> {
    let tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context("Failed to read client identity from users")?
        .ok_or_else(|| {
            anyhow::anyhow!("User {} referenced by subscription does not exist", user_id)
        })?;

    Ok(config_client_identity(tg_id, user_id))
}

/// Строка `{идентичность}:{uuid_без_дефисов}` — ЕДИНСТВЕННЫЙ построитель
/// пароля Hysteria2 и учётки naive.
///
/// Обе половины обязаны звать именно её: нода кладёт результат в
/// `Hysteria2User::password` (`orchestration_service`), клиент получает тот же
/// результат в ссылке подписки (`subscription_service`, `api/internal`). Пока
/// формат собирался двумя разными `format!` по разным входам, расхождение
/// нельзя было заметить ничем, кроме жалобы пользователя: неудачная
/// аутентификация Hysteria2 не пишет в логи панели ничего.
pub fn proxy_auth_password(client_identity: i64, uuid: &str) -> String {
    format!("{}:{}", client_identity, uuid.replace('-', ""))
}

fn normalize_client_ip(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "0.0.0.0" || trimmed == "::" {
        return None;
    }

    let ip = parse_ip_maybe(trimmed)?;
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    Some(ip.to_string())
}

fn parse_ip_maybe(value: &str) -> Option<IpAddr> {
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Some(canonicalize_ip(ip));
    }

    if let Ok(sock) = value.parse::<std::net::SocketAddr>() {
        return Some(canonicalize_ip(sock.ip()));
    }

    if let Some((host, _port)) = value.rsplit_once(':')
        && let Ok(ip) = host.parse::<IpAddr>()
    {
        return Some(canonicalize_ip(ip));
    }

    None
}

fn canonicalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_SUBSCRIPTION_BY_USER_SQL, ACTIVE_SUBSCRIPTION_ORDER_SQL,
        ActiveSubscriptionCandidate, config_client_identity, pick_primary_subscription,
        proxy_auth_password,
    };
    use chrono::{DateTime, TimeZone, Utc};

    fn at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
    }

    fn free(id: i64) -> ActiveSubscriptionCandidate {
        // Бесплатная подписка создаётся ровно с этой датой — см.
        // store_service::ensure_free_plan_subscription_tx.
        ActiveSubscriptionCandidate {
            id,
            is_free: true,
            expires_at: at(9999, 12, 31),
        }
    }

    fn paid(id: i64, expires: DateTime<Utc>) -> ActiveSubscriptionCandidate {
        ActiveSubscriptionCandidate {
            id,
            is_free: false,
            expires_at: expires,
        }
    }

    /// Тот самый баг: `ORDER BY expires_at DESC` отдавал бесплатную подписку с
    /// 9999 годом вместо только что оплаченной. Правило приоритета обязано
    /// ставить платную первой независимо от дат.
    #[test]
    fn a_paid_subscription_beats_the_everlasting_free_one() {
        let subs = [free(1), paid(2, at(2026, 10, 1))];
        assert_eq!(pick_primary_subscription(&subs).unwrap().id, 2);
        // Порядок аргументов ничего не меняет.
        let reversed = [paid(2, at(2026, 10, 1)), free(1)];
        assert_eq!(pick_primary_subscription(&reversed).unwrap().id, 2);
    }

    /// Среди равных по платности важнее та, что сгорает раньше: её исчезновение
    /// человек заметит, а «вечную» — нет.
    #[test]
    fn among_equals_the_soonest_expiry_wins() {
        let subs = [paid(7, at(2027, 1, 1)), paid(3, at(2026, 10, 1))];
        assert_eq!(pick_primary_subscription(&subs).unwrap().id, 3);

        // Полное равенство — меньший id, чтобы выбор был устойчивым между
        // вызовами (иначе один и тот же аккаунт обслуживался бы по-разному).
        let tie = [
            paid(9, at(2026, 10, 1)),
            paid(4, at(2026, 10, 1)),
            paid(6, at(2026, 10, 1)),
        ];
        assert_eq!(pick_primary_subscription(&tie).unwrap().id, 4);
    }

    #[test]
    fn only_free_subscriptions_still_yield_one() {
        let subs = [free(5), free(2)];
        assert_eq!(pick_primary_subscription(&subs).unwrap().id, 2);
    }

    #[test]
    fn no_active_subscriptions_yield_nothing() {
        assert_eq!(pick_primary_subscription(&[]), None);
    }

    /// Правило обязано быть ОДНО: SQL-запрос и функция в памяти не имеют права
    /// разойтись — именно на таком расхождении бэкенд и мини-апп показывали
    /// разные подписки одного аккаунта.
    #[test]
    fn the_sql_and_the_in_memory_rule_are_the_same_rule() {
        let squash = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };
        assert!(
            squash(ACTIVE_SUBSCRIPTION_BY_USER_SQL)
                .contains(&squash(ACTIVE_SUBSCRIPTION_ORDER_SQL)),
            "запрос активной подписки перестал использовать общий порядок: {ACTIVE_SUBSCRIPTION_BY_USER_SQL}"
        );
        assert!(
            ACTIVE_SUBSCRIPTION_ORDER_SQL.contains("COALESCE(p.is_free, FALSE) ASC"),
            "из порядка выбора исчез приоритет платного плана"
        );
        assert!(
            ACTIVE_SUBSCRIPTION_ORDER_SQL.contains("s.expires_at ASC"),
            "порядок выбора вернулся к expires_at DESC — это и есть баг с 9999 годом"
        );
    }

    /// Регрессия на BLOCKER: у email-аккаунтов `users.tg_id` = NULL. Раньше
    /// строка с NULL роняла весь `get_active_subs_by_plans` в Err, и конфиг
    /// ноды публиковался пустым. Теперь такой пользователь обязан получить
    /// идентичность, а не исчезнуть.
    #[test]
    fn user_without_telegram_id_gets_stable_surrogate_identity() {
        assert_eq!(config_client_identity(None, 42), -42);
        // Стабильность: тот же пользователь — тот же идентификатор при каждой
        // перегенерации конфига (иначе клиент терял бы доступ после регена).
        assert_eq!(
            config_client_identity(None, 42),
            config_client_identity(None, 42)
        );
        // Разные пользователи — разные идентификаторы (общий тег означал бы
        // взаимный kill-switch и смешанный учёт трафика).
        assert_ne!(
            config_client_identity(None, 42),
            config_client_identity(None, 43)
        );
    }

    /// Telegram-пользователи (все 13 платных на проде) должны получить ровно
    /// свой tg_id — иначе сломается совместимость с уже выданными ссылками
    /// подписок и с разбором тегов в учёте трафика.
    #[test]
    fn telegram_user_identity_is_unchanged() {
        assert_eq!(config_client_identity(Some(95_679_857), 1), 95_679_857);
        assert_eq!(
            config_client_identity(Some(8_986_550_680), 45),
            8_986_550_680
        );
    }

    /// Суррогат не может столкнуться с реальным Telegram id: те строго
    /// положительные, суррогаты строго отрицательные.
    #[test]
    fn surrogate_namespace_cannot_collide_with_telegram_ids() {
        for user_id in 1..=1000i64 {
            let surrogate = config_client_identity(None, user_id);
            assert!(
                surrogate < 0,
                "surrogate for user {user_id} must live in the negative namespace, got {surrogate}"
            );
            // И не совпадает ни с одним tg_id того же аккаунта.
            assert_ne!(surrogate, config_client_identity(Some(user_id), user_id));
        }
    }

    /// Формат пароля зафиксирован: ноды в проде уже раздают
    /// `{идентичность}:{uuid без дефисов}`, и любое изменение здесь отключит
    /// всех подписчиков Hysteria2 разом.
    #[test]
    fn proxy_password_pins_the_wire_format() {
        assert_eq!(
            proxy_auth_password(95_679_857, "550e8400-e29b-41d4-a716-446655440000"),
            "95679857:550e8400e29b41d4a716446655440000"
        );
        // Суррогат отрицательный — минус обязан попасть в пароль, иначе
        // -42 и 42 схлопнутся в одну строку.
        assert_eq!(proxy_auth_password(-42, "aa-bb"), "-42:aabb");
    }
}
