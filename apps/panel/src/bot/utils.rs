pub fn escape_md(text: &str) -> String {
    text.replace(".", "\\.")
        .replace("-", "\\-")
        .replace("_", "\\_")
        .replace("*", "\\*")
        .replace("[", "\\[")
        .replace("]", "\\]")
        .replace("(", "\\(")
        .replace(")", "\\)")
        .replace("~", "\\~")
        .replace("`", "\\`")
        .replace(">", "\\>")
        .replace("#", "\\#")
        .replace("+", "\\+")
        .replace("=", "\\=")
        .replace("|", "\\|")
        .replace("{", "\\{")
        .replace("}", "\\}")
        .replace("!", "\\!")
}

// Channel Trial Helpers
use teloxide::prelude::*;
use crate::AppState;
use tracing::error;

pub async fn register_bot_message(bot: Bot, state: &AppState, user_id: i64, sent_msg: &Message) {
    let chat_id = sent_msg.chat.id.0;
    let msg_id = sent_msg.id.0;
    
    // Add current message
    if let Err(e) = state.store_service.add_bot_message_to_history(user_id, chat_id, msg_id).await {
        error!("Failed to track bot msg: {}", e);
        return;
    }

    // Cleanup (Keep 3)
    match state.store_service.cleanup_bot_history(user_id, 3).await {
        Ok(items) => {
            for (cid, mid) in items {
                // Best effort delete
                let _ = bot.delete_message(teloxide::types::ChatId(cid), teloxide::types::MessageId(mid)).await;
            }
        },
        Err(e) => error!("Failed to cleanup bot history: {}", e)
    }
}
use teloxide::types::{ChatId, UserId, ChatMemberKind};

/// Check if user is a member of the required channel
pub async fn check_channel_membership(bot: &teloxide::Bot, user_id: i64) -> anyhow::Result<bool> {
    let channel_id = match std::env::var("REQUIRED_CHANNEL_ID") {
        Ok(id) if !id.is_empty() => match id.parse::<i64>() {
            Ok(n) => n,
            Err(_) => return Ok(false),
        },
        _ => return Ok(false),
    };
    
    match bot.get_chat_member(ChatId(channel_id), UserId(user_id as u64)).await {
        Ok(member) => Ok(matches!(
            member.kind,
            ChatMemberKind::Administrator(_) | ChatMemberKind::Owner(_) | ChatMemberKind::Member
        )),
        Err(_) => Ok(false),
    }
}

/// Get trial duration based on channel membership
pub fn get_trial_days(is_channel_member: bool) -> i64 {
    if is_channel_member {
        std::env::var("CHANNEL_TRIAL_DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(7)
    } else {
        std::env::var("FREE_TRIAL_DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(3)
    }
}
