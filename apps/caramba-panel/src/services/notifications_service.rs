use anyhow::{Context, Result};
use caramba_db::models::notifications::{NotificationChannelPref, UserNotification};
use serde_json::Value;
use sqlx::PgPool;
use tracing::{error, warn};

use crate::bot_manager::BotManager;

pub struct NotificationsService {
    pool: PgPool,
    bot_manager: std::sync::Arc<BotManager>,
}

impl NotificationsService {
    pub fn new(pool: PgPool, bot_manager: std::sync::Arc<BotManager>) -> Self {
        Self { pool, bot_manager }
    }

    /// Создаёт уведомление и при необходимости доставляет его через бот.
    ///
    /// Логика доставки через бот:
    ///   1. Проверяем настройки канала bot_dm для данной категории у пользователя.
    ///   2. Если включён — отправляем сообщение через BotManager и выставляем delivered_via_bot = TRUE.
    ///   3. Если бот не запущен — тихо продолжаем (уведомление сохраняется для Mini App).
    pub async fn create(
        &self,
        user_id: i64,
        category: &str,
        severity: &str,
        title: &str,
        body: &str,
        payload: Option<Value>,
    ) -> Result<i64> {
        // Проверяем, включён ли канал bot_dm для этой категории у пользователя.
        // Если строки в таблице нет — считаем, что включено по умолчанию.
        let bot_dm_enabled: bool = sqlx::query_scalar(
            "SELECT COALESCE(
                (SELECT enabled FROM notification_channel_prefs
                 WHERE user_id = $1 AND category = $2 AND channel = 'bot_dm'),
                TRUE
             )",
        )
        .bind(user_id)
        .bind(category)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(true);

        // Достаём tg_id пользователя для DM-доставки
        let tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);

        // Пытаемся отправить в бот, если оба условия выполнены
        let mut delivered_via_bot = false;
        if bot_dm_enabled {
            if let Some(tid) = tg_id {
                let msg = format!("*{}*\n{}", escape_md(title), escape_md(body));
                match self.bot_manager.send_notification(tid, &msg).await {
                    Ok(_) => {
                        delivered_via_bot = true;
                    }
                    Err(e) => {
                        warn!(
                            "Не удалось доставить уведомление пользователю {} через бот: {}",
                            user_id, e
                        );
                    }
                }
            }
        }

        // Сохраняем уведомление в БД
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO user_notifications
                (user_id, category, severity, title, body, payload_json, delivered_via_bot)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(user_id)
        .bind(category)
        .bind(severity)
        .bind(title)
        .bind(body)
        .bind(&payload)
        .bind(delivered_via_bot)
        .fetch_one(&self.pool)
        .await
        .context("Не удалось вставить уведомление")?;

        Ok(id)
    }

    /// Saves notification to inbox WITHOUT touching bot DM. Use when the
    /// caller already sends a richly-formatted Telegram message via
    /// `bot_manager.send_notification` and just wants the row to appear in
    /// the Mini App inbox.
    pub async fn create_inbox_only(
        &self,
        user_id: i64,
        category: &str,
        severity: &str,
        title: &str,
        body: &str,
        payload: Option<Value>,
    ) -> Result<i64> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO user_notifications
                (user_id, category, severity, title, body, payload_json, delivered_via_bot)
             VALUES ($1, $2, $3, $4, $5, $6, FALSE)
             RETURNING id",
        )
        .bind(user_id)
        .bind(category)
        .bind(severity)
        .bind(title)
        .bind(body)
        .bind(&payload)
        .fetch_one(&self.pool)
        .await
        .context("Не удалось вставить уведомление в inbox")?;
        Ok(id)
    }

    /// Возвращает уведомления пользователя.
    ///
    /// `status_filter` может быть "unread", "read", "archived" или None (все).
    pub async fn list(
        &self,
        user_id: i64,
        status_filter: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<UserNotification>> {
        let rows = if let Some(status) = status_filter {
            sqlx::query_as::<_, UserNotification>(
                "SELECT id, user_id, category, severity, title, body, payload_json, status,
                        delivered_via_bot, created_at, read_at
                 FROM user_notifications
                 WHERE user_id = $1 AND status = $2
                 ORDER BY created_at DESC
                 LIMIT $3 OFFSET $4",
            )
            .bind(user_id)
            .bind(status)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .context("Ошибка запроса уведомлений")?
        } else {
            sqlx::query_as::<_, UserNotification>(
                "SELECT id, user_id, category, severity, title, body, payload_json, status,
                        delivered_via_bot, created_at, read_at
                 FROM user_notifications
                 WHERE user_id = $1
                 ORDER BY created_at DESC
                 LIMIT $2 OFFSET $3",
            )
            .bind(user_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .context("Ошибка запроса уведомлений")?
        };
        Ok(rows)
    }

    /// Помечает одно уведомление как прочитанное (только если принадлежит пользователю).
    pub async fn mark_read(&self, user_id: i64, notification_id: i64) -> Result<()> {
        let affected = sqlx::query(
            "UPDATE user_notifications
             SET status = 'read', read_at = NOW()
             WHERE id = $1 AND user_id = $2 AND status = 'unread'",
        )
        .bind(notification_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Ошибка пометки уведомления как прочитанного")?
        .rows_affected();

        if affected == 0 {
            // Уже прочитано или не принадлежит пользователю — не ошибка
        }
        Ok(())
    }

    /// Помечает все непрочитанные уведомления пользователя как прочитанные.
    /// Возвращает количество изменённых записей.
    pub async fn mark_all_read(&self, user_id: i64) -> Result<i64> {
        let affected = sqlx::query(
            "UPDATE user_notifications
             SET status = 'read', read_at = NOW()
             WHERE user_id = $1 AND status = 'unread'",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Ошибка массовой пометки уведомлений")?
        .rows_affected();

        Ok(affected as i64)
    }

    /// Количество непрочитанных уведомлений (дешёвый запрос для бейджа).
    pub async fn unread_count(&self, user_id: i64) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_notifications WHERE user_id = $1 AND status = 'unread'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .context("Ошибка подсчёта непрочитанных")?;

        Ok(count)
    }

    /// Получает настройки каналов уведомлений пользователя.
    pub async fn get_preferences(&self, user_id: i64) -> Result<Vec<NotificationChannelPref>> {
        let prefs = sqlx::query_as::<_, NotificationChannelPref>(
            "SELECT user_id, category, channel, enabled
             FROM notification_channel_prefs
             WHERE user_id = $1
             ORDER BY category, channel",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Ошибка запроса настроек уведомлений")?;

        Ok(prefs)
    }

    /// Заменяет настройки каналов уведомлений пользователя (UPSERT всех переданных записей).
    pub async fn set_preferences(
        &self,
        user_id: i64,
        prefs: Vec<NotificationChannelPref>,
    ) -> Result<()> {
        // Транзакционно удаляем старые и вставляем новые
        let mut tx = self.pool.begin().await.context("Ошибка начала транзакции")?;

        sqlx::query("DELETE FROM notification_channel_prefs WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("Ошибка удаления старых настроек")?;

        for pref in prefs {
            sqlx::query(
                "INSERT INTO notification_channel_prefs (user_id, category, channel, enabled)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(user_id)
            .bind(&pref.category)
            .bind(&pref.channel)
            .bind(pref.enabled)
            .execute(&mut *tx)
            .await
            .context("Ошибка вставки настройки")?;
        }

        tx.commit().await.context("Ошибка коммита транзакции")?;
        Ok(())
    }

    /// Создаёт уведомления для группы пользователей.
    ///
    /// `segment`:
    ///   - "all"         — все пользователи
    ///   - "active_subs" — только с активными подписками
    ///   - "trial"       — пользователи без платных подписок (только бесплатные/trial)
    ///
    /// Возвращает количество созданных уведомлений.
    pub async fn broadcast(
        &self,
        category: &str,
        severity: &str,
        title: &str,
        body: &str,
        payload: Option<Value>,
        segment: &str,
    ) -> Result<i64> {
        let user_ids: Vec<i64> = match segment {
            "active_subs" => {
                sqlx::query_scalar(
                    "SELECT DISTINCT user_id FROM subscriptions WHERE status = 'active'",
                )
                .fetch_all(&self.pool)
                .await?
            }
            "trial" => {
                sqlx::query_scalar(
                    "SELECT id FROM users
                     WHERE id NOT IN (
                         SELECT DISTINCT user_id FROM subscriptions
                         WHERE status = 'active' AND plan_id IN (SELECT id FROM plans WHERE price_kopecks > 0)
                     )",
                )
                .fetch_all(&self.pool)
                .await?
            }
            _ => {
                // "all" и любой другой сегмент — все пользователи
                sqlx::query_scalar("SELECT id FROM users")
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        let mut count = 0i64;
        for uid in user_ids {
            match self
                .create(uid, category, severity, title, body, payload.clone())
                .await
            {
                Ok(_) => count += 1,
                Err(e) => error!("Ошибка создания уведомления для user {}: {}", uid, e),
            }
        }

        Ok(count)
    }
}

/// Экранирует специальные символы MarkdownV2 для Telegram.
fn escape_md(s: &str) -> String {
    let special = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
