use anyhow::{Context, Result};
use sqlx::SqlitePool;
use teloxide::prelude::*;
use tracing::{info, warn};

/// Service for sending notifications to users via Telegram bot
pub struct NotificationService {
    pool: SqlitePool,
}

impl NotificationService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Notify users affected by SNI rotation on a specific node
    ///
    /// # Arguments
    /// * `bot` - Telegram bot instance
    /// * `node_id` - ID of the node that had SNI rotated
    /// * `old_sni` - Previous SNI domain
    /// * `new_sni` - New SNI domain
    /// * `rotation_id` - Database ID of rotation log entry
    ///
    /// # Returns
    /// Number of users successfully notified
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

        // Get affected users
        let users = self.get_affected_users(node_id).await?;
        
        if users.is_empty() {
            info!("No active users found on node {}, skipping notifications", node_id);
            return Ok(0);
        }

        info!("Found {} active users to notify on node {}", users.len(), node_id);

        let mut notified_count = 0;
        let mut failed_count = 0;

        for user in &users {
            let message = self.format_rotation_message(old_sni, new_sni, rotation_id);
            
            match bot.send_message(ChatId(user.tg_id), message)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await
            {
                Ok(_) => {
                    notified_count += 1;
                    info!("✓ Notified user {} (TG: {})", user.username, user.tg_id);
                }
                Err(e) => {
                    failed_count += 1;
                    warn!("✗ Failed to notify user {} (TG: {}): {}", user.username, user.tg_id, e);
                }
            }

            // Rate limiting: Telegram allows ~30 messages/second
            // Using 50ms delay = 20 messages/second to be safe
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

    /// Query users affected by SNI rotation on a specific node
    ///
    /// Returns users with:
    /// - Active subscriptions on the rotated node
    /// - Subscription not expired OR trial not expired
    async fn get_affected_users(&self, node_id: i64) -> Result<Vec<AffectedUser>> {
        let users = sqlx::query_as::<_, AffectedUser>(
            "SELECT DISTINCT 
                u.id, 
                u.tg_id, 
                COALESCE(u.username, 'User') as username
             FROM users u
             INNER JOIN subscriptions s ON u.id = s.user_id
             WHERE s.node_id = ?
               AND s.status = 'active'
               AND (
                   s.expires_at > datetime('now') 
                   OR u.trial_expires_at > datetime('now')
               )
             ORDER BY u.id"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to query affected users for SNI rotation")?;

        Ok(users)
    }

    /// Format SNI rotation notification message for Telegram
    ///
    /// Uses HTML formatting with clear action items
    fn format_rotation_message(&self, old_sni: &str, new_sni: &str, rotation_id: i64) -> String {
        format!(
            "⚠️ <b>Connection Update Required</b>\n\n\
             Your VPN configuration has been automatically updated for improved stability.\n\n\
             <b>Previous domain:</b> <code>{}</code>\n\
             <b>New domain:</b> <code>{}</code>\n\n\
             <b>📱 Action Required:</b>\n\
             Please reconnect to apply the changes:\n\
             1️⃣ Disconnect from VPN\n\
             2️⃣ Wait 10 seconds\n\
             3️⃣ Reconnect to VPN\n\n\
             Your new configuration is ready. No need to re-download.\n\n\
             <i>Rotation ID: #{}</i>",
            old_sni, new_sni, rotation_id
        )
    }
}

/// User affected by SNI rotation
#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct AffectedUser {
    id: i64,
    tg_id: i64,
    username: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_formatting() {
        let service = NotificationService::new(
            sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap()
        );
        
        let message = service.format_rotation_message(
            "www.google.com",
            "www.cloudflare.com",
            42
        );
        
        assert!(message.contains("www.google.com"));
        assert!(message.contains("www.cloudflare.com"));
        assert!(message.contains("Rotation ID: #42"));
        assert!(message.contains("Disconnect from VPN"));
    }
}
