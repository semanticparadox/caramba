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
use teloxide::prelude::Requester;
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
