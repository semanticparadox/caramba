use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use tracing::{info, warn, error};

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};



pub struct BotManager {
    shutdown_sender: broadcast::Sender<()>,
    current_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    current_bot: Arc<Mutex<Option<Bot>>>,
    bot_username: Arc<RwLock<Option<String>>>,
}

impl BotManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1);
        Self {
            shutdown_sender: tx,
            current_handle: Arc::new(Mutex::new(None)),
            current_bot: Arc::new(Mutex::new(None)),
            bot_username: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start_bot(&self, token: String, state: crate::AppState) -> bool {
        let mut handle_lock = self.current_handle.lock().await;
        
        if handle_lock.is_some() {
            warn!("Bot is already running, stop it first");
            return false;
        }

        info!("Starting new bot instance...");
        let bot = Bot::new(token.clone());
        
        // Validate token + fetch username before exposing bot as running.
        match bot.get_me().await {
            Ok(me) => {
                let username = me.user.username.clone();
                {
                    let mut lock = self.bot_username.write().await;
                    *lock = username.clone();
                }
                if let Some(username) = username {
                    let _ = state.settings.set("bot_username", &username).await;
                    info!("Bot username initialized: @{}", username);
                } else {
                    info!("Bot connected but username is not set");
                }
            }
            Err(e) => {
                error!("Failed to validate bot token: {}", e);
                return false;
            }
        }

        // Store bot clone for notifications
        let mut bot_lock = self.current_bot.lock().await;
        *bot_lock = Some(bot.clone());
        drop(bot_lock);

        let shutdown_rx = self.shutdown_sender.subscribe();

        let handle = tokio::spawn(async move {
            info!("Bot task started in lightweight mode");
            let mut rx = shutdown_rx;
            let _ = rx.recv().await;
            info!("Bot task finished/stopped");
        });

        *handle_lock = Some(handle);
        true
    }

    pub async fn stop_bot(&self) -> bool {
        let mut handle_lock = self.current_handle.lock().await;
        
        if let Some(handle) = handle_lock.take() {
            info!("Sending shutdown signal to bot...");
            let _ = self.shutdown_sender.send(());
            // Wait for the task to finish (optional, but good for clean termination)
            let _ = handle.await;
            info!("Bot task stopped");
            
            let mut bot_username = self.bot_username.write().await;
            *bot_username = None;

            let mut bot_lock = self.current_bot.lock().await;
            *bot_lock = None;
            
            true
        } else {
            warn!("Bot is not running");
            false
        }
    }

    pub async fn is_running(&self) -> bool {
        self.current_handle.lock().await.is_some()
    }

    pub async fn get_bot(&self) -> Result<Bot, String> {
        let bot_lock = self.current_bot.lock().await;
        bot_lock.clone().ok_or_else(|| "Bot not running".to_string())
    }

    pub async fn get_username(&self) -> Option<String> {
        self.bot_username.read().await.clone()
    }

    pub async fn send_notification(&self, chat_id: i64, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bot_lock = self.current_bot.lock().await;
        if let Some(bot) = bot_lock.as_ref() {
            bot.send_message(ChatId(chat_id), text).parse_mode(ParseMode::MarkdownV2).await?;
            Ok(())
        } else {
            warn!("Cannot send notification: bot is not running");
            Ok(()) // Don't crash caller if bot is stopped
        }
    }
}
