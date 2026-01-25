mod db;
mod settings;
mod handlers;
mod bot;
mod bot_manager;
mod node_manager;
mod ssh;
mod scripts;
mod singbox;
mod cli;
mod models;
mod services;
mod api;

use std::net::SocketAddr;
use std::sync::Arc;
use std::io;
use db::init_db;
use settings::SettingsService;
use bot_manager::BotManager;
use node_manager::NodeManager;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use anyhow::Result;
use clap::{Parser, Subcommand};
use axum::{
    routing::{get, post},
    response::IntoResponse,
};
use axum_extra::extract::cookie::CookieJar;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub settings: Arc<SettingsService>,
    pub bot_manager: Arc<BotManager>,
    pub node_manager: Arc<NodeManager>,
    pub store_service: Arc<services::store_service::StoreService>,
    pub orchestration_service: Arc<services::orchestration_service::OrchestrationService>,
    pub pay_service: Arc<services::pay_service::PayService>,
    pub ssh_public_key: String,
}

#[derive(Parser)]
#[command(name = "exarobot")]
#[command(about = "EXA ROBOT VPN Control Plane CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the web server and bot
    Serve,
    /// Administrative tools
    Admin {
        #[command(subcommand)]
        subcommand: AdminCommands,
    },
    /// Install the panel as a systemd service
    Install,
}

#[derive(Subcommand)]
enum AdminCommands {
    /// Reset an administrator's password
    ResetPassword {
        /// Username of the admin
        username: String,
        /// New password
        new_pass: String,
    },
    /// Show panel connection information
    Info,
}


async fn auth_middleware(
    jar: CookieJar,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let path = req.uri().path();
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    let login_path = format!("{}/login", admin_path);
    
    // Allow login page/handler without auth
    if path == login_path {
        return next.run(req).await;
    }

    if let Some(cookie) = jar.get("admin_session") {
        if cookie.value() == "true" {
            return next.run(req).await;
        }
    }

    axum::response::Redirect::to(&login_path).into_response()
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // Initialize tracing
    let file_appender = tracing_appender::rolling::never(".", "server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "exarobot=debug,axum=info,tower_http=info,sqlx=warn".into()))
        .with(tracing_subscriber::fmt::layer().with_writer(io::stdout))
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    // Initialize database (needed for most commands)
    let pool = init_db().await?;

    match cli.command {
        Commands::Serve => {
            let pub_key = ensure_ssh_keys()?;
            run_server(pool, pub_key).await?;
        }
        Commands::Admin { subcommand } => {
            match subcommand {
                AdminCommands::ResetPassword { username, new_pass } => {
                    cli::reset_password(&pool, &username, &new_pass).await?;
                }
                AdminCommands::Info => {
                    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
                    println!("\n=== EXA ROBOT INFO ===");
                    println!("Admin Path: {}", admin_path);
                    println!("Login URL:  <YOUR_DOMAIN>{}/login", admin_path);
                    println!("======================\n");
                }
            }
        }
        Commands::Install => {
            cli::install_service()?;
        }
    }

    Ok(())
}

async fn run_server(pool: sqlx::SqlitePool, ssh_public_key: String) -> Result<()> {
    // Initialize settings service
    let settings = Arc::new(SettingsService::new(pool.clone()).await?);
    
    // ... rest of run_server ... (No changes here, line 135+ is fine)
    // BUT we need to make sure AppState uses ssh_public_key we passed
    
    // Initialize bot manager
    let bot_manager = Arc::new(BotManager::new());

    // Initialize node manager
    let node_manager = Arc::new(NodeManager::new(pool.clone()));

    // Initialize store service
    let store_service = Arc::new(services::store_service::StoreService::new(pool.clone()));

    // Initialize orchestration service
    let orchestration_service = Arc::new(services::orchestration_service::OrchestrationService::new(
        pool.clone(),
        store_service.clone(),
    ));

    let pay_token = settings.get_or_default("payment_api_key", "").await;
    let nowpayments_key = settings.get_or_default("nowpayments_key", "").await;
    let pay_service = Arc::new(services::pay_service::PayService::new(
        pool.clone(),
        store_service.clone(),
        bot_manager.clone(),
        pay_token,
        nowpayments_key,
        true, // Testnet for now
    ));

    // App state
    let state = AppState {
        pool,
        settings,
        bot_manager,
        node_manager,
        store_service,
        orchestration_service,
        pay_service,
        ssh_public_key,
    };
    
    // ... rest of function ...
    // Note: I will only replace the top partial block first to fix the match arm, then append function.
    // Actually, let's just do the match arm fix first.
    let _ = state; // prevent unused variable warning if rest of function is cut off (it won't be in real file)
    
    // Auto-start bot if enabled in settings
    let bot_token: String = state.settings.get_or_default("bot_token", "").await;
    let bot_status: String = state.settings.get_or_default("bot_status", "stopped").await;
    if !bot_token.is_empty() && bot_status == "running" {
        tracing::info!("Auto-starting bot...");
        let token_clone = bot_token.clone();
        state.bot_manager.start_bot(token_clone, state.clone()).await;
    }

    // Start Monitoring Service
    let monitoring_state = state.clone();
    tokio::spawn(async move {
        let monitor = services::monitoring::MonitoringService::new(monitoring_state);
        monitor.start().await;
    });

    // Start Traffic Service (Phase 1 Enforcement)
    let traffic_state = state.clone();
    tokio::spawn(async move {
        let traffic_svc = services::traffic_service::TrafficService::new(traffic_state);
        traffic_svc.start().await;
    });

    // Start Connection Service (Device Limit Enforcement)
    let connection_state = state.clone();
    let connection_store = state.store_service.clone();
    let connection_orch = state.orchestration_service.clone();
    tokio::spawn(async move {
        let connection_svc = services::connection_service::ConnectionService::new(
            connection_state.pool.clone(),
            connection_orch,
            connection_store,
        );
        connection_svc.start_monitoring().await;
    });


    // Routes
    let admin_routes = axum::Router::new()
        .route("/dashboard", axum::routing::get(handlers::admin::get_dashboard))
        .route("/settings", axum::routing::get(handlers::admin::get_settings))
        .route("/settings/save", axum::routing::post(handlers::admin::save_settings))
        .route("/settings/bot/toggle", axum::routing::post(handlers::admin::toggle_bot))
        .route("/nodes", axum::routing::get(handlers::admin::get_nodes))
        .route("/nodes/install", axum::routing::post(handlers::admin::install_node))
        .route("/nodes/:id/edit", axum::routing::get(handlers::admin::get_node_edit))
        .route("/nodes/:id/update", axum::routing::post(handlers::admin::update_node))
        .route("/nodes/:id/activate", axum::routing::post(handlers::admin::activate_node))
        .route("/nodes/:id/script", axum::routing::get(handlers::admin::get_node_install_script))
        .route("/nodes/:id/raw-install", axum::routing::get(handlers::admin::get_node_raw_install_script))
        .route("/nodes/:id/config/preview", axum::routing::get(handlers::admin_network::preview_node_config))
        .route("/nodes/:id/sync", axum::routing::post(handlers::admin::sync_node))
        // Node Control (Phase 4)
        .route("/nodes/:id/test", axum::routing::post(handlers::node_control::test_node_connection))
        .route("/nodes/:id/restart", axum::routing::post(handlers::node_control::restart_node_service))
        .route("/nodes/:id/logs", axum::routing::get(handlers::node_control::pull_node_logs))
        .route("/nodes/:id/health", axum::routing::get(handlers::node_control::get_node_health))
        .route("/nodes/:id/delete", axum::routing::delete(handlers::admin::delete_node))
        .route("/nodes/:id/toggle", axum::routing::post(handlers::admin::toggle_node_enable))
        .route("/nodes/:id/inbounds", axum::routing::get(handlers::admin_network::get_node_inbounds).post(handlers::admin_network::add_inbound))
        .route("/nodes/:id/inbounds/:inbound_id", axum::routing::get(handlers::admin_network::get_edit_inbound).post(handlers::admin_network::update_inbound).delete(handlers::admin_network::delete_inbound))
        .route("/plans", axum::routing::get(handlers::admin::get_plans))
        .route("/plans/add", axum::routing::post(handlers::admin::add_plan))
        .route("/plans/:id", axum::routing::get(handlers::admin::get_plan_edit).post(handlers::admin::update_plan).delete(handlers::admin::delete_plan))
        .route("/plans/:id/bindings", axum::routing::get(handlers::admin_network::get_plan_bindings).post(handlers::admin_network::save_plan_bindings))
        .route("/users", get(handlers::admin::get_users))
        .route("/users/:id", get(handlers::admin::get_user_details))
        .route("/users/:id/balance", post(handlers::admin::update_user_balance))
        .route("/users/:id/update", post(handlers::admin::update_user))
        .route("/users/:id/gift", post(handlers::admin::admin_gift_subscription))
        .route("/users/subs/:id", axum::routing::delete(handlers::admin::delete_user_subscription))
        .route("/users/subs/:id/refund", axum::routing::post(handlers::admin::refund_user_subscription))
        .route("/users/subs/:id/extend", axum::routing::post(handlers::admin::extend_user_subscription))
        .route("/subs/:id/devices", axum::routing::get(handlers::admin::get_subscription_devices))
        .route("/transactions", axum::routing::get(handlers::admin::get_transactions))
        .route("/bot-logs", axum::routing::get(handlers::admin::bot_logs_page))
        .route("/bot-logs/history", axum::routing::get(handlers::admin::bot_logs_history))
        .route("/bot-logs/tail", axum::routing::get(handlers::admin::bot_logs_tail))
        .route("/logout", axum::routing::post(handlers::admin::logout))
        
        // Store Management Routes
        .route("/store/categories", axum::routing::get(handlers::admin_store::categories_page).post(handlers::admin_store::add_category))
        .route("/store/categories/:id", axum::routing::delete(handlers::admin_store::delete_category))
        .route("/store/products", axum::routing::get(handlers::admin_store::products_page).post(handlers::admin_store::add_product))
        .route("/store/products/:id", axum::routing::delete(handlers::admin_store::delete_product))
        .route("/store/orders", axum::routing::get(handlers::admin_store::orders_page))
        .layer(axum::middleware::from_fn(auth_middleware));

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    tracing::info!("Admin panel available at: {}", admin_path);

    let app = axum::Router::new()
        .route("/", axum::routing::get({
            let path = admin_path.clone(); // Clone for closure
            move || async move { axum::response::Redirect::to(&format!("{}/dashboard", path)) } // Redirect to dashboard
        }))
        .route(&format!("{}/login", admin_path), axum::routing::get(handlers::admin::get_login).post(handlers::admin::login))
        .route("/api/payments/cryptobot", axum::routing::post(handlers::admin::handle_payment))
        // Agent V2 API
        .route("/api/v2/node/heartbeat", axum::routing::post(api::v2::node::heartbeat))
        .route("/api/v2/node/config", axum::routing::get(api::v2::node::get_config))
        .nest(&admin_path, admin_routes)
        .with_state(state)
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10MB limit
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::X_XSS_PROTECTION,
            axum::http::HeaderValue::from_static("1; mode=block"),
        ));

    // Start server
    let port: u16 = std::env::var("PANEL_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PANEL_PORT must be a number");
        
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn ensure_ssh_keys() -> Result<String> {
    let key_path = std::path::Path::new("id_rsa");
    let pub_path = std::path::Path::new("id_rsa.pub");

    if !key_path.exists() || !pub_path.exists() {
        tracing::info!("SSH keys not found. Generating new RSA keypair...");
        // Use ssh-keygen command
        let output = std::process::Command::new("ssh-keygen")
            .arg("-t").arg("rsa")
            .arg("-b").arg("4096")
            .arg("-f").arg("id_rsa")
            .arg("-q") // Quiet
            .arg("-N").arg("") // Empty passphrase
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute ssh-keygen: {}", e))?;

        if !output.status.success() {
             return Err(anyhow::anyhow!("ssh-keygen failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
    }

    let pub_key = std::fs::read_to_string(pub_path)
        .map_err(|e| anyhow::anyhow!("Failed to read public key: {}", e))?;
    
    Ok(pub_key.trim().to_string())
}
