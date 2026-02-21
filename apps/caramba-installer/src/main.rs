use clap::{Parser, Subcommand};
use console::style;
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use std::process::exit;

#[derive(Parser)]
#[command(name = "caramba-installer")]
#[command(about = "Caramba VPN Installer & Manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
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
    Upgrade {
        /// Installation directory
        #[arg(long)]
        install_dir: Option<String>,
        /// Target version tag (e.g. v0.3.24). Defaults to latest stable release.
        #[arg(long)]
        version: Option<String>,
        /// Do not restart services after upgrade
        #[arg(long)]
        no_restart: bool,
    },
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

fn prompt_text(prompt: &str, default: Option<&str>) -> String {
    let theme = ColorfulTheme::default();
    let mut input = Input::<String>::with_theme(&theme).with_prompt(prompt.to_string());
    if let Some(d) = default {
        input = input.default(d.to_string());
    }
    input.interact_text().unwrap_or_default().trim().to_string()
}

fn prompt_optional_text(prompt: &str) -> Option<String> {
    let value = prompt_text(prompt, Some(""));
    if value.is_empty() { None } else { Some(value) }
}

fn prompt_bool(prompt: &str, default: bool) -> bool {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .default(default)
        .interact()
        .unwrap_or(default)
}

fn run_self_command(args: &[String]) {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Failed to locate installer binary: {}", e);
            return;
        }
    };

    let status = std::process::Command::new(exe).args(args).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("Command failed with status: {}", s),
        Err(e) => eprintln!("Failed to run command: {}", e),
    }
}

fn run_tui_menu() {
    let theme = ColorfulTheme::default();

    loop {
        println!();
        let options = vec![
            "Install Hub (Panel + Sub + optional Bot)",
            "Upgrade existing installation",
            "Install Node",
            "Install Frontend/Sub service",
            "Install Bot service",
            "Run diagnostics",
            "Uninstall Caramba",
            "Exit",
        ];

        let selected = Select::with_theme(&theme)
            .with_prompt("Caramba Installer")
            .default(0)
            .items(&options)
            .interact()
            .unwrap_or(options.len() - 1);

        match selected {
            0 => {
                let mut args = vec!["install".to_string(), "--hub".to_string()];
                if prompt_bool("Skip apt/system dependencies?", false) {
                    args.push("--skip-deps".to_string());
                }
                run_self_command(&args);
            }
            1 => {
                let install_dir = prompt_text("Install directory", Some("/opt/caramba"));
                let version = prompt_optional_text("Target version (e.g. v0.3.24, empty = latest)");
                let no_restart = prompt_bool("Do not restart services after upgrade?", false);

                let mut args = vec![
                    "upgrade".to_string(),
                    "--install-dir".to_string(),
                    install_dir,
                ];
                if let Some(v) = version {
                    args.push("--version".to_string());
                    args.push(v);
                }
                if no_restart {
                    args.push("--no-restart".to_string());
                }
                run_self_command(&args);
            }
            2 => {
                let panel_url = prompt_text("Panel URL (https://panel.example.com)", None);
                let token = prompt_text("Node token (join/enrollment token)", None);
                let install_dir = prompt_text("Install directory", Some("/opt/caramba"));
                let mut args = vec![
                    "install".to_string(),
                    "--node".to_string(),
                    "--panel-url".to_string(),
                    panel_url,
                    "--token".to_string(),
                    token,
                    "--install-dir".to_string(),
                    install_dir,
                ];
                if prompt_bool("Skip apt/system dependencies?", false) {
                    args.push("--skip-deps".to_string());
                }
                run_self_command(&args);
            }
            3 => {
                let domain = prompt_text("Frontend domain", None);
                let panel_url = prompt_text("Panel URL (https://panel.example.com)", None);
                let token = prompt_text("Frontend auth token", None);
                let region = prompt_text("Region", Some("global"));
                let listen_port = prompt_text("Listen port", Some("8080"));
                let install_dir = prompt_text("Install directory", Some("/opt/caramba"));
                let mut args = vec![
                    "install".to_string(),
                    "--sub".to_string(),
                    "--domain".to_string(),
                    domain,
                    "--panel-url".to_string(),
                    panel_url,
                    "--token".to_string(),
                    token,
                    "--region".to_string(),
                    region,
                    "--listen-port".to_string(),
                    listen_port,
                    "--install-dir".to_string(),
                    install_dir,
                ];
                if prompt_bool("Skip apt/system dependencies?", false) {
                    args.push("--skip-deps".to_string());
                }
                run_self_command(&args);
            }
            4 => {
                let panel_url = prompt_text("Panel URL (https://panel.example.com)", None);
                let bot_token = prompt_text("BOT_TOKEN", None);
                let panel_token = prompt_optional_text("Optional PANEL_TOKEN");
                let install_dir = prompt_text("Install directory", Some("/opt/caramba"));
                let mut args = vec![
                    "install".to_string(),
                    "--bot".to_string(),
                    "--panel-url".to_string(),
                    panel_url,
                    "--bot-token".to_string(),
                    bot_token,
                    "--install-dir".to_string(),
                    install_dir,
                ];
                if let Some(token) = panel_token {
                    args.push("--panel-token".to_string());
                    args.push(token);
                }
                if prompt_bool("Skip apt/system dependencies?", false) {
                    args.push("--skip-deps".to_string());
                }
                run_self_command(&args);
            }
            5 => {
                run_self_command(&["diagnose".to_string()]);
            }
            6 => {
                let install_dir = prompt_text("Install directory", Some("/opt/caramba"));
                let keep_db = prompt_bool("Keep PostgreSQL database and role?", false);
                let mut args = vec![
                    "uninstall".to_string(),
                    "--install-dir".to_string(),
                    install_dir,
                ];
                if keep_db {
                    args.push("--keep-db".to_string());
                }
                run_self_command(&args);
            }
            _ => break,
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

    if cli.command.is_none() {
        run_tui_menu();
        return;
    }

    match cli.command.expect("checked is_some") {
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
        Commands::Upgrade {
            install_dir,
            version,
            no_restart,
        } => {
            let install_dir = pick_non_empty(install_dir, "INSTALL_DIR")
                .unwrap_or_else(|| "/opt/caramba".to_string());
            let version_hint = version
                .or_else(|| std::env::var("CARAMBA_VERSION").ok())
                .unwrap_or_else(|| "latest".to_string());
            let resolved_version = match install::resolve_version(&version_hint).await {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to resolve release version: {}", e);
                    exit(1);
                }
            };

            if let Err(e) =
                install::upgrade_caramba(&install_dir, &resolved_version, !no_restart).await
            {
                eprintln!("Upgrade failed: {}", e);
                exit(1);
            }
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
