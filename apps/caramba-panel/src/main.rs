mod api;
mod bot;
mod bot_manager;
mod cli;
pub mod handlers;
mod scripts;
mod services;
mod settings;
mod singbox;
mod subscription;
mod utils;

use bot_manager::BotManager;
use caramba_db::{connect as init_db, repositories};
use settings::SettingsService;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_appender;

use anyhow::Result;
use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use clap::{Parser, Subcommand};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn init_jwt_crypto_provider() {
    // jsonwebtoken 10.x requires a process-wide crypto provider when
    // multiple provider features are present in the dependency graph.
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
}

fn init_rustls_provider() {
    // rustls 0.23 may require explicit process-level provider selection when
    // multiple provider features are present transitively.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
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
    pub org_service: Arc<services::org_service::OrganizationService>,          // Phase 3
    pub marketplace_service: Arc<services::marketplace_service::MarketplaceService>,
    pub sni_repo: Arc<repositories::sni_repo::SniRepository>,
    pub telemetry_service: Arc<services::telemetry_service::TelemetryService>,
    pub infrastructure_service: Arc<services::infrastructure_service::InfrastructureService>,
    pub security_service: Arc<services::security_service::SecurityService>,
    pub promo_service: Arc<services::promo_service::PromoService>,
    pub geo_service: Arc<services::geo_service::GeoService>,

    pub ssh_public_key: String,
    // geo_cache moved to GeoService
    pub session_secret: String,
    pub admin_path: String,
    pub system_stats: Arc<tokio::sync::Mutex<sysinfo::System>>,
    pub task_health: Arc<services::task_health::TaskHealthRegistry>,
}

#[derive(Parser)]
#[command(name = "caramba-panel")]
#[command(about = "Caramba VPN Control Plane CLI", long_about = None)]
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
    let admin_path = if admin_path.starts_with('/') {
        admin_path
    } else {
        format!("/{}", admin_path)
    };

    let login_path = format!("{}/login", admin_path);
    let setup_path = format!("{}/setup", admin_path);
    let setup_create_path = format!("{}/setup/create_admin", admin_path);

    // Allow static assets, login, and initial setup paths (but NOT restore_backup)
    if path == login_path
        || path == setup_path
        || path == setup_create_path
        || path.starts_with("/assets")
    {
        return next.run(req).await;
    }

    if let Some(cookie) = jar.get("admin_session") {
        let token = cookie.value();
        let redis_key = format!("session:{}", token);
        // Check if token exists in Redis
        if let Ok(Some(username)) = state.redis.get(&redis_key).await {
            // Verify this username actually exists in the DB (prevents ghost sessions after reinstall)
            let user_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admins WHERE username = $1)")
                    .bind(&username)
                    .fetch_one(&state.pool)
                    .await
                    .unwrap_or(false);

            if user_exists {
                return next.run(req).await;
            } else {
                tracing::warn!(
                    "Session INVALID: Redis has username '{}' but DB check failed. (Ghost session?)",
                    username
                );
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
    println!(
        "Caramba Panel binary started. Version: {}",
        env!("CARGO_PKG_VERSION")
    );

    init_rustls_provider();
    init_jwt_crypto_provider();

    // Load .env
    if let Err(e) = dotenvy::dotenv() {
        // Only warn if we are not in a test/dev environment where it might be intentional
        println!("⚠️  Warning: Failed to load .env file: {}", e);
    }

    let cli = Cli::parse();

    // Initialize tracing
    let file_appender = tracing_appender::rolling::never(".", "server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let bot_file_appender = tracing_appender::rolling::never(".", "bot.log");
    let (bot_non_blocking, _bot_guard) = tracing_appender::non_blocking(bot_file_appender);
    let bot_targets = Targets::new()
        .with_target("caramba_panel::bot", tracing::Level::TRACE)
        .with_target("caramba_panel::bot_manager", tracing::Level::INFO)
        .with_target("teloxide", tracing::Level::INFO);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "caramba=debug,axum=info,tower_http=info,sqlx=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(io::stdout))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(bot_non_blocking)
                .with_ansi(false)
                .with_filter(bot_targets),
        )
        .init();

    // Initialize database (needed for most commands)
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = init_db(&database_url).await?;
    println!("Database initialized successfully.");

    match cli.command {
        Commands::Serve => {
            let pub_key = ensure_ssh_keys()?;
            run_server(pool, pub_key).await?;
        }
        Commands::Admin { subcommand } => match subcommand {
            AdminCommands::ResetPassword { username, new_pass } => {
                cli::reset_password(&pool, &username, &new_pass).await?;
            }
            AdminCommands::Info => {
                let admin_path =
                    std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                let admin_path = if admin_path.starts_with('/') {
                    admin_path
                } else {
                    format!("/{}", admin_path)
                };
                println!("\n=== CARAMBA INFO ===");
                println!("Admin Path: {}", admin_path);
                println!("Login URL:  <YOUR_DOMAIN>{}/login", admin_path);
                println!(
                    "Redis URL:  {}",
                    std::env::var("REDIS_URL").unwrap_or("redis://127.0.0.1:6379".to_string())
                );
                println!("======================\n");
            }
        },
        Commands::Install => {
            cli::install_service()?;
        }
    }

    Ok(())
}

async fn run_server(pool: sqlx::PgPool, ssh_public_key: String) -> Result<()> {
    // Initialize settings service
    let settings = Arc::new(SettingsService::new(pool.clone()).await?);

    // Initialize bot manager
    let bot_manager = Arc::new(BotManager::new());

    // Initialize Redis
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    // Check if redis_url actually starts with redis://, if not, assume it's just host:port or similar and prefix, or default
    // Basic fallback for robust dev env
    let redis_service = {
        let mut last_err = None;
        let mut redis_ok = None;
        for attempt in 1..=5 {
            match services::redis_service::RedisService::new(&redis_url).await {
                Ok(r) => {
                    tracing::info!("Redis connected (attempt {})", attempt);
                    redis_ok = Some(Arc::new(r));
                    break;
                }
                Err(e) => {
                    tracing::warn!("Redis connection attempt {}/5 failed: {}", attempt, e);
                    last_err = Some(e);
                    if attempt < 5 {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
        match redis_ok {
            Some(r) => r,
            None => {
                tracing::error!("Redis connection failed after 5 attempts. Cannot start.");
                return Err(last_err.unwrap());
            }
        }
    };

    // Initialize PubSub Service (Moved up for dependency injection)
    let pubsub_service = {
        let mut last_err_ps = None;
        let mut pubsub_ok = None;
        for attempt in 1..=5 {
            match services::pubsub_service::PubSubService::new(redis_url.clone()).await {
                Ok(ps) => {
                    tracing::info!("PubSub connected (attempt {})", attempt);
                    pubsub_ok = Some(ps);
                    break;
                }
                Err(e) => {
                    tracing::warn!("PubSub connection attempt {}/5 failed: {}", attempt, e);
                    last_err_ps = Some(e);
                    if attempt < 5 {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
        match pubsub_ok {
            Some(ps) => ps,
            None => {
                tracing::error!("PubSub connection failed after 5 attempts. Cannot start.");
                return Err(last_err_ps.unwrap());
            }
        }
    };

    // Initialize store service
    let store_service_raw = services::store_service::StoreService::new(pool.clone());
    let store_service = Arc::new(store_service_raw);

    // Initialize infrastructure & security services
    let infrastructure_service =
        Arc::new(services::infrastructure_service::InfrastructureService::new(pool.clone()));
    let security_service = Arc::new(services::security_service::SecurityService::new(
        pool.clone(),
    ));

    // Initialize orchestration service
    let orchestration_service =
        Arc::new(services::orchestration_service::OrchestrationService::new(
            pool.clone(),
            store_service.clone(),
            security_service.clone(),
            pubsub_service.clone(),
            std::env::var("REDIS_URL").ok(),
        ));

    // Break circular dependency - Inject Orchestrator back into StoreService
    // We need interior mutability on StoreService.
    // See modification in store_service.rs (using RwLock).
    store_service.set_orchestration_service(orchestration_service.clone());

    // Initialize new modular services
    let user_service = Arc::new(services::user_service::UserService::new(pool.clone()));
    let billing_service = Arc::new(services::billing_service::BillingService::new(pool.clone()));
    // Subscription service also needs orchestrator for trigger-based sync!
    let mut sub_svc_raw = services::subscription_service::SubscriptionService::new(pool.clone());
    sub_svc_raw.set_orchestration_service(orchestration_service.clone());
    let subscription_service = Arc::new(sub_svc_raw);
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
    let update_service = Arc::new(services::update_service::UpdateService::new(
        settings.clone(),
    ));
    // Run update check in background on startup
    let update_svc_clone = update_service.clone();
    tokio::spawn(async move {
        update_svc_clone.initialize_agent_updates().await;
    });

    // Initialize connection service
    let connection_service = Arc::new(services::connection_service::ConnectionService::new(
        orchestration_service.clone(),
        store_service.clone(),
        subscription_service.clone(),
    ));

    let bot_token = settings.get_or_default("bot_token", "").await;
    let pay_token = settings.get_or_default("payment_api_key", "").await;
    let nowpayments_key = settings.get_or_default("nowpayments_key", "").await;
    let nowpayments_ipn_secret = settings.get_or_default("nowpayments_ipn_secret", "").await;
    let cryptobot_token = settings.get_or_default("cryptobot_token", "").await;

    let crystalpay_login = settings.get_or_default("crystalpay_login", "").await;
    let crystalpay_secret = settings.get_or_default("crystalpay_secret", "").await;

    let stripe_secret_key = settings.get_or_default("stripe_secret_key", "").await;
    let stripe_webhook_secret = settings.get_or_default("stripe_webhook_secret", "").await;
    let cryptomus_merchant_id = settings.get_or_default("cryptomus_merchant_id", "").await;
    let cryptomus_payment_api_key = settings
        .get_or_default("cryptomus_payment_api_key", "")
        .await;
    let aaio_merchant_id = settings.get_or_default("aaio_merchant_id", "").await;
    let aaio_secret_1 = settings.get_or_default("aaio_secret_1", "").await;
    let aaio_secret_2 = settings.get_or_default("aaio_secret_2", "").await;

    let lava_project_id = settings.get_or_default("lava_project_id", "").await;
    let lava_secret_key = settings.get_or_default("lava_secret_key", "").await;

    // Новые провайдеры (7 штук)
    // Примечание: crystalpay_login/crystalpay_secret уже считаны выше для pay_service.
    let wata_jwt_token = settings.get_or_default("wata_jwt_token", "").await;
    let wata_webhook_secret = settings.get_or_default("wata_webhook_secret", "").await;
    let crystalpay_salt = settings.get_or_default("crystalpay_salt", "").await;
    let tribute_api_key = settings.get_or_default("tribute_api_key", "").await;
    let tribute_webhook_secret = settings.get_or_default("tribute_webhook_secret", "").await;
    let btcpay_url = settings.get_or_default("btcpay_url", "").await;
    let btcpay_api_key = settings.get_or_default("btcpay_api_key", "").await;
    let btcpay_store_id = settings.get_or_default("btcpay_store_id", "").await;
    let btcpay_webhook_secret = settings.get_or_default("btcpay_webhook_secret", "").await;
    let oxapay_merchant_key = settings.get_or_default("oxapay_merchant_key", "").await;
    let coinbase_api_key = settings.get_or_default("coinbase_api_key", "").await;
    let coinbase_webhook_secret = settings.get_or_default("coinbase_webhook_secret", "").await;
    let plisio_api_key = settings.get_or_default("plisio_api_key", "").await;

    let is_testnet: String = settings.get_or_default("payment_testnet", "true").await;
    let panel_url = settings.get_or_default("panel_url", "").await;
    let api_domain = settings
        .get_or_default(
            "api_domain",
            if panel_url.is_empty() {
                "your-panel-domain.com"
            } else {
                panel_url.as_str()
            },
        )
        .await;
    // Читаем имя бота из настроек — сохраняется при первом запуске бота через get_me()
    let marketplace_bot_username = settings.get_or_default("bot_username", "").await;
    let marketplace_api_domain = api_domain.clone();

    let pay_service = Arc::new(services::pay_service::PayService::new(
        pool.clone(),
        store_service.clone(),
        catalog_service.clone(),
        bot_manager.clone(),
        bot_token,
        pay_token,
        nowpayments_key.clone(),
        crystalpay_login.clone(),
        crystalpay_secret.clone(),
        stripe_secret_key.clone(),
        cryptomus_merchant_id.clone(),
        cryptomus_payment_api_key.clone(),
        aaio_merchant_id.clone(),
        aaio_secret_1.clone(),
        aaio_secret_2.clone(),
        lava_project_id.clone(),
        lava_secret_key.clone(),
        is_testnet == "true",
        api_domain,
    ));

    let export_service = Arc::new(services::export_service::ExportService::new());

    let marketplace_service = Arc::new(services::marketplace_service::MarketplaceService::new(
        pool.clone(),
        nowpayments_key.clone(),
        nowpayments_ipn_secret,
        cryptobot_token,
        cryptomus_merchant_id.clone(),
        cryptomus_payment_api_key.clone(),
        lava_project_id.clone(),
        lava_secret_key.clone(),
        aaio_merchant_id.clone(),
        aaio_secret_1.clone(),
        aaio_secret_2.clone(),
        stripe_secret_key,
        stripe_webhook_secret,
        // Новые провайдеры
        wata_jwt_token,
        wata_webhook_secret,
        crystalpay_login.clone(),
        crystalpay_secret.clone(),
        crystalpay_salt,
        tribute_api_key,
        tribute_webhook_secret,
        btcpay_url,
        btcpay_api_key,
        btcpay_store_id,
        btcpay_webhook_secret,
        oxapay_merchant_key,
        coinbase_api_key,
        coinbase_webhook_secret,
        plisio_api_key,
        marketplace_api_domain,
        marketplace_bot_username,
        (*store_service).clone(),
        (*subscription_service).clone(),
    ));

    let notification_service = Arc::new(services::notification_service::NotificationService::new(
        pool.clone(),
    ));

    // Telemetry Service (Phase 3) - Depends on Security, Notification, BotManager
    let telemetry_service = Arc::new(services::telemetry_service::TelemetryService::new(
        pool.clone(),
        security_service.clone(),
        notification_service.clone(),
        bot_manager.clone(),
    ));

    let admin_path_prefix = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path_prefix = if admin_path_prefix.starts_with('/') {
        admin_path_prefix
    } else {
        format!("/{}", admin_path_prefix)
    };

    // Initialize System Monitor
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let system_stats = std::sync::Arc::new(tokio::sync::Mutex::new(sys));

    let analytics_service = Arc::new(services::analytics_service::AnalyticsService::new(
        pool.clone(),
    ));

    let session_secret = std::env::var("SESSION_SECRET")
        .expect("SESSION_SECRET must be set (minimum 32 characters)");
    assert!(session_secret.len() >= 32, "SESSION_SECRET must be at least 32 characters");

    let promo_service = Arc::new(services::promo_service::PromoService::new(pool.clone()));

    let geo_db_path = std::env::var("GEOIP_DB_PATH")
        .unwrap_or("/usr/share/GeoIP/GeoLite2-Country.mmdb".to_string());
    // Check if file exists, else None
    let geo_path_opt = if std::path::Path::new(&geo_db_path).exists() {
        Some(geo_db_path)
    } else {
        None
    };
    let geo_service = Arc::new(services::geo_service::GeoService::new(
        geo_path_opt.as_deref(),
    ));

    // Реестр здоровья фоновых задач — собирает статистику успехов/ошибок каждого воркера
    let task_health = Arc::new(services::task_health::TaskHealthRegistry::new());

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
        marketplace_service,
        sni_repo,
        telemetry_service,
        infrastructure_service,
        security_service,
        promo_service,
        geo_service,

        ssh_public_key,
        // geo_cache moved to GeoService
        session_secret,
        admin_path: admin_path_prefix.clone(),
        system_stats,
        task_health,
    };

    if let Err(e) = state.sni_repo.seed_default_global_pool_if_empty().await {
        tracing::warn!("Failed to seed default global SNI pool: {}", e);
    }

    // Auto-start bot if enabled in settings
    let bot_token: String = state.settings.get_or_default("bot_token", "").await;
    let bot_status: String = state.settings.get_or_default("bot_status", "stopped").await;
    if !bot_token.is_empty() && bot_status == "running" {
        tracing::info!("Auto-starting bot...");
        let token_clone = bot_token.clone();
        if !state
            .bot_manager
            .start_bot(token_clone, state.clone())
            .await
        {
            tracing::warn!("Failed to auto-start bot, switching status to stopped");
            let _ = state.settings.set("bot_status", "stopped").await;
        }
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

    let connection_svc = state.connection_service.clone();
    tokio::spawn(async move {
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

    // Start Expiry Reminder background task
    let expiry_state = state.clone();
    tokio::spawn(async move {
        handlers::admin::run_expiry_reminder_loop(expiry_state).await;
    });

    use tower_http::services::ServeDir;

    // Routes
    let admin_routes = axum::Router::new()
        .nest_service("/assets", ServeDir::new("apps/caramba-panel/assets"))
        .route(
            "/dashboard",
            axum::routing::get(handlers::admin::get_dashboard),
        )
        .route(
            "/settings",
            axum::routing::get(handlers::admin::get_settings),
        )
        .route(
            "/updates",
            axum::routing::get(handlers::admin::get_updates_page),
        )
        .route(
            "/settings/save",
            axum::routing::post(handlers::admin::save_settings),
        )
        .route(
            "/settings/bot/toggle",
            axum::routing::post(handlers::admin::toggle_bot),
        )
        .route(
            "/settings/update/check",
            axum::routing::post(handlers::admin::check_update),
        ) // NEW
        .route(
            "/settings/topology/apply",
            axum::routing::post(handlers::admin::apply_deployment_topology),
        )
        .route(
            "/settings/update/agent/prepare",
            axum::routing::post(handlers::admin::prepare_agent_update),
        )
        // Marketplace
        .route(
            "/marketplace",
            axum::routing::get(handlers::admin::get_marketplace_page),
        )
        .route(
            "/marketplace/settings",
            axum::routing::post(handlers::admin::save_marketplace_settings),
        )
        .route(
            "/marketplace/manual/{id}/approve",
            axum::routing::post(handlers::admin::approve_manual_payment),
        )
        .route(
            "/marketplace/manual/{id}/reject",
            axum::routing::post(handlers::admin::reject_manual_payment),
        )
        .route(
            "/settings/update/agent/rollout",
            axum::routing::post(handlers::admin::rollout_agent_update),
        )
        .route(
            "/settings/update/worker/queue",
            axum::routing::post(handlers::admin::queue_worker_update),
        )
        // New Bot Page
        .route("/bot", axum::routing::get(handlers::admin::bot_logs_page))
        // Tools Logic (Page removed, actions preserved)
        // .route("/tools", axum::routing::get(handlers::admin::get_tools_page)) // Removed
        .route(
            "/tools/export",
            axum::routing::get(handlers::admin::export_database),
        )
        // .route("/traffic", axum::routing::get(handlers::admin::get_traffic_analytics)) // Merged into /analytics
        .route(
            "/logs",
            axum::routing::get(handlers::admin::get_system_logs_page),
        ) // NEW
        .route("/nodes", axum::routing::get(handlers::admin::get_nodes))
        .route(
            "/nodes/exit",
            axum::routing::get(handlers::admin::nodes::get_exit_nodes_page),
        )
        .route(
            "/nodes/relay",
            axum::routing::get(handlers::admin::nodes::get_relay_nodes_page),
        )
        .route(
            "/nodes/rows/exit",
            axum::routing::get(handlers::admin::nodes::get_exit_nodes_rows),
        )
        .route(
            "/nodes/rows/relay",
            axum::routing::get(handlers::admin::nodes::get_relay_nodes_rows),
        )
        .route(
            "/nodes/{id}/manage",
            axum::routing::get(handlers::admin::get_node_manage),
        ) // NEW Unified UI
        .route(
            "/nodes/install",
            axum::routing::post(handlers::admin::install_node),
        )
        .route(
            "/nodes/{id}/edit",
            axum::routing::get(handlers::admin::get_node_edit),
        )
        .route(
            "/nodes/{id}/restart",
            axum::routing::post(handlers::admin::nodes::restart_node),
        )
        .route(
            "/nodes/{id}/rotate",
            axum::routing::post(handlers::admin::nodes::rotate_node_inbounds),
        )
        .route(
            "/nodes/{id}/rotate-sni",
            axum::routing::post(handlers::admin::nodes::admin_rotate_node_sni),
        )
        .route(
            "/nodes/{id}/sni-interval",
            axum::routing::post(handlers::admin::nodes::update_sni_interval),
        )
        .route(
            "/nodes/{id}/rescan",
            axum::routing::post(handlers::admin::nodes::trigger_scan),
        ) // Fixed alias if needed or just ensure consistency
        .route(
            "/nodes/{id}/scan",
            axum::routing::post(handlers::admin::nodes::trigger_scan),
        )
        .route(
            "/nodes/{id}/config/preview",
            axum::routing::get(handlers::admin::nodes::get_node_config_preview),
        )
        .route(
            "/nodes/{id}/update",
            axum::routing::post(handlers::admin::update_node),
        )
        // Fix: Route was incorrect, trigger_update handler does not take ID in path
        .route(
            "/nodes/update/trigger",
            axum::routing::post(handlers::admin::updates::trigger_update),
        )
        .route(
            "/nodes/{id}/activate",
            axum::routing::post(handlers::admin::activate_node),
        )
        .route(
            "/nodes/{id}/sync",
            axum::routing::post(handlers::admin::sync_node),
        )
        .route(
            "/nodes/{id}/logs",
            axum::routing::get(handlers::admin::get_node_logs),
        )
        .route(
            "/nodes/{id}/rescue",
            axum::routing::get(handlers::admin::get_node_rescue),
        )
        // SSH-based Node Control removed - use Agent API endpoints instead
        .route(
            "/nodes/{id}/delete",
            axum::routing::delete(handlers::admin::delete_node),
        )
        .route(
            "/nodes/{id}/toggle",
            axum::routing::post(handlers::admin::toggle_node_enable),
        )
        .route(
            "/nodes/{id}/snis/{sni_id}/pin",
            axum::routing::post(handlers::admin::nodes::pin_sni),
        )
        .route(
            "/nodes/{id}/snis/{sni_id}/unpin",
            axum::routing::post(handlers::admin::nodes::unpin_sni),
        )
        .route(
            "/nodes/{id}/snis/{sni_id}/block",
            axum::routing::post(handlers::admin::nodes::block_sni),
        )
        .route(
            "/nodes/{id}/inbounds",
            axum::routing::get(handlers::admin_network::get_node_inbounds)
                .post(handlers::admin_network::add_inbound),
        )
        .route(
            "/nodes/{id}/inbounds/{inbound_id}",
            axum::routing::get(handlers::admin_network::get_edit_inbound)
                .post(handlers::admin_network::update_inbound)
                .delete(handlers::admin_network::delete_inbound),
        )
        .route(
            "/nodes/{id}/inbounds/{inbound_id}/toggle",
            axum::routing::post(handlers::admin_network::toggle_inbound),
        )
        .route("/plans", axum::routing::get(handlers::admin::get_plans))
        .route("/plans/add", axum::routing::post(handlers::admin::add_plan))
        .route(
            "/plans/{id}",
            axum::routing::get(handlers::admin::get_plan_edit)
                .post(handlers::admin::update_plan)
                .delete(handlers::admin::delete_plan),
        )
        .route(
            "/plans/{id}/bindings",
            axum::routing::get(handlers::admin_network::get_plan_bindings)
                .post(handlers::admin_network::save_plan_bindings),
        )
        .route("/users", get(handlers::admin::get_users))
        .route("/users/{id}", get(handlers::admin::get_user_details))
        .route(
            "/users/{id}/balance",
            post(handlers::admin::update_user_balance),
        )
        .route("/users/{id}/update", post(handlers::admin::update_user))
        .route(
            "/users/{id}/referral-rates",
            post(handlers::admin::update_user_referral_rates)
                .delete(handlers::admin::reset_user_referral_rates),
        )
        .route(
            "/users/{id}/gift",
            post(handlers::admin::admin_gift_subscription),
        )
        .route("/users/{id}/notify", post(handlers::admin::notify_user))
        .route(
            "/users/notify/preview",
            post(handlers::admin::notify_preview),
        )
        .route("/users/notify/all", post(handlers::admin::notify_all_users))
        // Dedicated Notifications page
        .route(
            "/notifications",
            get(handlers::admin::get_notifications_page),
        )
        .route(
            "/notifications/send",
            post(handlers::admin::notify_all_users),
        )
        .route(
            "/notifications/preview",
            post(handlers::admin::notify_preview),
        )
        .route(
            "/users/subs/{id}",
            axum::routing::delete(handlers::admin::delete_user_subscription),
        )
        .route(
            "/users/subs/{id}/refund",
            axum::routing::post(handlers::admin::refund_user_subscription),
        )
        .route(
            "/users/subs/{id}/extend",
            axum::routing::post(handlers::admin::extend_user_subscription),
        )
        .route(
            "/users/subs/{id}/set-node",
            axum::routing::post(handlers::admin::set_subscription_node),
        )
        .route(
            "/subs/{id}/devices",
            axum::routing::get(handlers::admin::get_subscription_devices),
        )
        .route(
            "/subs/{id}/devices/kill",
            axum::routing::post(handlers::admin::admin_kill_subscription_sessions),
        )
        .route(
            "/analytics",
            axum::routing::get(handlers::admin::get_traffic_analytics),
        )
        .route("/promo", axum::routing::get(handlers::admin::get_promos))
        .route(
            "/promo/add",
            axum::routing::post(handlers::admin::add_promo),
        )
        .route(
            "/promo/{id}/delete",
            axum::routing::delete(handlers::admin::delete_promo),
        )
        // Frontend Servers (Page)
        .route(
            "/frontends",
            axum::routing::get(handlers::admin::frontends::get_frontends),
        )
        .route(
            "/frontends/settings",
            axum::routing::post(handlers::admin::frontends::save_frontend_settings),
        )
        .route(
            "/partials/frontends_rows",
            axum::routing::get(handlers::admin::frontends::get_frontends_rows),
        )
        // Client API (Mini App) - served via .nest("/api/client", ...) below
        .route(
            "/transactions",
            axum::routing::get(handlers::admin::get_transactions),
        )
        .route(
            "/bot-logs",
            axum::routing::get(handlers::admin::bot_logs_page),
        )
        .route(
            "/bot-logs/history",
            axum::routing::get(handlers::admin::bot_logs_history),
        )
        .route(
            "/bot-logs/tail",
            axum::routing::get(handlers::admin::bot_logs_tail),
        )
        .route(
            "/api-keys",
            axum::routing::get(handlers::admin::list_api_keys)
                .post(handlers::admin::create_api_key),
        )
        .route(
            "/api-keys/delete/{id}",
            axum::routing::post(handlers::admin::delete_api_key),
        )
        // SNI Pool Management
        .route(
            "/sni",
            axum::routing::get(handlers::admin_sni::get_sni_page),
        )
        .route(
            "/sni/add",
            axum::routing::post(handlers::admin_sni::add_sni),
        )
        .route(
            "/sni/bulk",
            axum::routing::post(handlers::admin_sni::bulk_add_sni),
        )
        .route(
            "/sni/bulk-action",
            axum::routing::post(handlers::admin_sni::bulk_action_sni),
        )
        .route(
            "/sni/delete/{id}",
            axum::routing::delete(handlers::admin_sni::delete_sni),
        )
        .route(
            "/sni/toggle/{id}",
            axum::routing::post(handlers::admin_sni::toggle_sni),
        )
        .route(
            "/sni/favorite/{id}",
            axum::routing::post(handlers::admin_sni::toggle_favorite_sni),
        )
        .route(
            "/sni/blacklist/delete/{domain}",
            axum::routing::delete(handlers::admin_sni::unblock_sni),
        )
        .route(
            "/partials/statusbar",
            axum::routing::get(handlers::admin::get_statusbar),
        ) // NEW
        .route("/logout", axum::routing::post(handlers::admin::logout))
        // Store Management Routes
        .route(
            "/store/categories",
            axum::routing::get(handlers::admin::get_store_categories_page)
                .post(handlers::admin::create_category),
        )
        .route(
            "/store/categories/{id}",
            axum::routing::delete(handlers::admin::delete_category),
        )
        .route(
            "/store/products",
            axum::routing::get(handlers::admin::get_store_products_page)
                .post(handlers::admin::create_product),
        )
        .route(
            "/store/products/{id}",
            axum::routing::delete(handlers::admin::delete_product),
        )
        // Groups Management (Phase 1.8)
        .route(
            "/groups",
            axum::routing::get(handlers::admin_groups::get_groups_page)
                .post(handlers::admin_groups::create_group),
        )
        .route(
            "/groups/{id}",
            axum::routing::get(handlers::admin_groups::get_group_edit)
                .delete(handlers::admin_groups::delete_group),
        )
        .route(
            "/groups/{id}/members",
            axum::routing::post(handlers::admin_groups::add_group_member),
        )
        .route(
            "/groups/{id}/members/{node_id}",
            axum::routing::delete(handlers::admin_groups::remove_group_member),
        )
        .route(
            "/groups/{id}/rotate",
            axum::routing::post(handlers::admin_groups::rotate_group_inbounds),
        )
        // Templates Management
        .route(
            "/templates",
            axum::routing::get(handlers::admin_templates::get_templates_page)
                .post(handlers::admin_templates::create_template),
        )
        .route(
            "/templates/{id}",
            axum::routing::delete(handlers::admin_templates::delete_template)
                .post(handlers::admin_templates::update_template),
        )
        .route(
            "/templates/{id}/edit",
            axum::routing::get(handlers::admin_templates::get_template_edit),
        )
        .route(
            "/templates/{id}/sync",
            axum::routing::post(handlers::admin_templates::sync_template),
        )
        .route(
            "/templates/{id}/json",
            axum::routing::get(handlers::admin_templates::get_template_json),
        )
        // Organization Management (Phase 3)
        .route(
            "/orgs",
            axum::routing::get(handlers::admin_orgs::get_organizations)
                .post(handlers::admin_orgs::create_organization),
        )
        // .route("/store/orders", axum::routing::get(handlers::admin_store::orders_page)) // Handled by analytics/dashboard now
        // Task Health API — состояние фоновых воркеров мониторинга
        .route(
            "/api/health/tasks",
            axum::routing::get(handlers::admin::get_task_health),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') {
        admin_path
    } else {
        format!("/{}", admin_path)
    };
    tracing::info!("Admin panel available at: {}", admin_path);

    // Единый роутер API-маршрутов — регистрируется под /api и /caramba-api через .nest().
    // Webhooks добавлены в оба префикса для единообразия (ранее были только под /api).
    let api_routes: axum::Router<AppState> = axum::Router::new()
        // Payments & Webhooks
        .route(
            "/payments/{source}",
            axum::routing::post(handlers::admin::handle_payment),
        )
        .nest("/webhooks", api::webhooks::router())
        // Family API
        .route(
            "/family/invite",
            axum::routing::post(handlers::api::family::generate_invite),
        )
        .route(
            "/family/join",
            axum::routing::post(handlers::api::family::redeem_invite),
        )
        // Agent V2 API — node management
        .route(
            "/v2/node/heartbeat",
            axum::routing::post(api::v2::node::heartbeat),
        )
        .route(
            "/v2/node/config",
            axum::routing::get(api::v2::node::get_config),
        )
        .route(
            "/v2/node/rotate-sni",
            axum::routing::post(api::v2::node::rotate_sni),
        )
        .route(
            "/v2/node/update-info",
            axum::routing::get(api::v2::node::get_update_info),
        )
        .route(
            "/v2/node/updates/poll",
            axum::routing::get(api::v2::node::poll_updates),
        )
        .route(
            "/v2/node/logs",
            axum::routing::post(api::v2::node::report_node_logs),
        )
        .route(
            "/v2/node/settings",
            axum::routing::get(api::v2::node::get_settings),
        )
        .route(
            "/v2/node/register",
            axum::routing::post(api::v2::node::register),
        )
        // Bot API — защищённый роутер с проверкой X-Bot-Token и rate limiting
        .nest("/v2/bot", api::v2::bot_routes(state.clone()))
        // AI Routing — рекомендованные узлы для клиента
        .route(
            "/v2/client/recommended",
            axum::routing::get(api::v2::client::get_recommended_nodes),
        )
        // Client API
        .nest("/client", api::client::routes(state.clone()))
        // Internal API for Microservices (Sub/Bot)
        .route(
            "/internal/nodes/active",
            axum::routing::get(handlers::api::internal::get_active_nodes),
        )
        .route(
            "/internal/nodes/active/exit",
            axum::routing::get(handlers::api::internal::get_active_exit_nodes),
        )
        .route(
            "/internal/nodes/active/relay",
            axum::routing::get(handlers::api::internal::get_active_relay_nodes),
        )
        .route(
            "/internal/subscriptions/{uuid}",
            axum::routing::get(handlers::api::internal::get_subscription),
        )
        .route(
            "/internal/users/{id}/keys",
            axum::routing::get(handlers::api::internal::get_user_keys),
        )
        .route(
            "/internal/frontend/heartbeat",
            axum::routing::post(handlers::api::internal::frontend_heartbeat),
        )
        .route(
            "/internal/workers/{role}/updates/poll",
            axum::routing::get(handlers::api::internal::poll_worker_update),
        )
        .route(
            "/internal/workers/{role}/updates/report",
            axum::routing::post(handlers::api::internal::report_worker_update),
        )
        // Frontend API Routes
        .route(
            "/admin/frontends",
            axum::routing::get(handlers::frontend::list_frontends)
                .post(handlers::frontend::create_frontend),
        )
        .route(
            "/admin/frontends/by-region/{region}",
            axum::routing::get(handlers::frontend::get_active_frontends),
        )
        .route(
            "/admin/frontends/{id}",
            axum::routing::delete(handlers::frontend::delete_frontend),
        )
        .route(
            "/admin/frontends/{id}/rotate-token",
            axum::routing::post(handlers::frontend::rotate_token),
        )
        .route(
            "/admin/frontends/{domain}/heartbeat",
            axum::routing::post(handlers::frontend::frontend_heartbeat),
        );

    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::get({
                let path = admin_path.clone();
                move || async move {
                    let expose_root = std::env::var("EXPOSE_PANEL_ROOT_REDIRECT")
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if expose_root == "1" || expose_root == "true" || expose_root == "yes" {
                        return axum::response::Redirect::to(&format!("{}/dashboard", path))
                            .into_response();
                    }

                    (axum::http::StatusCode::NOT_FOUND, "Not found").into_response()
                }
            }),
        )
        .route(
            "/assets/css/modern.css",
            axum::routing::get(handlers::assets::modern_css),
        )
        .route(
            &format!("{}/login", admin_path),
            axum::routing::get(handlers::admin::get_login).post(handlers::admin::login),
        )
        // Serve Downloads (for frontend binaries)
        .nest_service("/downloads", ServeDir::new("apps/caramba-panel/downloads"))
        // Serve Assets (Public)
        .nest_service("/assets", ServeDir::new("apps/caramba-panel/assets"))
        // Setup Routes
        .route(
            &format!("{}/setup", admin_path),
            axum::routing::get(handlers::setup::get_setup),
        )
        .route(
            &format!("{}/setup/create_admin", admin_path),
            axum::routing::post(handlers::setup::create_admin),
        )
        .route(
            &format!("{}/setup/restore_backup", admin_path),
            axum::routing::post(handlers::setup::restore_backup),
        )
        // Единый роутер для всех API-маршрутов — регистрируется под /api и /caramba-api
        // через .nest(), чтобы не дублировать каждый маршрут дважды.
        .nest("/api", api_routes.clone())
        .nest("/caramba-api", api_routes)
        // Public Subscription URL endpoint
        .route(
            "/sub/{uuid}",
            axum::routing::get(subscription::subscription_handler),
        )
        // Local Mini App Serving
        .route("/app", axum::routing::get(handlers::local_app::serve_app))
        .route(
            "/app/{*path}",
            axum::routing::get(handlers::local_app::serve_app_assets),
        )
        .nest(&admin_path, admin_routes)
        // install.sh is a public, read-only shell script with no secrets — safe to serve unauthenticated
        .route(
            "/install.sh",
            axum::routing::get(handlers::admin::get_install_sh),
        )
        // Node install scripts expose per-node join tokens — must be behind admin auth middleware
        .nest(
            &admin_path,
            axum::Router::new()
                .route(
                    "/nodes/{id}/script",
                    axum::routing::get(handlers::admin::get_node_install_script),
                )
                .route(
                    "/nodes/{id}/raw-install",
                    axum::routing::get(handlers::admin::get_node_raw_install_script),
                )
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                )),
        )
        .with_state(state)
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            10 * 1024 * 1024,
        )) // 10MB limit
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

// Ожидает SIGTERM (systemd/Docker) или Ctrl-C и инициирует graceful shutdown
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown");
}

fn ensure_ssh_keys() -> Result<String> {
    let key_path = std::path::Path::new("id_rsa");
    let pub_path = std::path::Path::new("id_rsa.pub");

    if !key_path.exists() || !pub_path.exists() {
        tracing::info!("SSH keys not found. Generating new RSA keypair...");
        // Use ssh-keygen command
        let output = std::process::Command::new("ssh-keygen")
            .arg("-t")
            .arg("rsa")
            .arg("-b")
            .arg("4096")
            .arg("-f")
            .arg("id_rsa")
            .arg("-q") // Quiet
            .arg("-N")
            .arg("") // Empty passphrase
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute ssh-keygen: {}", e))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ssh-keygen failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    let pub_key = std::fs::read_to_string(pub_path)
        .map_err(|e| anyhow::anyhow!("Failed to read public key: {}", e))?;

    Ok(pub_key.trim().to_string())
}
