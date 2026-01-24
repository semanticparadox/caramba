use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use tracing::{info, warn};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::bot::run_bot;

pub struct BotManager {
    shutdown_sender: broadcast::Sender<()>,
    current_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    current_bot: Arc<Mutex<Option<Bot>>>,
}

impl BotManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1);
        Self {
            shutdown_sender: tx,
            current_handle: Arc::new(Mutex::new(None)),
            current_bot: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_bot(&self, token: String, state: crate::AppState) -> bool {
        let mut handle_lock = self.current_handle.lock().await;
        
        if handle_lock.is_some() {
            warn!("Bot is already running, stop it first");
            return false;
        }

        info!("Starting new bot instance...");
        let bot = Bot::new(token);
        
        // Store bot clone for notifications
        let mut bot_lock = self.current_bot.lock().await;
        *bot_lock = Some(bot.clone());
        drop(bot_lock);

        let shutdown_rx = self.shutdown_sender.subscribe();

        let handle = tokio::spawn(async move {
            run_bot(bot, shutdown_rx, state).await;
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
            true
        } else {
            warn!("Bot is not running");
            false
        }
    }

    pub async fn is_running(&self) -> bool {
        self.current_handle.lock().await.is_some()
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
