use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod geo_service;
mod handlers;
mod panel_client;
mod singbox_generator;

use config::FrontendConfig;
use geo_service::GeoService;
use std::sync::Arc;

fn init_rustls_provider() {
    // rustls 0.23 may require explicit process-level provider selection.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

#[derive(Default)]
pub struct FrontendMetrics {
    requests_count: AtomicU64,
    bandwidth_used: AtomicU64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_rustls_provider();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "caramba_frontend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load config
    let config = FrontendConfig::load()?;
    tracing::info!("Frontend module starting...");
    tracing::info!("Domain: {}", config.domain);
    tracing::info!("Region: {}", config.region);
    tracing::info!("Panel URL: {}", config.panel_url);
    if let Some(path) = &config.geoip_db_path {
        tracing::info!("GeoIP DB: {}", path);
    }

    // Initialize GeoService
    let geo_service = Arc::new(GeoService::new(config.geoip_db_path.as_deref()));

    // Create shared state
    let state = AppState::new(config.clone(), geo_service);
    start_heartbeat_loop(state.clone());
    start_worker_update_loop(state.clone());

    // Build router
    let app = Router::new()
        // Health check
        .route("/health", get(handlers::health::health_check))
        // Subscription URLs
        .route(
            "/sub/{uuid}",
            get(handlers::subscription::subscription_handler),
        )
        // Mini App (static files)
        .route("/app", get(handlers::app::serve_app))
        .route("/app/{*path}", get(handlers::app::serve_app_assets))
        // API proxy to main panel
        .route(
            "/api/{*path}",
            axum::routing::any(handlers::proxy::proxy_handler),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            metrics_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.listen_port));
    tracing::info!("Frontend listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone)]
pub struct AppState {
    pub config: FrontendConfig,
    pub panel_client: panel_client::PanelClient,
    pub geo_service: Arc<GeoService>,
    pub metrics: Arc<FrontendMetrics>,
}

impl AppState {
    fn new(config: FrontendConfig, geo_service: Arc<GeoService>) -> Self {
        let panel_client =
            panel_client::PanelClient::new(config.panel_url.clone(), config.auth_token.clone());

        Self {
            config,
            panel_client,
            geo_service,
            metrics: Arc::new(FrontendMetrics::default()),
        }
    }
}

async fn metrics_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let response = next.run(req).await;
    state.metrics.requests_count.fetch_add(1, Ordering::Relaxed);

    let content_len = response
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    if content_len > 0 {
        state
            .metrics
            .bandwidth_used
            .fetch_add(content_len, Ordering::Relaxed);
    }

    response
}

fn start_heartbeat_loop(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;

            let requests = state.metrics.requests_count.swap(0, Ordering::Relaxed);
            let bandwidth = state.metrics.bandwidth_used.swap(0, Ordering::Relaxed);

            if requests == 0 && bandwidth == 0 {
                continue;
            }

            let stats = panel_client::FrontendStats {
                requests_count: requests,
                bandwidth_used: bandwidth,
            };

            if let Err(e) = state
                .panel_client
                .send_heartbeat(&state.config.domain, stats)
                .await
            {
                tracing::warn!("Frontend heartbeat failed: {}", e);
            }
        }
    });
}

fn local_sub_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn local_sub_worker_id(config: &FrontendConfig) -> String {
    if !config.domain.trim().is_empty() {
        return format!("sub:{}", config.domain.trim());
    }
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown-host".to_string());
    format!("sub:{}", hostname)
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
        ".caramba-sub.update.{}.tmp",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    ));

    tokio::fs::write(&tmp_path, &bytes).await?;
    std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&tmp_path, &exe_path)?;
    Ok(())
}

fn restart_sub_service() {
    match Command::new("systemctl")
        .args(["restart", "caramba-sub.service"])
        .status()
    {
        Ok(status) if status.success() => {
            tracing::info!("caramba-sub.service restart requested after self-update.");
        }
        Ok(status) => {
            tracing::error!(
                "Failed to restart caramba-sub.service (status: {}). Manual restart required.",
                status
            );
        }
        Err(e) => {
            tracing::error!(
                "Failed to execute systemctl restart for caramba-sub.service: {}",
                e
            );
        }
    }
}

fn start_worker_update_loop(state: AppState) {
    tokio::spawn(async move {
        let worker_id = local_sub_worker_id(&state.config);
        let current_version = local_sub_version();
        let mut tick = tokio::time::interval(Duration::from_secs(90));

        loop {
            tick.tick().await;

            let poll = state
                .panel_client
                .poll_worker_update("sub", &worker_id, &current_version)
                .await;
            let payload = match poll {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!("Worker update poll failed: {}", e);
                    continue;
                }
            };

            if !payload.update {
                continue;
            }

            let target_version = payload.target_version.unwrap_or_default();
            let asset_url = payload.asset_url.unwrap_or_default();
            if target_version.trim().is_empty() || asset_url.trim().is_empty() {
                tracing::warn!("Worker update payload is incomplete; skipping.");
                continue;
            }

            let _ = state
                .panel_client
                .report_worker_update(
                    "sub",
                    &panel_client::WorkerUpdateReportRequest {
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
                    let _ = state
                        .panel_client
                        .report_worker_update(
                            "sub",
                            &panel_client::WorkerUpdateReportRequest {
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

                    restart_sub_service();
                    return;
                }
                Err(e) => {
                    tracing::error!("Worker self-update failed: {}", e);
                    let _ = state
                        .panel_client
                        .report_worker_update(
                            "sub",
                            &panel_client::WorkerUpdateReportRequest {
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
