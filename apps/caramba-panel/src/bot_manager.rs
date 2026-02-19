use std::sync::Arc;

use teloxide::dptree;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode, Update, WebAppInfo,
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

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

        if let Some(existing) = handle_lock.as_ref() {
            if existing.is_finished() {
                info!("Detected finished bot task. Cleaning stale handle.");
                *handle_lock = None;
                let mut bot_lock = self.current_bot.lock().await;
                *bot_lock = None;
            } else {
                warn!("Bot is already running, stop it first");
                return false;
            }
        }

        info!("Starting new bot instance...");
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

        let mut bot_lock = self.current_bot.lock().await;
        *bot_lock = Some(bot.clone());
        drop(bot_lock);

        let shutdown_rx = self.shutdown_sender.subscribe();
        let state_clone = state.clone();

        let handle = tokio::spawn(async move {
            info!("Bot task started with dispatcher mode");
            run_panel_bot(bot, shutdown_rx, state_clone).await;
            info!("Bot dispatcher task finished");
        });

        *handle_lock = Some(handle);
        true
    }

    pub async fn stop_bot(&self) -> bool {
        let mut handle_lock = self.current_handle.lock().await;

        if let Some(handle) = handle_lock.take() {
            info!("Sending shutdown signal to bot...");
            let _ = self.shutdown_sender.send(());
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
        let mut handle_lock = self.current_handle.lock().await;
        if let Some(handle) = handle_lock.as_ref() {
            if handle.is_finished() {
                info!("Bot task is finished, resetting running state");
                *handle_lock = None;
                let mut bot_lock = self.current_bot.lock().await;
                *bot_lock = None;
                let mut bot_username = self.bot_username.write().await;
                *bot_username = None;
                return false;
            }
            return true;
        }
        false
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
        let bot_lock = self.current_bot.lock().await;
        if let Some(bot) = bot_lock.as_ref() {
            bot.send_message(ChatId(chat_id), text)
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
            Ok(())
        } else {
            warn!("Cannot send notification: bot is not running");
            Ok(())
        }
    }
}

async fn run_panel_bot(
    bot: Bot,
    mut shutdown_signal: tokio::sync::broadcast::Receiver<()>,
    state: crate::AppState,
) {
    info!("Starting embedded panel bot dispatcher...");

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handle_message))
        .branch(Update::filter_callback_query().endpoint(handle_callback));

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .default_handler(|upd: std::sync::Arc<Update>| async move {
            info!("Unhandled bot update: {:?}", upd);
        })
        .build();

    tokio::select! {
        _ = dispatcher.dispatch() => {
            info!("Embedded bot dispatcher exited");
        }
        _ = shutdown_signal.recv() => {
            info!("Embedded bot received shutdown signal");
        }
    }
}

async fn handle_message(
    bot: Bot,
    msg: Message,
    state: crate::AppState,
) -> Result<(), teloxide::RequestError> {
    let Some(text) = msg.text() else {
        return Ok(());
    };

    let tg_id = msg.chat.id.0 as i64;
    let command = text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();

    match command.as_str() {
        "/start" => {
            let start_payload = text.strip_prefix("/start").unwrap_or("").trim();
            let referrer_id = if start_payload.is_empty() {
                None
            } else {
                state
                    .store_service
                    .resolve_referrer_id(start_payload)
                    .await
                    .ok()
                    .flatten()
            };
            let full_name = msg
                .from
                .as_ref()
                .map(|u| u.full_name())
                .unwrap_or_else(|| "User".to_string());
            let username = msg.from.as_ref().and_then(|u| u.username.as_deref());

            let user = match state
                .store_service
                .upsert_user(tg_id, username, Some(&full_name), referrer_id)
                .await
            {
                Ok(user) => user,
                Err(e) => {
                    error!("Failed to upsert user on /start: {}", e);
                    bot.send_message(
                        msg.chat.id,
                        "Failed to initialize your profile. Please try again later.",
                    )
                    .await?;
                    return Ok(());
                }
            };

            if user.terms_accepted_at.is_none() {
                return send_terms(&bot, msg.chat.id, &state).await;
            }

            send_welcome(&bot, msg.chat.id, &state).await
        }
        "/app" => {
            match state.store_service.get_user_by_tg_id(tg_id).await {
                Ok(Some(user)) => {
                    if user.terms_accepted_at.is_none() {
                        return send_terms(&bot, msg.chat.id, &state).await;
                    }
                    send_welcome(&bot, msg.chat.id, &state).await
                }
                _ => {
                    bot.send_message(msg.chat.id, "Use /start first to initialize your profile.")
                        .await?;
                    Ok(())
                }
            }
        }
        "/help" => {
            let help_text = "<b>Commands</b>\n\n/start - initialize profile\n/app - open mini app button\n/help - show this help";
            bot.send_message(msg.chat.id, help_text)
                .parse_mode(ParseMode::Html)
                .await?;
            Ok(())
        }
        _ => {
            // Keep bot responsive instead of silent ignoring.
            bot.send_message(msg.chat.id, "Unknown command. Use /help.")
                .await?;
            Ok(())
        }
    }
}

async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    state: crate::AppState,
) -> Result<(), teloxide::RequestError> {
    let callback_id = q.id.clone();
    let tg_id = q.from.id.0 as i64;
    let data = q.data.clone().unwrap_or_default();

    if data != "accept_terms" {
        bot.answer_callback_query(callback_id).await?;
        return Ok(());
    }

    bot.answer_callback_query(callback_id)
        .text("Terms accepted")
        .await?;

    if let Ok(Some(user)) = state.store_service.get_user_by_tg_id(tg_id).await {
        if let Err(e) = state.store_service.update_user_terms(user.id).await {
            error!("Failed to accept user terms for {}: {}", user.id, e);
        }
    }

    if let Some(message) = q.message {
        let _ = bot.delete_message(message.chat().id, message.id()).await;
        return send_welcome(&bot, message.chat().id, &state).await;
    }

    send_welcome(&bot, ChatId(tg_id), &state).await
}

async fn send_welcome(
    bot: &Bot,
    chat_id: ChatId,
    state: &crate::AppState,
) -> Result<(), teloxide::RequestError> {
    let base_text =
        "<b>Welcome to CARAMBA</b>\n\nUse the button below to open your client interface.";
    if let Some(url) = resolve_miniapp_url(state).await {
        if let Ok(parsed) = url::Url::parse(&url) {
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::web_app(
                "Open App",
                WebAppInfo { url: parsed },
            )]]);
            bot.send_message(chat_id, base_text)
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard)
                .await?;
            return Ok(());
        }
    }

    bot.send_message(chat_id, base_text)
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

async fn send_terms(
    bot: &Bot,
    chat_id: ChatId,
    state: &crate::AppState,
) -> Result<(), teloxide::RequestError> {
    let terms = state
        .store_service
        .get_setting("terms_of_service")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "Welcome to CARAMBA.".to_string());
    let text = format!(
        "<b>User Agreement</b>\n\n{}\n\nPress <b>Accept</b> to continue.",
        terms
    );
    bot.send_message(chat_id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(terms_keyboard())
        .await?;
    Ok(())
}

async fn resolve_miniapp_url(state: &crate::AppState) -> Option<String> {
    let explicit = state.settings.get_or_default("mini_app_url", "").await;
    if !explicit.trim().is_empty() {
        return Some(explicit.trim().to_string());
    }

    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    if sub_domain.trim().is_empty() {
        return None;
    }

    Some(format!("https://{}/app", sub_domain.trim()))
}

fn terms_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Accept",
        "accept_terms",
    )]])
}
