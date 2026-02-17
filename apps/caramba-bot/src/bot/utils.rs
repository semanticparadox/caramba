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
// End of file
