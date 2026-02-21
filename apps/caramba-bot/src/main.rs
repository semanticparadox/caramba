use dotenvy::dotenv;
use sha2::{Digest, Sha256};
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::Duration;
use teloxide::prelude::*;

mod api_client;
mod bot;
pub mod models;
mod services;
mod state;

use crate::api_client::ApiClient;
use crate::state::AppState;

fn local_bot_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn local_bot_worker_id() -> String {
    let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "unknown-host".to_string());
    format!("bot:{}", hostname)
}

fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    let expected = expected_hex.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = format!("{:x}", hasher.finalize());
    actual == expected
}

async fn apply_self_update(asset_url: &str, expected_sha256: Option<&str>) -> anyhow::Result<()> {
    let response = reqwest::Client::new()
        .get(asset_url)
        .send()
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;

    if let Some(hash) = expected_sha256 {
        if !hash.trim().is_empty() && !verify_sha256(&bytes, hash) {
            return Err(anyhow::anyhow!("SHA256 mismatch for downloaded binary"));
        }
    }

    let exe_path = std::env::current_exe()?;
    let exe_parent = exe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Failed to detect executable parent directory"))?;
    let tmp_path = exe_parent.join(format!(
        ".caramba-bot.update.{}.tmp",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    ));

    tokio::fs::write(&tmp_path, &bytes).await?;
    std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&tmp_path, &exe_path)?;
    Ok(())
}

fn restart_bot_service() {
    match Command::new("systemctl")
        .args(["restart", "caramba-bot.service"])
        .status()
    {
        Ok(status) if status.success() => {
            log::info!("caramba-bot.service restart requested after self-update.");
        }
        Ok(status) => {
            log::error!(
                "Failed to restart caramba-bot.service (status: {}). Manual restart required.",
                status
            );
        }
        Err(e) => {
            log::error!(
                "Failed to execute systemctl restart for caramba-bot.service: {}",
                e
            );
        }
    }
}

fn start_worker_update_loop(api_client: ApiClient) {
    tokio::spawn(async move {
        let worker_id = local_bot_worker_id();
        let current_version = local_bot_version();
        let mut tick = tokio::time::interval(Duration::from_secs(120));

        loop {
            tick.tick().await;

            let payload = match api_client
                .poll_worker_update("bot", &worker_id, &current_version)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    log::debug!("Bot worker update poll failed: {}", e);
                    continue;
                }
            };

            if !payload.update {
                continue;
            }

            let target_version = payload.target_version.unwrap_or_default();
            let asset_url = payload.asset_url.unwrap_or_default();
            if target_version.trim().is_empty() || asset_url.trim().is_empty() {
                log::warn!("Worker update payload is incomplete; skipping.");
                continue;
            }

            let _ = api_client
                .report_worker_update(
                    "bot",
                    &crate::api_client::WorkerUpdateReportRequest {
                        worker_id: worker_id.clone(),
                        current_version: current_version.clone(),
                        target_version: target_version.clone(),
                        status: "started".to_string(),
                        message: Some("Downloading update asset".to_string()),
                    },
                )
                .await;

            match apply_self_update(&asset_url, payload.sha256.as_deref()).await {
                Ok(_) => {
                    let _ = api_client
                        .report_worker_update(
                            "bot",
                            &crate::api_client::WorkerUpdateReportRequest {
                                worker_id: worker_id.clone(),
                                current_version: current_version.clone(),
                                target_version: target_version.clone(),
                                status: "success".to_string(),
                                message: Some(
                                    "Update binary applied. Restarting service.".to_string(),
                                ),
                            },
                        )
                        .await;
                    restart_bot_service();
                    return;
                }
                Err(e) => {
                    log::error!("Bot worker self-update failed: {}", e);
                    let _ = api_client
                        .report_worker_update(
                            "bot",
                            &crate::api_client::WorkerUpdateReportRequest {
                                worker_id: worker_id.clone(),
                                current_version: current_version.clone(),
                                target_version: target_version.clone(),
                                status: "failed".to_string(),
                                message: Some(e.to_string()),
                            },
                        )
                        .await;
                }
            }
        }
    });
}

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
    if api_client.has_token() {
        start_worker_update_loop(api_client.clone());
    } else {
        log::info!("PANEL_TOKEN not set. Bot worker rollout polling is disabled.");
    }

    let settings = crate::services::settings_service::SettingsService::new(api_client.clone());

    let store_service = crate::services::store_service::StoreService::new(api_client.clone());
    let promo_service = crate::services::promo_service::PromoService::new(api_client.clone());
    let pay_service = crate::services::pay_service::PayService::new(api_client.clone());
    let logging_service = crate::services::logging_service::LoggingService::new(api_client.clone());

    let state = AppState {
        settings,
        store_service,
        promo_service,
        pay_service,
        logging_service,
    };

    let bot = Bot::new(token);

    // Create a dummy shutdown signal for now
    let (_tx, rx) = tokio::sync::broadcast::channel(1);

    bot::run_bot(bot, rx, state).await;
}
