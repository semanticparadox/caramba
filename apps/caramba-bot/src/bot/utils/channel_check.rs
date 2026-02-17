use teloxide::prelude::*;
use teloxide::types::{ChatId, UserId};
use anyhow::Result;

/// Check if user is a member of the required channel
pub async fn check_channel_membership(bot: &Bot, user_id: i64) -> Result<bool> {
    // Get channel ID from environment
    let channel_id = match std::env::var("REQUIRED_CHANNEL_ID") {
        Ok(id) if !id.is_empty() => match id.parse::<i64>() {
            Ok(n) => n,
            Err(_) => {
                tracing::warn!("Invalid REQUIRED_CHANNEL_ID format: {}", id);
                return Ok(false);
            }
        },
        _ => return Ok(false), // No channel configured
    };
    
    // Check membership using Telegram Bot API
    match bot.get_chat_member(ChatId(channel_id), UserId(user_id as u64)).await {
        Ok(member) => {
            use teloxide::types::ChatMemberKind;
            let is_member = matches!(
                member.kind,
                ChatMemberKind::Administrator(_) |
                ChatMemberKind::Owner(_) |
                ChatMemberKind::Member
            );
            
            if is_member {
                tracing::info!("User {} is a member of channel {}", user_id, channel_id);
            } else {
                tracing::debug!("User {} is NOT a member of channel {} (status: {:?})", user_id, channel_id, member.kind);
            }
            
            Ok(is_member)
        }
        Err(e) => {
            tracing::warn!("Failed to check channel membership for user {}: {}", user_id, e);
            Ok(false) // Fail gracefully
        }
    }
}

/// Get trial duration based on channel membership
pub fn get_trial_days(is_channel_member: bool) -> i64 {
    if is_channel_member {
        std::env::var("CHANNEL_TRIAL_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7)
    } else {
        std::env::var("FREE_TRIAL_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    }
}
