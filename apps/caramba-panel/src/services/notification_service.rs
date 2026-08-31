use anyhow::{Context, Result};
use sqlx::PgPool;
use teloxide::prelude::*;
use tracing::{info, warn};

use crate::bot::translations::{Lang, default_language_setting, resolve_lang, tf};
use crate::bot::utils::escape_html;

/// Service for sending notifications to users via Telegram bot
pub struct NotificationService {
    pool: PgPool,
}

impl NotificationService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Notify users affected by SNI rotation on a specific node
    pub async fn notify_sni_rotation(
        &self,
        bot: &Bot,
        node_id: i64,
        old_sni: &str,
        new_sni: &str,
        rotation_id: i64,
    ) -> Result<usize> {
        info!(
            "Starting SNI rotation notifications for node {}: {} → {} (rotation #{})",
            node_id, old_sni, new_sni, rotation_id
        );

        let users = self.get_affected_users(node_id).await?;

        if users.is_empty() {
            info!(
                "No active users found on node {}, skipping notifications",
                node_id
            );
            return Ok(0);
        }

        info!(
            "Found {} active users to notify on node {}",
            users.len(),
            node_id
        );

        // Настройку читаем один раз на всю рассылку, а не на каждого адресата.
        let default_language = default_language_setting(&self.pool).await;

        // То же и для переопределений текста из панели: два запроса на ротацию,
        // а не по одному на адресата. Эта рассылка идёт сырым ботом, поэтому
        // кнопка и медиа здесь не поддерживаются — редактор их для этого
        // события и не показывает.
        let override_ru =
            crate::services::notification_templates::text_override(&self.pool, "notify.sni_rotation", Lang::Ru)
                .await;
        let override_en =
            crate::services::notification_templates::text_override(&self.pool, "notify.sni_rotation", Lang::En)
                .await;

        let mut notified_count = 0;
        let mut failed_count = 0;

        for user in &users {
            let lang = resolve_lang(user.language_code.as_deref(), default_language.as_deref());
            let custom = match lang {
                Lang::Ru => override_ru.as_deref(),
                Lang::En => override_en.as_deref(),
            };
            let message =
                self.format_rotation_message_with(custom, lang, old_sni, new_sni, rotation_id);

            match bot
                .send_message(ChatId(user.tg_id), message)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await
            {
                Ok(_) => {
                    notified_count += 1;
                    info!("✓ Notified user {} (TG: {})", user.username, user.tg_id);
                }
                Err(e) => {
                    failed_count += 1;
                    warn!(
                        "✗ Failed to notify user {} (TG: {}): {}",
                        user.username, user.tg_id, e
                    );
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        info!(
            "SNI rotation notifications complete: {}/{} sent, {} failed",
            notified_count,
            users.len(),
            failed_count
        );

        Ok(notified_count)
    }

    async fn get_affected_users(&self, node_id: i64) -> Result<Vec<AffectedUser>> {
        let users = sqlx::query_as::<_, AffectedUser>(
            "SELECT DISTINCT 
                u.id, 
                u.tg_id, 
                COALESCE(u.username, 'User') as username
                ,u.language_code
             FROM users u
             INNER JOIN subscriptions s ON u.id = s.user_id
             WHERE s.node_id = $1
               AND s.status = 'active'
               AND s.expires_at > CURRENT_TIMESTAMP
             ORDER BY u.id",
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to query affected users for SNI rotation")?;

        Ok(users)
    }

    /// Сообщение отправляется в HTML parse mode (см. вызов выше), поэтому
    /// подставляемые домены экранируются под HTML.
    fn format_rotation_message(
        &self,
        lang: Lang,
        old_sni: &str,
        new_sni: &str,
        rotation_id: i64,
    ) -> String {
        self.format_rotation_message_with(None, lang, old_sni, new_sni, rotation_id)
    }

    /// То же, но поверх текста, отредактированного в панели. `None` —
    /// переопределения нет, берётся встроенная строка. Подстановка идёт через
    /// ту же `substitute`, что и у `tf`, поэтому отредактированный текст не
    /// может подставить иначе, чем встроенный.
    fn format_rotation_message_with(
        &self,
        custom: Option<&str>,
        lang: Lang,
        old_sni: &str,
        new_sni: &str,
        rotation_id: i64,
    ) -> String {
        let args = [
            escape_html(old_sni),
            escape_html(new_sni),
            rotation_id.to_string(),
        ];
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match custom {
            Some(template) => crate::bot::translations::substitute(template, &args),
            None => tf(lang, "notify.sni_rotation", &args),
        }
    }
}

/// User affected by SNI rotation
#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct AffectedUser {
    id: i64,
    tg_id: i64,
    username: String,
    language_code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_formatting() {
        // Mock pool not needed for formatting test
        // But we need to use a real lazy pool if we want to instantiate it
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let service = NotificationService::new(pool);

        // По умолчанию — русский.
        let ru =
            service.format_rotation_message(Lang::Ru, "www.google.com", "www.cloudflare.com", 42);
        assert!(ru.contains("www.google.com"));
        assert!(ru.contains("www.cloudflare.com"));
        assert!(ru.contains("ID ротации: #42"));
        assert!(ru.contains("Отключите VPN"));

        // Английский — только по явному выбору пользователя.
        let en =
            service.format_rotation_message(Lang::En, "www.google.com", "www.cloudflare.com", 42);
        assert!(en.contains("Rotation ID: #42"));
        assert!(en.contains("Disconnect from VPN"));
    }

    /// Домены попадают в HTML-сообщение, поэтому `<` и `&` обязаны быть
    /// экранированы — иначе Telegram отклонит всё сообщение как битый HTML.
    #[tokio::test]
    async fn rotation_message_escapes_html() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let service = NotificationService::new(pool);

        let msg = service.format_rotation_message(Lang::Ru, "a<b&c", "d>e", 1);
        assert!(msg.contains("a&lt;b&amp;c"));
        assert!(msg.contains("d&gt;e"));
    }
}
