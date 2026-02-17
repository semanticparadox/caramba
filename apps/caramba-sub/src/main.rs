use axum::{
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod handlers;
mod panel_client;
mod geo_service;
mod singbox_generator;

use config::FrontendConfig;
use geo_service::GeoService; // Added
use std::sync::Arc; // Added

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
    pub geo_service: Arc<GeoService>, // Added
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
        }
    }
}
