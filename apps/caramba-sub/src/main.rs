use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use caramba_shared::self_update::{apply_self_update, restart_service};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod geo_service;
mod handlers;
mod panel_client;

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
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Frontend shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install SIGINT handler");
    };
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! {
        _ = ctrl_c => tracing::info!("SIGINT received — initiating graceful shutdown"),
        _ = terminate => tracing::info!("SIGTERM received — initiating graceful shutdown"),
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: FrontendConfig,
    pub panel_client: panel_client::PanelClient,
    pub geo_service: Arc<GeoService>,
    pub metrics: Arc<FrontendMetrics>,
    /// Redis клиент для кеширования конфигов подписок (опциональный — сервис работает без него)
    pub redis_client: Option<redis::Client>,
}

impl AppState {
    fn new(config: FrontendConfig, geo_service: Arc<GeoService>) -> Self {
        let panel_client =
            panel_client::PanelClient::new(config.panel_url.clone(), config.auth_token.clone());

        // Инициализируем Redis клиент из переменной окружения REDIS_URL (опционально)
        let redis_client =
            std::env::var("REDIS_URL").ok().and_then(|url| {
                match redis::Client::open(url.as_str()) {
                    Ok(client) => {
                        tracing::info!("Redis cache enabled for subscription configs");
                        Some(client)
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to initialize Redis client (caching disabled): {}",
                            e
                        );
                        None
                    }
                }
            });

        Self {
            config,
            panel_client,
            geo_service,
            metrics: Arc::new(FrontendMetrics::default()),
            redis_client,
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

            match apply_self_update(&asset_url, payload.sha256.as_deref(), "caramba-sub").await {
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

                    restart_service("caramba-sub.service");
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
