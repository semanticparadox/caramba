/// Экранирование для Telegram **HTML** parse mode.
///
/// В HTML-режиме Telegram требует экранировать ровно три символа: `&`, `<`, `>`
/// (https://core.telegram.org/bots/api#html-style). Это на порядок безопаснее
/// MarkdownV2 для русского текста, где `.`, `!`, `-`, `(`, `)` встречаются в
/// каждом втором предложении, и одна пропущенная обратная косая черта роняет
/// отправку сообщения целиком.
///
/// `&` заменяется первым — иначе он повторно экранировал бы уже вставленные
/// `&lt;` / `&gt;`.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// Bot message helpers
use crate::AppState;
use crate::bot::translations::{Lang, lang_for};
use teloxide::prelude::*;
use tracing::error;

/// Язык пользователя по его Telegram id.
///
/// Порядок разрешения — общий для всего проекта (см. [`crate::bot::translations`]):
/// `users.language_code` → настройка `default_language` → `ru`. Неизвестный
/// пользователь просто не даёт первого шага и получает язык по умолчанию.
pub async fn lang_by_tg_id(state: &AppState, tg_id: i64) -> Lang {
    let lang: Option<String> =
        sqlx::query_scalar("SELECT language_code FROM users WHERE tg_id = $1")
            .bind(tg_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
            .flatten();
    lang_for(&state.settings, lang.as_deref()).await
}

pub async fn register_bot_message(bot: Bot, state: &AppState, user_id: i64, sent_msg: &Message) {
    let chat_id = sent_msg.chat.id.0;
    let msg_id = sent_msg.id.0;

    // Add current message
    if let Err(e) = state
        .store_service
        .add_bot_message_to_history(user_id, chat_id, msg_id.into())
        .await
    {
        error!("Failed to track bot msg: {}", e);
        return;
    }

    // Cleanup (Keep 3)
    match state.store_service.cleanup_bot_history(user_id, 3).await {
        Ok(items) => {
            for (cid, mid) in items {
                // Best effort delete
                let _ = bot
                    .delete_message(
                        teloxide::types::ChatId(cid),
                        teloxide::types::MessageId(mid as i32),
                    )
                    .await;
            }
        }
        Err(e) => error!("Failed to cleanup bot history: {}", e),
    }
}
// End of file
