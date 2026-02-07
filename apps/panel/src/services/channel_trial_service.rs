use anyhow::{Context, Result};
use sqlx::SqlitePool;
use teloxide::prelude::*;
use teloxide::types::{ChatId, ChatMemberKind, UserId};
use tracing::{info, warn};

/// Service for managing channel-based trial activation and verification
pub struct ChannelTrialService {
    pool: SqlitePool,
}

impl ChannelTrialService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Check if user is a member of the required Telegram channel
    pub async fn check_channel_membership(&self, bot: &Bot, user_id: i64) -> Result<bool> {
        // Get channel ID from environment
        let channel_id = match std::env::var("REQUIRED_CHANNEL_ID") {
            Ok(id) if !id.is_empty() => match id.parse::<i64>() {
                Ok(n) => n,
                Err(e) => {
                    warn!("Invalid REQUIRED_CHANNEL_ID format '{}': {}", id, e);
                    return Ok(false);
                }
            },
            _ => {
                // No channel configured - feature disabled
                return Ok(false);
            }
        };

        // Query Telegram Bot API for chat member status
        match bot.get_chat_member(ChatId(channel_id), UserId(user_id as u64)).await {
            Ok(member) => {
                let is_member = matches!(
                    member.kind,
                    ChatMemberKind::Administrator(_)
                        | ChatMemberKind::Owner(_)
                        | ChatMemberKind::Member(_)
                );

                if is_member {
                    info!("User {} verified as member of channel {}", user_id, channel_id);
                } else {
                    info!(
                        "User {} is NOT a member of channel {} (status: {:?})",
                        user_id, channel_id, member.kind
                    );
                }

                Ok(is_member)
            }
            Err(e) => {
                warn!("Failed to check channel membership for user {}: {}", user_id, e);
                // Fail gracefully - don't block trial activation
                Ok(false)
            }
        }
    }

    /// Get trial duration based on channel membership status
    fn get_trial_days(&self, is_channel_member: bool) -> i64 {
        if is_channel_member {
            std::env::var("CHANNEL_TRIAL_DAYS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(7) // Default: 7 days for channel members
        } else {
            std::env::var("FREE_TRIAL_DAYS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3) // Default: 3 days for regular users
        }
    }

    /// Activate trial for a user with automatic channel membership check
    /// Returns (trial_days, trial_source) tuple
    pub async fn activate_trial(&self, user_id: i64, bot: &Bot) -> Result<(i64, String)> {
        // Check channel membership
        let is_channel_member = self.check_channel_membership(bot, user_id).await?;

        // Determine trial parameters
        let trial_days = self.get_trial_days(is_channel_member);
        let trial_source = if is_channel_member { "channel" } else { "default" };

        // Activate trial in database
        sqlx::query(
            "UPDATE users 
             SET trial_expires_at = datetime('now', '+' || ? || ' days'),
                 channel_member_verified = ?,
                 channel_verified_at = CASE WHEN ? = 1 THEN CURRENT_TIMESTAMP ELSE NULL END,
                 trial_source = ?
             WHERE id = ?"
        )
        .bind(trial_days)
        .bind(is_channel_member as i32)
        .bind(is_channel_member as i32)
        .bind(trial_source)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to activate trial")?;

        info!(
            "Activated {} trial ({} days) for user {}",
            trial_source, trial_days, user_id
        );

        Ok((trial_days, trial_source.to_string()))
    }

    /// Periodic task: Verify channel membership for all users with channel trials
    /// Downgrade users who are no longer members
    pub async fn verify_and_downgrade_expired_members(&self, bot: &Bot) -> Result<()> {
        info!("Starting periodic channel membership verification");

        // Get all users with channel trials who aren't subscribed
        let users = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, tg_id FROM users 
             WHERE trial_source = 'channel' 
             AND channel_member_verified = 1
             AND subscription_status != 'active'"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch users for verification")?;

        info!("Verifying {} users with channel trials", users.len());

        let mut downgraded_count = 0;

        for (user_id, telegram_id) in users {
            // Re-check membership
            let is_still_member = self.check_channel_membership(bot, telegram_id).await.unwrap_or(false);

            if !is_still_member {
                // User left channel - downgrade to default trial
                let default_days = self.get_trial_days(false);
                
                sqlx::query(
                    "UPDATE users 
                     SET trial_expires_at = datetime('now', '+' || ? || ' days'),
                         channel_member_verified = 0,
                         channel_verified_at = NULL,
                         trial_source = 'default'
                     WHERE id = ?"
                )
                .bind(default_days)
                .bind(user_id)
                .execute(&self.pool)
                .await
                .ok(); // Soft fail on individual updates

                info!("Downgraded user {} - no longer in channel", user_id);
                downgraded_count += 1;
            }
        }

        info!(
            "Channel verification complete: {} users downgraded",
            downgraded_count
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_trial_days_default() {
        unsafe {
            std::env::set_var("FREE_TRIAL_DAYS", "3");
            std::env::set_var("CHANNEL_TRIAL_DAYS", "7");
        }

        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let service = ChannelTrialService::new(pool);

        assert_eq!(service.get_trial_days(false), 3);
        assert_eq!(service.get_trial_days(true), 7);
    }
}
