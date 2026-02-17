use dotenvy::dotenv;
use teloxide::prelude::*;
use std::env;

mod api_client;
mod state;
mod services;
mod bot;
pub mod models;

use crate::state::AppState;
use crate::api_client::ApiClient;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();

    log::info!("Starting Caramba Bot...");

    let token = env::var("BOT_TOKEN").expect("BOT_TOKEN is not set");
    let panel_url = env::var("PANEL_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    // We might need a special token for the bot to authenticate with the panel API if it's protected
    // For now assuming Panel API layout allows bot interaction or we add a shared secret
    let panel_token = env::var("PANEL_TOKEN").unwrap_or_default(); 

    let api_client = ApiClient::new(panel_url, panel_token);
    
    let settings = crate::services::settings_service::SettingsService::new(api_client.clone());
    let admin_service = crate::services::admin_service::AdminService::new(api_client.clone());

    let store_service = crate::services::store_service::StoreService::new(api_client.clone());
    let promo_service = crate::services::promo_service::PromoService::new(api_client.clone());
    let pay_service = crate::services::pay_service::PayService::new(api_client.clone());
    let logging_service = crate::services::logging_service::LoggingService::new(api_client.clone());

    let state = AppState {
        api: api_client,
        settings,
        store_service,
        promo_service,
        pay_service,
        admin_service,
        logging_service,
    };

    let bot = Bot::new(token);
    
    // Create a dummy shutdown signal for now
    let (_tx, rx) = tokio::sync::broadcast::channel(1);

    bot::run_bot(bot, rx, state).await;
}
