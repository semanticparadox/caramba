use teloxide::{dptree, prelude::*, types::Update};
use tracing::{error, info};

pub mod handlers;
pub mod keyboards;
pub mod utils;

pub async fn run_bot(
    bot: Bot,
    mut shutdown_signal: tokio::sync::broadcast::Receiver<()>,
    state: crate::AppState,
) {
    info!("Starting refined bot dispatcher...");

    // 0. Safety Net for Panics
    let _prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        error!("CRITICAL BOT PANIC: {:?}", info);
    }));

    // 1. Connectivity Check
    info!("Bot identity check...");
    match bot.get_me().await {
        Ok(me) => {
            let username = me.username.clone().unwrap_or("unknown".into());
            info!("Bot connected as: @{}", username);
            // Store bot username in settings for the footer
            let _ = state.settings.set("bot_username", &username).await;
        }
        Err(e) => {
            error!("CRITICAL: Bot failed to connect to Telegram: {}", e);
            // Don't crash immediately, maybe it's a temp network issue?
            // But usually this means invalid token.
            return;
        }
    }
    // Force a log to prove we reached here
    info!("Bot identity check... (verified)");

    let handler = Update::filter_message().endpoint(handlers::command::message_handler);
    let callback_handler =
        Update::filter_callback_query().endpoint(handlers::callback::callback_handler);
    let pre_checkout_handler =
        Update::filter_pre_checkout_query().endpoint(handlers::payment::pre_checkout_handler);

    let mut dispatcher = Dispatcher::builder(
        bot,
        dptree::entry()
            .branch(handler)
            .branch(callback_handler)
            .branch(pre_checkout_handler),
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
