mod db;
mod settings;
mod bot;
mod bot_manager;
mod scripts;
mod singbox;
mod cli;
mod models;
mod services;
mod api;
mod subscription;
mod utils;
mod repositories;
pub mod handlers;




use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;
use std::io;
use tracing_appender;
use db::init_db;
use settings::SettingsService;
use bot_manager::BotManager;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use anyhow::Result;
use clap::{Parser, Subcommand};
use axum::{
    routing::{get, post},
    response::IntoResponse,
    extract::State,
};
use axum_extra::extract::cookie::CookieJar;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub settings: Arc<SettingsService>,
    pub bot_manager: Arc<BotManager>,

    pub store_service: Arc<services::store_service::StoreService>,
    pub orchestration_service: Arc<services::orchestration_service::OrchestrationService>,
    pub pay_service: Arc<services::pay_service::PayService>,
    pub export_service: Arc<services::export_service::ExportService>,
    pub notification_service: Arc<services::notification_service::NotificationService>,
    pub connection_service: Arc<services::connection_service::ConnectionService>,
    pub redis: Arc<services::redis_service::RedisService>,
    pub pubsub: Arc<services::pubsub_service::PubSubService>,

    // Enterprise Modular Services
    pub user_service: Arc<services::user_service::UserService>,
    pub billing_service: Arc<services::billing_service::BillingService>,
    pub subscription_service: Arc<services::subscription_service::SubscriptionService>,
    pub catalog_service: Arc<services::catalog_service::CatalogService>,
    pub analytics_service: Arc<services::analytics_service::AnalyticsService>,
    pub generator_service: Arc<services::generator_service::GeneratorService>, // Phase 1.8
    pub org_service: Arc<services::org_service::OrganizationService>, // Phase 3
    pub sni_repo: Arc<repositories::sni_repo::SniRepository>,
    pub telemetry_service: Arc<services::telemetry_service::TelemetryService>, 
    pub infrastructure_service: Arc<services::infrastructure_service::InfrastructureService>,
    pub security_service: Arc<services::security_service::SecurityService>,
    pub promo_service: Arc<services::promo_service::PromoService>,


    pub ssh_public_key: String,
    // Format: IP -> (Lat, Lon, Timestamp)
    pub geo_cache: Arc<Mutex<HashMap<String, (f64, f64, Instant)>>>,
    pub session_secret: String,
    pub admin_path: String,
    pub system_stats: Arc<tokio::sync::Mutex<sysinfo::System>>,
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
    State(state): State<AppState>,
    jar: CookieJar,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let path = req.uri().path();
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    let login_path = format!("{}/login", admin_path);
    let setup_path = format!("{}/setup", admin_path);

    // Allow static assets, login, and setup paths
    if path == login_path || path.starts_with(&setup_path) || path.starts_with("/assets") {
        return next.run(req).await;
    }

    if let Some(cookie) = jar.get("admin_session") {
        let token = cookie.value();
        let redis_key = format!("session:{}", token);
        // Check if token exists in Redis
        if let Ok(Some(username)) = state.redis.get(&redis_key).await {
            // Verify this username actually exists in the DB (prevents ghost sessions after reinstall)
            let user_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admins WHERE username = ?)")
                .bind(&username)
                .fetch_one(&state.pool)
                .await
                .unwrap_or(false);

            if user_exists {
                return next.run(req).await;
            } else {
                tracing::warn!("Session INVALID: Redis has username '{}' but DB check failed. (Ghost session?)", username);
                // Force cache clear on client side if possible, but mainly we just reject access here.
                // NOTE: Falling through will redirect to Login.
            }
        }
    }

    // Check if any admin exists
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admins")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    if admin_count == 0 {
        return axum::response::Redirect::to(&setup_path).into_response();
    }

    axum::response::Redirect::to(&login_path).into_response()
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("ExaRobot binary started. Version: {}", env!("CARGO_PKG_VERSION"));
    // Load .env
    if let Err(e) = dotenvy::dotenv() {
        // Only warn if we are not in a test/dev environment where it might be intentional
        println!("⚠️  Warning: Failed to load .env file: {}", e);
    }

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
    println!("Database initialized successfully.");

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
                    println!("Redis URL:  {}", std::env::var("REDIS_URL").unwrap_or("redis://127.0.0.1:6379".to_string()));
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
    
    // Initialize bot manager
    let bot_manager = Arc::new(BotManager::new());

    // Initialize Redis
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    // Check if redis_url actually starts with redis://, if not, assume it's just host:port or similar and prefix, or default
    // Basic fallback for robust dev env
    let redis_service = match services::redis_service::RedisService::new(&redis_url).await {
        Ok(r) => Arc::new(r),
        Err(e) => {
             // Fallback to internal/mock if real redis fails? For now failure is fatal as planned.
             tracing::error!("Redis connection failed: {}. Ensure Redis is running.", e);
             return Err(e);
        }
    };

    // Initialize PubSub Service (Moved up for dependency injection)
    let pubsub_service = services::pubsub_service::PubSubService::new(redis_url).await.expect("Failed to init PubSub");

    // Initialize store service
    let store_service = Arc::new(services::store_service::StoreService::new(pool.clone()));

    // Initialize infrastructure & security services (Moved up for dependency injection)
    let infrastructure_service = Arc::new(services::infrastructure_service::InfrastructureService::new(pool.clone()));
    let security_service = Arc::new(services::security_service::SecurityService::new(pool.clone()));

    // Initialize orchestration service
    let orchestration_service = Arc::new(services::orchestration_service::OrchestrationService::new(
        pool.clone(),
        store_service.clone(),
        security_service.clone(),
        pubsub_service.clone(),
    ));

    // Initialize new modular services
    let user_service = Arc::new(services::user_service::UserService::new(pool.clone()));
    let billing_service = Arc::new(services::billing_service::BillingService::new(pool.clone()));
    let subscription_service = Arc::new(services::subscription_service::SubscriptionService::new(pool.clone()));
    let catalog_service = Arc::new(services::catalog_service::CatalogService::new(pool.clone()));
    let generator_service = Arc::new(services::generator_service::GeneratorService::new(
        pool.clone(),
        security_service.clone(),
        orchestration_service.clone(),
        pubsub_service.clone(),
    )); // Phase 1.8
    let org_repo = repositories::org_repo::OrganizationRepository::new(pool.clone());
    let org_service = Arc::new(services::org_service::OrganizationService::new(org_repo));
    let sni_repo = Arc::new(repositories::sni_repo::SniRepository::new(pool.clone()));

    // Initialize Update Service (Phase 66)
    let update_service = Arc::new(services::update_service::UpdateService::new(settings.clone()));
    // Run update check in background on startup
    let update_svc_clone = update_service.clone();
    tokio::spawn(async move {
        update_svc_clone.initialize_agent_updates().await;
    });

    // Initialize connection service
    let connection_service = Arc::new(services::connection_service::ConnectionService::new(
        pool.clone(),
        orchestration_service.clone(),
        store_service.clone(),
    ));

    let bot_token = settings.get_or_default("bot_token", "").await;
    let pay_token = settings.get_or_default("payment_api_key", "").await;
    let nowpayments_key = settings.get_or_default("nowpayments_key", "").await;
    let crystalpay_login = settings.get_or_default("crystalpay_login", "").await;
    let crystalpay_secret = settings.get_or_default("crystalpay_secret", "").await;

    let stripe_secret_key = settings.get_or_default("stripe_secret_key", "").await;
    let cryptomus_merchant_id = settings.get_or_default("cryptomus_merchant_id", "").await;
    let cryptomus_payment_api_key = settings.get_or_default("cryptomus_payment_api_key", "").await;
    let aaio_merchant_id = settings.get_or_default("aaio_merchant_id", "").await;
    let aaio_secret_1 = settings.get_or_default("aaio_secret_1", "").await;
    let aaio_secret_2 = settings.get_or_default("aaio_secret_2", "").await;
    
    let lava_project_id = settings.get_or_default("lava_project_id", "").await;
    let lava_secret_key = settings.get_or_default("lava_secret_key", "").await;

    let is_testnet: String = settings.get_or_default("payment_testnet", "true").await;
    
    let pay_service = Arc::new(services::pay_service::PayService::new(
        pool.clone(),
        store_service.clone(),
        bot_manager.clone(),
        bot_token,
        pay_token,
        nowpayments_key,
        crystalpay_login,
        crystalpay_secret,
        stripe_secret_key,
        cryptomus_merchant_id,
        cryptomus_payment_api_key,
        aaio_merchant_id,
        aaio_secret_1,
        aaio_secret_2,
        lava_project_id,
        lava_secret_key,
        is_testnet == "true",
    ));

    let export_service = Arc::new(services::export_service::ExportService::new(pool.clone()));
    let notification_service = Arc::new(services::notification_service::NotificationService::new(pool.clone()));
    
    // Telemetry Service (Phase 3) - Depends on Security, Notification, BotManager
    let telemetry_service = Arc::new(services::telemetry_service::TelemetryService::new(
        pool.clone(),
        security_service.clone(),
        notification_service.clone(),
        bot_manager.clone(),
    ));



    let admin_path_prefix = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path_prefix = if admin_path_prefix.starts_with('/') { admin_path_prefix } else { format!("/{}", admin_path_prefix) };

    // Initialize System Monitor
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let system_stats = std::sync::Arc::new(tokio::sync::Mutex::new(sys));

    let analytics_service = Arc::new(services::analytics_service::AnalyticsService::new(pool.clone()));

    let geo_cache = Arc::new(Mutex::new(HashMap::new()));
    let session_secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "secret".to_string());

    let promo_service = Arc::new(services::promo_service::PromoService::new(pool.clone()));

    // App state
    let state = AppState {
        pool: pool.clone(),
        settings: settings.clone(),
        bot_manager: bot_manager.clone(),
        store_service: store_service.clone(),
        orchestration_service: orchestration_service.clone(),
        pay_service: pay_service.clone(),
        export_service: export_service.clone(),
        notification_service: notification_service.clone(),
        connection_service: connection_service.clone(),
        redis: redis_service.clone(),
        pubsub: pubsub_service.clone(),
        
        user_service,
        billing_service,
        subscription_service,
        catalog_service,
        analytics_service,
        generator_service,
        org_service,
        sni_repo,
        telemetry_service,
        infrastructure_service,
        security_service,
        promo_service,
        
        ssh_public_key,
        geo_cache,
        session_secret,
        admin_path: admin_path_prefix.clone(),
        system_stats,
    };
    
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
    
    // Start Inbound Rotation Scheduler (Phase 5)
    let rotation_state = state.clone();
    let rotation_generator = state.generator_service.clone();
    tokio::spawn(async move {
        let rotation_svc = services::rotation_service::RotationService::new(
            rotation_state.pool.clone(),
            rotation_generator,
        );
        rotation_svc.start().await;
    });

    // Start SNI Health Monitor (Phase 5)
    let sni_monitor_state = state.clone();
    let sni_monitor_repo = (*state.sni_repo).clone();
    tokio::spawn(async move {
        let sni_monitor = services::sni_monitor::SniMonitor::new(
            sni_monitor_state.pool.clone(),
            sni_monitor_repo,
        );
        sni_monitor.start().await;
    });


use tower_http::services::ServeDir;

    // Routes
    let admin_routes = axum::Router::new()
        .nest_service("/assets", ServeDir::new("apps/panel/assets"))
        .route("/dashboard", axum::routing::get(handlers::admin::get_dashboard))
        .route("/settings", axum::routing::get(handlers::admin::get_settings))
        .route("/settings/save", axum::routing::post(handlers::admin::save_settings))
        .route("/settings/bot/toggle", axum::routing::post(handlers::admin::toggle_bot))
        .route("/settings/update/check", axum::routing::post(handlers::admin::check_update)) // NEW
        // New Bot Page
        .route("/bot", axum::routing::get(handlers::admin::bot_logs_page))
        // Tools Logic (Page removed, actions preserved)
        // .route("/tools", axum::routing::get(handlers::admin::get_tools_page)) // Removed
        .route("/tools/export", axum::routing::get(handlers::admin::export_database))
        .route("/tools/trial-config", axum::routing::post(handlers::admin::update_trial_config))
        // .route("/traffic", axum::routing::get(handlers::admin::get_traffic_analytics)) // Merged into /analytics
        .route("/logs", axum::routing::get(handlers::admin::get_system_logs_page)) // NEW
        .route("/nodes", axum::routing::get(handlers::admin::get_nodes))
        .route("/nodes/{id}/manage", axum::routing::get(handlers::admin::get_node_manage)) // NEW Unified UI
        .route("/nodes/install", axum::routing::post(handlers::admin::install_node))
        .route("/nodes/{id}/edit", axum::routing::get(handlers::admin::get_node_edit))
        .route("/nodes/{id}/update", axum::routing::post(handlers::admin::update_node))
        .route("/nodes/{id}/update/trigger", axum::routing::post(handlers::admin::updates::trigger_update)) // NEW Phase 67
        .route("/nodes/{id}/activate", axum::routing::post(handlers::admin::activate_node))
        .route("/nodes/{id}/config/preview", axum::routing::get(handlers::admin_network::preview_node_config))
        .route("/nodes/{id}/sync", axum::routing::post(handlers::admin::sync_node))
        .route("/nodes/{id}/logs", axum::routing::get(handlers::admin::get_node_logs))
        .route("/nodes/{id}/rescue", axum::routing::get(handlers::admin::get_node_rescue))
        // SSH-based Node Control removed - use Agent API endpoints instead
        .route("/nodes/{id}/delete", axum::routing::delete(handlers::admin::delete_node))
        .route("/nodes/{id}/toggle", axum::routing::post(handlers::admin::toggle_node_enable))
        .route("/nodes/{id}/snis/{sni_id}/pin", axum::routing::post(handlers::admin::nodes::pin_sni))
        .route("/nodes/{id}/snis/{sni_id}/unpin", axum::routing::post(handlers::admin::nodes::unpin_sni))
        .route("/nodes/{id}/snis/{sni_id}/block", axum::routing::post(handlers::admin::nodes::block_sni))
        .route("/nodes/{id}/inbounds", axum::routing::get(handlers::admin_network::get_node_inbounds).post(handlers::admin_network::add_inbound))
        .route("/nodes/{id}/inbounds/{inbound_id}", axum::routing::get(handlers::admin_network::get_edit_inbound).post(handlers::admin_network::update_inbound).delete(handlers::admin_network::delete_inbound))
        .route("/nodes/{id}/inbounds/{inbound_id}/toggle", axum::routing::post(handlers::admin_network::toggle_inbound))
        .route("/plans", axum::routing::get(handlers::admin::get_plans))
        .route("/plans/add", axum::routing::post(handlers::admin::add_plan))
        .route("/plans/{id}", axum::routing::get(handlers::admin::get_plan_edit).post(handlers::admin::update_plan).delete(handlers::admin::delete_plan))
        .route("/plans/{id}/bindings", axum::routing::get(handlers::admin_network::get_plan_bindings).post(handlers::admin_network::save_plan_bindings))
        .route("/users", get(handlers::admin::get_users))
        .route("/users/{id}", get(handlers::admin::get_user_details))
        .route("/users/{id}/balance", post(handlers::admin::update_user_balance))
        .route("/users/{id}/update", post(handlers::admin::update_user))
        .route("/users/{id}/gift", post(handlers::admin::admin_gift_subscription))
        .route("/users/subs/{id}", axum::routing::delete(handlers::admin::delete_user_subscription))
        .route("/users/subs/{id}/refund", axum::routing::post(handlers::admin::refund_user_subscription))
        .route("/users/subs/{id}/extend", axum::routing::post(handlers::admin::extend_user_subscription))
        .route("/subs/{id}/devices", axum::routing::get(handlers::admin::get_subscription_devices))
        .route("/subs/{id}/devices/kill", axum::routing::post(handlers::admin::admin_kill_subscription_sessions))
        .route("/analytics", axum::routing::get(handlers::admin::get_traffic_analytics))
        .route("/promo", axum::routing::get(handlers::admin::get_promos))
        .route("/promo/add", axum::routing::post(handlers::admin::add_promo))
        .route("/promo/{id}/delete", axum::routing::delete(handlers::admin::delete_promo))
        
        // Frontend Servers (Page)
        .route("/frontends", axum::routing::get(handlers::admin::get_frontends))
        
        // Client API (Mini App) - served via .nest("/api/client", ...) below
        
        .route("/transactions", axum::routing::get(handlers::admin::get_transactions))
        .route("/bot-logs", axum::routing::get(handlers::admin::bot_logs_page))
        .route("/bot-logs/history", axum::routing::get(handlers::admin::bot_logs_history))
        .route("/bot-logs/tail", axum::routing::get(handlers::admin::bot_logs_tail))
        .route("/api-keys", axum::routing::get(handlers::admin::list_api_keys).post(handlers::admin::create_api_key))
        .route("/api-keys/delete/{id}", axum::routing::post(handlers::admin::delete_api_key))
        
        // SNI Pool Management
        .route("/sni", axum::routing::get(handlers::admin_sni::get_sni_page))
        .route("/sni/add", axum::routing::post(handlers::admin_sni::add_sni))
        .route("/sni/bulk", axum::routing::post(handlers::admin_sni::bulk_add_sni))
        .route("/sni/delete/{id}", axum::routing::delete(handlers::admin_sni::delete_sni))
        .route("/sni/toggle/{id}", axum::routing::post(handlers::admin_sni::toggle_sni))
        .route("/partials/statusbar", axum::routing::get(handlers::admin::get_statusbar)) // NEW
        .route("/logout", axum::routing::post(handlers::admin::logout))
        
        // Store Management Routes
        .route("/store/categories", axum::routing::get(handlers::admin::get_store_categories_page).post(handlers::admin::create_category))
        .route("/store/categories/{id}", axum::routing::delete(handlers::admin::delete_category))
        .route("/store/products", axum::routing::get(handlers::admin::get_store_products_page).post(handlers::admin::create_product))
        .route("/store/products/{id}", axum::routing::delete(handlers::admin::delete_product))
        
        // Groups Management (Phase 1.8)
        .route("/groups", axum::routing::get(handlers::admin_groups::get_groups_page).post(handlers::admin_groups::create_group))
        .route("/groups/{id}", axum::routing::get(handlers::admin_groups::get_group_edit).delete(handlers::admin_groups::delete_group))
        .route("/groups/{id}/members", axum::routing::post(handlers::admin_groups::add_group_member))
        .route("/groups/{id}/members/{node_id}", axum::routing::delete(handlers::admin_groups::remove_group_member))
        .route("/groups/{id}/rotate", axum::routing::post(handlers::admin_groups::rotate_group_inbounds))
        
        // Templates Management
        .route("/templates", axum::routing::get(handlers::admin_templates::get_templates_page).post(handlers::admin_templates::create_template))
        .route("/templates/{id}", axum::routing::delete(handlers::admin_templates::delete_template).post(handlers::admin_templates::update_template))
        .route("/templates/{id}/edit", axum::routing::get(handlers::admin_templates::get_template_edit))
        .route("/templates/{id}/sync", axum::routing::post(handlers::admin_templates::sync_template))
        
        // Organization Management (Phase 3)
        .route("/orgs", axum::routing::get(handlers::admin_orgs::get_organizations).post(handlers::admin_orgs::create_organization))
        
        // .route("/store/orders", axum::routing::get(handlers::admin_store::orders_page)) // Handled by analytics/dashboard now
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));

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
        // Serve Downloads (for frontend binaries)
        .nest_service("/downloads", ServeDir::new("apps/panel/downloads"))
        // Serve Assets (Public)
        .nest_service("/assets", ServeDir::new("apps/panel/assets"))
        // Setup Routes
        .route(&format!("{}/setup", admin_path), axum::routing::get(handlers::setup::get_setup))
        .route(&format!("{}/setup/create_admin", admin_path), axum::routing::post(handlers::setup::create_admin))
        .route(&format!("{}/setup/restore_backup", admin_path), axum::routing::post(handlers::setup::restore_backup))
        .route("/api/payments/{source}", axum::routing::post(handlers::admin::handle_payment))
        // Family API
        .route("/api/family/invite", axum::routing::post(handlers::api::family::generate_invite))
        .route("/api/family/join", axum::routing::post(handlers::api::family::redeem_invite))
        // Agent V2 API
        .route("/api/v2/node/heartbeat", axum::routing::post(api::v2::node::heartbeat))
        .route("/api/v2/node/config", axum::routing::get(api::v2::node::get_config))
        .route("/api/v2/node/rotate-sni", axum::routing::post(api::v2::node::rotate_sni))
        .route("/api/v2/node/update-info", axum::routing::get(api::v2::node::get_update_info))
        .route("/api/v2/node/updates/poll", axum::routing::get(api::v2::node::poll_updates)) // NEW
        .route("/api/v2/node/logs", axum::routing::post(api::v2::node::report_node_logs)) // NEW
        .route("/api/v2/node/settings", axum::routing::get(api::v2::node::get_settings)) // NEW
        .route("/api/v2/node/register", axum::routing::post(api::v2::node::register)) // NEW Enrollment
        .route("/api/v2/client/recommended", axum::routing::get(api::v2::client::get_recommended_nodes)) // AI Routing
        // Client API
        .nest("/api/client", api::client::routes(state.clone()))
        // Public Subscription URL endpoint
        .route("/sub/{uuid}", axum::routing::get(subscription::subscription_handler))
        
        // Local Mini App Serving
        .route("/app", axum::routing::get(handlers::local_app::serve_app))
        .route("/app/{*path}", axum::routing::get(handlers::local_app::serve_app_assets))

        // Frontend API Routes (Must be top level to match /api/admin/frontends)
        .route("/api/admin/frontends", axum::routing::get(handlers::frontend::list_frontends).post(handlers::frontend::create_frontend))
        .route("/api/admin/frontends/by-region/{region}", axum::routing::get(handlers::frontend::get_active_frontends))
        .route("/api/admin/frontends/{id}", axum::routing::delete(handlers::frontend::delete_frontend))
        .route("/api/admin/frontends/{id}/rotate-token", axum::routing::post(handlers::frontend::rotate_token))
        .route("/api/admin/frontends/{domain}/heartbeat", axum::routing::post(handlers::frontend::frontend_heartbeat))

        .nest(&admin_path, admin_routes)
        .route("/install.sh", axum::routing::get(handlers::admin::get_install_sh))
        .route("/nodes/{id}/script", axum::routing::get(handlers::admin::get_node_install_script))
        .route("/nodes/{id}/raw-install", axum::routing::get(handlers::admin::get_node_raw_install_script))
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
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

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
