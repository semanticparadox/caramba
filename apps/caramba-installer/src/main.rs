use clap::{Parser, Subcommand};
use console::style;
use std::process::exit;

#[derive(Parser)]
#[command(name = "caramba-installer")]
#[command(about = "Caramba VPN Installer & Manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install Caramba components
    Install {
        /// Install panel
        #[arg(long)]
        panel: bool,
        /// Install node agent
        #[arg(long)]
        node: bool,
        /// Install sub/frontend edge service
        #[arg(long)]
        sub: bool,
        /// Install telegram bot service
        #[arg(long)]
        bot: bool,
        /// Force reinstall
        #[arg(long)]
        force: bool,
        /// Install everything (Panel, Node, Bot, Sub) in Hub Mode
        #[arg(long)]
        hub: bool,
        /// Panel domain (for panel/hub setup)
        #[arg(long)]
        domain: Option<String>,
        /// Subscription domain (for hub setup)
        #[arg(long = "sub-domain")]
        sub_domain: Option<String>,
        /// Admin path (e.g. /admin)
        #[arg(long)]
        admin_path: Option<String>,
        /// Installation directory
        #[arg(long)]
        install_dir: Option<String>,
        /// Database password
        #[arg(long)]
        db_pass: Option<String>,
        /// Initial admin username
        #[arg(long)]
        admin_user: Option<String>,
        /// Initial admin password
        #[arg(long)]
        admin_pass: Option<String>,
        /// Panel URL (for node/sub/bot roles)
        #[arg(long)]
        panel_url: Option<String>,
        /// Token for role.
        /// - node: join token OR enrollment key
        /// - sub: internal/frontend auth token
        #[arg(long)]
        token: Option<String>,
        /// Sub/frontend region label
        #[arg(long)]
        region: Option<String>,
        /// Sub/frontend listen port
        #[arg(long)]
        listen_port: Option<u16>,
        /// Telegram bot token
        #[arg(long)]
        bot_token: Option<String>,
        /// Panel API token for bot (optional)
        #[arg(long)]
        panel_token: Option<String>,
        /// Skip installing apt dependencies
        #[arg(long)]
        skip_deps: bool,
    },
    /// Upgrade Caramba components
    Upgrade,
    /// Run diagnostics
    Diagnose,
    /// Administrative tools
    Admin,
    /// Restore from backup
    Restore {
        /// Backup file path
        file: String,
    },
    /// Uninstall Caramba
    Uninstall {
        /// Installation directory
        #[arg(long)]
        install_dir: Option<String>,
        /// Keep PostgreSQL database and role
        #[arg(long)]
        keep_db: bool,
    },
}

mod assets;
mod diagnose;
mod install;
mod restore;
mod setup;

fn pick_non_empty(value: Option<String>, env_key: &str) -> Option<String> {
    if let Some(v) = value {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    std::env::var(env_key).ok().and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn require_value(value: Option<String>, name: &str) -> String {
    match value {
        Some(v) => v,
        None => {
            eprintln!("Missing required value: {}", name);
            exit(1);
        }
    }
}

fn print_install_summary(config: &setup::InstallConfig, bot_token: Option<&str>) {
    let panel_host = config.domain.trim();
    let scheme = if panel_host.starts_with("http://") || panel_host.starts_with("https://") {
        ""
    } else {
        "https://"
    };
    let admin_path = if config.admin_path.starts_with('/') {
        config.admin_path.clone()
    } else {
        format!("/{}", config.admin_path)
    };
    let login_url = format!(
        "{}{}/{}/login",
        scheme,
        panel_host,
        admin_path.trim_start_matches('/')
    );
    let sub_domain = config
        .sub_domain
        .as_deref()
        .unwrap_or(config.domain.as_str())
        .to_string();

    let summary = format!(
        "\n=== CARAMBA INSTALL SUMMARY ===\n\
Panel domain: {panel}\n\
Subscription domain: {sub}\n\
Admin path: {admin_path}\n\
Login URL: {login_url}\n\
Database password: {db_pass}\n\
Admin username: {admin_user}\n\
Admin password: {admin_pass}\n\
Install directory: {install_dir}\n\
BOT_TOKEN: {bot_token}\n\
===============================\n",
        panel = config.domain,
        sub = sub_domain,
        admin_path = config.admin_path,
        login_url = login_url,
        db_pass = config.db_pass,
        admin_user = config.admin_username,
        admin_pass = config.admin_password,
        install_dir = config.install_dir,
        bot_token = bot_token.unwrap_or("<not set>"),
    );

    println!("{}", style(&summary).green());

    let summary_path = format!(
        "{}/INSTALL_SUMMARY.txt",
        config.install_dir.trim_end_matches('/')
    );
    if let Err(e) = std::fs::write(&summary_path, summary) {
        eprintln!("Failed to write summary file {}: {}", summary_path, e);
    } else {
        println!("Saved install summary to {}", summary_path);
    }
}

async fn resolve_release_version_or_exit() -> String {
    let hint = std::env::var("CARAMBA_VERSION").unwrap_or_else(|_| "latest".to_string());
    match install::resolve_version(&hint).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to resolve release version: {}", e);
            exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    println!("{}", style("Caramba Installer Daemon v0.1.0").bold().cyan());
    println!("{}", style("=================================").cyan());

    // Ensure root
    if let Err(e) = install::check_root() {
        eprintln!("{}", style(format!("Error: {}", e)).red());
        exit(1);
    }

    match cli.command {
        Commands::Install {
            panel,
            node,
            sub,
            bot,
            hub,
            force: _,
            domain,
            sub_domain,
            admin_path,
            install_dir,
            db_pass,
            admin_user,
            admin_pass,
            panel_url,
            token,
            region,
            listen_port,
            bot_token,
            panel_token,
            skip_deps,
        } => {
            let roles_count =
                (panel as u8) + (node as u8) + (sub as u8) + (bot as u8) + (hub as u8);

            if roles_count == 0 {
                println!("Please specify one role: --hub | --panel | --node | --sub | --bot");
                exit(1);
            }

            if roles_count > 1 {
                println!("Please install one role per run.");
                exit(1);
            }

            if hub {
                let config = match setup::resolve_install_config(
                    true,
                    domain,
                    sub_domain,
                    admin_path,
                    install_dir,
                    db_pass,
                    pick_non_empty(admin_user, "ADMIN_USER"),
                    pick_non_empty(admin_pass, "ADMIN_PASS"),
                    pick_non_empty(bot_token.clone(), "BOT_TOKEN"),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Failed to capture configuration: {}", e);
                        exit(1);
                    }
                };
                println!("Configuration captured. Proceeding with installation...");

                if !skip_deps {
                    if let Err(e) = install::install_dependencies().await {
                        eprintln!("Failed to install dependencies: {}", e);
                        exit(1);
                    }

                    if let Err(e) = install::configure_firewall().await {
                        eprintln!("Failed to configure firewall: {}", e);
                    }
                }

                let version = resolve_release_version_or_exit().await;

                if let Err(e) = install::setup_database(&config) {
                    eprintln!("Failed to setup database: {}", e);
                    exit(1);
                }

                let internal_api_token = install::generate_internal_api_token();
                if let Err(e) = install::create_env_file(&config, Some(&internal_api_token)) {
                    eprintln!("Failed to create .env file: {}", e);
                    exit(1);
                }

                let maybe_node_token = pick_non_empty(token, "NODE_TOKEN")
                    .or_else(|| pick_non_empty(None, "ENROLLMENT_KEY"));
                let maybe_bot_token =
                    pick_non_empty(bot_token, "BOT_TOKEN").or_else(|| config.hub_bot_token.clone());
                let region = pick_non_empty(region, "REGION");

                if let Err(e) = install::install_hub(
                    &config,
                    &version,
                    &internal_api_token,
                    maybe_node_token.as_deref(),
                    maybe_bot_token.as_deref(),
                    region.as_deref(),
                )
                .await
                {
                    eprintln!("Failed to install Hub components: {}", e);
                    exit(1);
                }

                if let Err(e) = install::bootstrap_admin(
                    &config.install_dir,
                    &config.admin_username,
                    &config.admin_password,
                ) {
                    eprintln!("Failed to bootstrap admin account: {}", e);
                    exit(1);
                }

                // Configure Caddy
                let caddyfile = setup::generate_caddyfile(&config); // Pass full config
                if let Err(e) = install::write_caddyfile(&caddyfile) {
                    eprintln!("Failed to configure Caddy: {}", e);
                }

                println!(
                    "{}",
                    style("System dependencies and service installed.").green()
                );
                print_install_summary(&config, maybe_bot_token.as_deref());
            } else if panel {
                let config = match setup::resolve_install_config(
                    false,
                    domain,
                    None,
                    admin_path,
                    install_dir,
                    db_pass,
                    pick_non_empty(admin_user, "ADMIN_USER"),
                    pick_non_empty(admin_pass, "ADMIN_PASS"),
                    None,
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Failed to capture configuration: {}", e);
                        exit(1);
                    }
                };
                println!("Configuration captured. Proceeding with panel installation...");

                if !skip_deps {
                    if let Err(e) = install::install_dependencies().await {
                        eprintln!("Failed to install dependencies: {}", e);
                        exit(1);
                    }

                    if let Err(e) = install::configure_firewall().await {
                        eprintln!("Failed to configure firewall: {}", e);
                    }
                }

                if let Err(e) = install::setup_database(&config) {
                    eprintln!("Failed to setup database: {}", e);
                    exit(1);
                }

                let internal_api_token = install::generate_internal_api_token();
                if let Err(e) = install::create_env_file(&config, Some(&internal_api_token)) {
                    eprintln!("Failed to create .env file: {}", e);
                    exit(1);
                }

                let version = resolve_release_version_or_exit().await;
                if let Err(e) = install::install_panel(&config.install_dir, &version).await {
                    eprintln!("Failed to install panel: {}", e);
                    exit(1);
                }

                if let Err(e) = install::bootstrap_admin(
                    &config.install_dir,
                    &config.admin_username,
                    &config.admin_password,
                ) {
                    eprintln!("Failed to bootstrap admin account: {}", e);
                    exit(1);
                }

                let caddyfile = setup::generate_caddyfile(&config);
                if let Err(e) = install::write_caddyfile(&caddyfile) {
                    eprintln!("Failed to configure Caddy: {}", e);
                }

                println!("{}", style("Panel installation completed.").green());
                print_install_summary(&config, None);
            } else if node {
                let panel_url = require_value(
                    pick_non_empty(panel_url, "PANEL_URL"),
                    "--panel-url or PANEL_URL",
                );
                let token = require_value(
                    pick_non_empty(token, "NODE_TOKEN")
                        .or_else(|| pick_non_empty(None, "ENROLLMENT_KEY")),
                    "--token (join token or enrollment key)",
                );
                let install_dir = pick_non_empty(install_dir, "INSTALL_DIR")
                    .unwrap_or_else(|| "/opt/caramba".to_string());
                let version = resolve_release_version_or_exit().await;

                if let Err(e) =
                    install::install_node(&install_dir, &version, &panel_url, &token).await
                {
                    eprintln!("Failed to install node: {}", e);
                    exit(1);
                }
            } else if sub {
                let panel_url = require_value(
                    pick_non_empty(panel_url, "PANEL_URL"),
                    "--panel-url or PANEL_URL",
                );
                let domain = require_value(
                    pick_non_empty(domain, "FRONTEND_DOMAIN"),
                    "--domain or FRONTEND_DOMAIN",
                );
                let token =
                    require_value(pick_non_empty(token, "AUTH_TOKEN"), "--token or AUTH_TOKEN");
                let region =
                    pick_non_empty(region, "REGION").unwrap_or_else(|| "global".to_string());
                let listen_port = listen_port
                    .or_else(|| {
                        std::env::var("LISTEN_PORT")
                            .ok()
                            .and_then(|v| v.parse::<u16>().ok())
                    })
                    .unwrap_or(8080);
                let install_dir = pick_non_empty(install_dir, "INSTALL_DIR")
                    .unwrap_or_else(|| "/opt/caramba".to_string());
                let version = resolve_release_version_or_exit().await;

                if let Err(e) = install::install_sub(
                    &install_dir,
                    &version,
                    &domain,
                    &panel_url,
                    &token,
                    &region,
                    listen_port,
                )
                .await
                {
                    eprintln!("Failed to install sub/frontend: {}", e);
                    exit(1);
                }
            } else if bot {
                let panel_url = require_value(
                    pick_non_empty(panel_url, "PANEL_URL"),
                    "--panel-url or PANEL_URL",
                );
                let bot_token = require_value(
                    pick_non_empty(bot_token, "BOT_TOKEN"),
                    "--bot-token or BOT_TOKEN",
                );
                let panel_token = pick_non_empty(panel_token, "PANEL_TOKEN");
                let install_dir = pick_non_empty(install_dir, "INSTALL_DIR")
                    .unwrap_or_else(|| "/opt/caramba".to_string());
                let version = resolve_release_version_or_exit().await;

                if let Err(e) = install::install_bot(
                    &install_dir,
                    &version,
                    &panel_url,
                    &bot_token,
                    panel_token.as_deref(),
                )
                .await
                {
                    eprintln!("Failed to install bot: {}", e);
                    exit(1);
                }
            }
        }
        Commands::Upgrade => {
            println!("Checking for updates...");
        }
        Commands::Diagnose => {
            if let Err(e) = diagnose::run_diagnostics() {
                eprintln!("Diagnostics failed: {}", e);
            }
        }
        Commands::Restore { file } => {
            if let Err(e) = restore::run_restore(&file) {
                eprintln!("Restore failed: {}", e);
            }
        }
        Commands::Admin => {
            println!("Admin tools...");
        }
        Commands::Uninstall {
            install_dir,
            keep_db,
        } => {
            let install_dir = pick_non_empty(install_dir, "INSTALL_DIR")
                .unwrap_or_else(|| "/opt/caramba".to_string());
            println!("Uninstalling Caramba from {} ...", install_dir);
            if let Err(e) = install::uninstall_caramba(&install_dir, !keep_db) {
                eprintln!("Uninstall failed: {}", e);
                exit(1);
            }
            println!("{}", style("Caramba uninstall completed.").green());
        }
    }
}
