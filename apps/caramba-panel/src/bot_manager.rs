use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::Bot;
use teloxide::types::{
    ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, LinkPreviewOptions, ParseMode,
};
use tracing::{error, info, warn};

use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Copy, Default)]
pub enum NotificationParseMode {
    #[default]
    Plain,
    MarkdownV2,
    Html,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum NotificationMediaType {
    #[default]
    None,
    Photo,
    Video,
    Document,
}

#[derive(Debug, Clone)]
pub struct NotificationPayload {
    pub text: String,
    pub parse_mode: NotificationParseMode,
    pub media_type: NotificationMediaType,
    pub media_url: Option<String>,
    pub buttons: Vec<(String, String)>,
    pub disable_link_preview: bool,
}

impl NotificationPayload {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            parse_mode: NotificationParseMode::Plain,
            media_type: NotificationMediaType::None,
            media_url: None,
            buttons: Vec::new(),
            disable_link_preview: false,
        }
    }

    pub fn legacy_markdown_v2(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            parse_mode: NotificationParseMode::MarkdownV2,
            media_type: NotificationMediaType::None,
            media_url: None,
            buttons: Vec::new(),
            disable_link_preview: false,
        }
    }
}

pub struct BotManager {
    current_bot: Arc<Mutex<Option<Bot>>>,
    bot_username: Arc<RwLock<Option<String>>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<()>>>>,
}

impl BotManager {
    pub fn new() -> Self {
        Self {
            current_bot: Arc::new(Mutex::new(None)),
            bot_username: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_bot(&self, token: String, state: crate::AppState) -> bool {
        let mut bot_lock = self.current_bot.lock().await;

        if bot_lock.is_some() {
            warn!("Bot notifier is already running, stop it first");
            return false;
        }

        info!("Starting new bot notifier instance...");
        let bot = Bot::new(token.clone());

        match bot.get_me().await {
            Ok(me) => {
                let username = me.user.username.clone();
                {
                    let mut lock = self.bot_username.write().await;
                    *lock = username.clone();
                }
                if let Some(username) = username {
                    let _ = state.settings.set("bot_username", &username).await;
                    info!("Bot notifier username initialized: @{}", username);
                } else {
                    info!("Bot notifier connected but username is not set");
                }
            }
            Err(e) => {
                error!("Failed to validate bot token: {}", e);
                return false;
            }
        }

        *bot_lock = Some(bot.clone());

        // Spawn background dispatcher
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        {
            let mut shutdown_lock = self.shutdown_tx.lock().await;
            *shutdown_lock = Some(tx);
        }

        tokio::spawn(async move {
            crate::bot::run_bot(bot, rx, state).await;
        });

        true
    }

    pub async fn stop_bot(&self) -> bool {
        let mut bot_lock = self.current_bot.lock().await;

        if bot_lock.take().is_some() {
            info!("Bot notifier stopping...");

            // Send shutdown signal to dispatcher
            let mut shutdown_lock = self.shutdown_tx.lock().await;
            if let Some(tx) = shutdown_lock.take() {
                let _ = tx.send(());
            }

            let mut bot_username = self.bot_username.write().await;
            *bot_username = None;

            info!("Bot notifier stopped");
            true
        } else {
            warn!("Bot notifier is not running");
            false
        }
    }

    pub async fn is_running(&self) -> bool {
        self.current_bot.lock().await.is_some()
    }

    pub async fn get_bot(&self) -> Result<Bot, String> {
        let bot_lock = self.current_bot.lock().await;
        bot_lock
            .clone()
            .ok_or_else(|| "Bot not running".to_string())
    }

    pub async fn get_username(&self) -> Option<String> {
        self.bot_username.read().await.clone()
    }

    pub async fn send_notification(
        &self,
        chat_id: i64,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = NotificationPayload::legacy_markdown_v2(text);
        self.send_rich_notification(chat_id, payload).await
    }

    pub async fn send_rich_notification(
        &self,
        chat_id: i64,
        payload: NotificationPayload,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bot_lock = self.current_bot.lock().await;
        if let Some(bot) = bot_lock.as_ref() {
            let mut buttons: Vec<InlineKeyboardButton> = Vec::new();
            for (text, url) in payload.buttons.iter() {
                let text = text.trim();
                let url = url.trim();
                if text.is_empty() || url.is_empty() {
                    continue;
                }
                let parsed = url::Url::parse(url)?;
                buttons.push(InlineKeyboardButton::url(text.to_string(), parsed));
            }

            let keyboard = if buttons.is_empty() {
                None
            } else {
                let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
                for chunk in buttons.chunks(2) {
                    rows.push(chunk.to_vec());
                }
                Some(InlineKeyboardMarkup::new(rows))
            };

            if let Some(media_url) = payload.media_url.as_deref().map(str::trim) {
                if !media_url.is_empty()
                    && !matches!(payload.media_type, NotificationMediaType::None)
                {
                    if payload.text.chars().count() > 1024 {
                        return Err("Caption too long for media message (max 1024 chars)"
                            .to_string()
                            .into());
                    }

                    let media_parsed = url::Url::parse(media_url)?;
                    match payload.media_type {
                        NotificationMediaType::Photo => {
                            let mut req = bot
                                .send_photo(ChatId(chat_id), InputFile::url(media_parsed))
                                .caption(payload.text.clone());

                            req = match payload.parse_mode {
                                NotificationParseMode::Plain => req,
                                NotificationParseMode::MarkdownV2 => {
                                    req.parse_mode(ParseMode::MarkdownV2)
                                }
                                NotificationParseMode::Html => req.parse_mode(ParseMode::Html),
                            };

                            if let Some(markup) = keyboard.clone() {
                                req = req.reply_markup(markup);
                            }

                            req.await?;
                        }
                        NotificationMediaType::Video => {
                            let mut req = bot
                                .send_video(ChatId(chat_id), InputFile::url(media_parsed))
                                .caption(payload.text.clone());

                            req = match payload.parse_mode {
                                NotificationParseMode::Plain => req,
                                NotificationParseMode::MarkdownV2 => {
                                    req.parse_mode(ParseMode::MarkdownV2)
                                }
                                NotificationParseMode::Html => req.parse_mode(ParseMode::Html),
                            };

                            if let Some(markup) = keyboard.clone() {
                                req = req.reply_markup(markup);
                            }

                            req.await?;
                        }
                        NotificationMediaType::Document => {
                            let mut req = bot
                                .send_document(ChatId(chat_id), InputFile::url(media_parsed))
                                .caption(payload.text.clone());

                            req = match payload.parse_mode {
                                NotificationParseMode::Plain => req,
                                NotificationParseMode::MarkdownV2 => {
                                    req.parse_mode(ParseMode::MarkdownV2)
                                }
                                NotificationParseMode::Html => req.parse_mode(ParseMode::Html),
                            };

                            if let Some(markup) = keyboard.clone() {
                                req = req.reply_markup(markup);
                            }

                            req.await?;
                        }
                        NotificationMediaType::None => {}
                    }
                    return Ok(());
                }
            }

            let mut req = bot.send_message(ChatId(chat_id), payload.text.clone());
            req = match payload.parse_mode {
                NotificationParseMode::Plain => req,
                NotificationParseMode::MarkdownV2 => req.parse_mode(ParseMode::MarkdownV2),
                NotificationParseMode::Html => req.parse_mode(ParseMode::Html),
            };

            if payload.disable_link_preview {
                req = req.link_preview_options(LinkPreviewOptions {
                    is_disabled: true,
                    url: None,
                    prefer_small_media: false,
                    prefer_large_media: false,
                    show_above_text: false,
                });
            }

            if let Some(markup) = keyboard {
                req = req.reply_markup(markup);
            }

            req.await?;
            Ok(())
        } else {
            warn!("Cannot send notification: bot is not running");
            Ok(())
        }
    }
}
