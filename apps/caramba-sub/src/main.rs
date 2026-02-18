use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod handlers;
mod panel_client;
mod geo_service;
mod singbox_generator;

use config::FrontendConfig;
use geo_service::GeoService;
use std::sync::Arc;

#[derive(Default)]
pub struct FrontendMetrics {
    requests_count: AtomicU64,
    bandwidth_used: AtomicU64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    // Build router
    let app = Router::new()
        // Health check
        .route("/health", get(handlers::health::health_check))
        
        // Subscription URLs
        .route("/sub/{uuid}", get(handlers::subscription::subscription_handler))
        
        // Mini App (static files)
        .route("/app", get(handlers::app::serve_app))
        .route("/app/{*path}", get(handlers::app::serve_app_assets))
        
        // API proxy to main panel
        .route("/api/{*path}", axum::routing::any(handlers::proxy::proxy_handler))
        .layer(middleware::from_fn_with_state(state.clone(), metrics_middleware))
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
        let panel_client = panel_client::PanelClient::new(
            config.panel_url.clone(),
            config.auth_token.clone(),
        );
        
        Self {
            config,
            panel_client,
            geo_service,
            metrics: Arc::new(FrontendMetrics::default()),
        }
    }
}

async fn metrics_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
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
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
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
