use std::sync::Arc;

use teloxide::dptree;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, LinkPreviewOptions,
    ParseMode, Update, WebAppInfo,
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
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

async fn run_panel_bot(
    bot: Bot,
    mut shutdown_signal: tokio::sync::broadcast::Receiver<()>,
    state: crate::AppState,
) {
    let handler = Update::filter_message().endpoint(handle_message);
    let callback_handler = Update::filter_callback_query().endpoint(handle_callback);

    let mut dispatcher = Dispatcher::builder(
        bot,
        dptree::entry().branch(handler).branch(callback_handler),
    )
    .dependencies(dptree::deps![state])
    .default_handler(|upd: std::sync::Arc<Update>| async move {
        info!("Unhandled update: {:?}", upd);
    })
    .build();

    tokio::select! {
        _ = dispatcher.dispatch() => {
            info!("Bot dispatcher exited naturally");
        }
        _ = shutdown_signal.recv() => {
            info!("Bot received shutdown signal, stopping...");
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
    let command = text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

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
        "/app" => match state.store_service.get_user_by_tg_id(tg_id).await {
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
        },
        "/help" => {
            let help_text = "<b>Commands</b>\n\n/start - initialize profile\n/app - open mini app button\n/plans - list active plans\n/my - my subscriptions\n/help - show this help";
            bot.send_message(msg.chat.id, help_text)
                .parse_mode(ParseMode::Html)
                .await?;
            Ok(())
        }
        "/plans" => {
            let plans = state
                .catalog_service
                .get_active_plans()
                .await
                .unwrap_or_default();

            if plans.is_empty() {
                bot.send_message(msg.chat.id, "No active plans yet.")
                    .await?;
                return Ok(());
            }

            let mut text = String::from("<b>Available Plans</b>\n\n");
            for plan in plans {
                text.push_str(&format!(
                    "• <b>{}</b> — {} GB, {} devices\n",
                    plan.name,
                    plan.traffic_limit_gb,
                    if plan.device_limit == 0 {
                        "unlimited".to_string()
                    } else {
                        plan.device_limit.to_string()
                    }
                ));
                if plan.durations.is_empty() {
                    text.push_str("  No prices configured\n");
                } else {
                    for d in plan.durations {
                        text.push_str(&format!(
                            "  {} days: ${}.{:02}\n",
                            d.duration_days,
                            d.price / 100,
                            d.price % 100
                        ));
                    }
                }
                text.push('\n');
            }

            bot.send_message(msg.chat.id, text)
                .parse_mode(ParseMode::Html)
                .await?;
            Ok(())
        }
        "/my" => {
            let user = match state.store_service.get_user_by_tg_id(tg_id).await {
                Ok(Some(u)) => u,
                _ => {
                    bot.send_message(msg.chat.id, "Use /start first to initialize profile.")
                        .await?;
                    return Ok(());
                }
            };

            let subs = state
                .subscription_service
                .get_user_subscriptions(user.id)
                .await
                .unwrap_or_default();

            if subs.is_empty() {
                bot.send_message(msg.chat.id, "You have no subscriptions yet.")
                    .await?;
                return Ok(());
            }

            let mut text = String::from("<b>Your Subscriptions</b>\n\n");
            for sub in subs {
                text.push_str(&format!(
                    "• <b>{}</b> — {}\n  expires: {}\n\n",
                    sub.plan_name,
                    sub.sub.status,
                    sub.sub.expires_at.format("%Y-%m-%d %H:%M UTC")
                ));
            }

            bot.send_message(msg.chat.id, text)
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
    fn with_cache_bust(mut url: String) -> String {
        let version = crate::utils::current_panel_version();
        let encoded_version = urlencoding::encode(&version);
        if url.contains('?') {
            url.push_str("&v=");
            url.push_str(&encoded_version);
        } else {
            url.push_str("?v=");
            url.push_str(&encoded_version);
        }
        url
    }

    let explicit = state.settings.get_or_default("mini_app_url", "").await;
    if !explicit.trim().is_empty() {
        return Some(with_cache_bust(explicit.trim().to_string()));
    }

    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    if sub_domain.trim().is_empty() {
        return None;
    }

    Some(with_cache_bust(format!(
        "https://{}/app",
        sub_domain.trim()
    )))
}

fn terms_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Accept",
        "accept_terms",
    )]])
}
