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
        /// Force reinstall
        #[arg(long)]
        force: bool,
        /// Install everything (Panel, Node, Bot, Sub) in Hub Mode
        #[arg(long)]
        hub: bool,
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
    Uninstall,
}

mod setup;
mod install;
mod diagnose;
mod restore;
mod assets;

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
        Commands::Install { panel, node, hub, force } => {
            if !panel && !node && !hub {
                println!("Please specify --panel, --node, or --hub to install.");
                exit(1);
            }
            


            // Run interactive setup if panel is selected or hub mode
            if panel || hub {
                let config = setup::interactive_setup(hub); // Pass hub flag
                println!("Configuration captured. Proceeding with installation...");
                
                if let Err(e) = install::install_dependencies().await {
                    eprintln!("Failed to install dependencies: {}", e);
                    exit(1);
                }
                
                if let Err(e) = install::configure_firewall().await {
                    eprintln!("Failed to configure firewall: {}", e);
                }
                
                if hub {
                     // Default version or fetch latest. For now hardcode or env
                     let version = std::env::var("CARAMBA_VERSION").unwrap_or("latest".to_string());
                     
                     // Setup DB and Env BEFORE binaries start up via services
                     if let Err(e) = install::setup_database(&config) {
                        eprintln!("Failed to setup database: {}", e);
                        exit(1);
                     }
                     if let Err(e) = install::create_env_file(&config) {
                        eprintln!("Failed to create .env file: {}", e);
                         exit(1);
                     }

                     if let Err(e) = install::install_hub(&config, &version).await {
                        eprintln!("Failed to install Hub components: {}", e);
                        exit(1);
                     }
                } else if panel {
                    if let Err(e) = install::install_service("caramba-panel.service") {
                        eprintln!("Failed to install panel service: {}", e);
                    }
                }

                // Configure Caddy
                let caddyfile = setup::generate_caddyfile(&config); // Pass full config
                if let Err(e) = install::write_caddyfile(&caddyfile) {
                    eprintln!("Failed to configure Caddy: {}", e);
                }

                println!("{}", style("System dependencies and service installed.").green());
            }
            if node || hub {
                println!("Installing Caramba Node...");
                // install::install_node().await;
                 if let Err(e) = install::install_service("caramba-node.service") {
                    eprintln!("Failed to install node service: {}", e);
                }
            }
        },
        Commands::Upgrade => {
            println!("Checking for updates...");
        },
        Commands::Diagnose => {
            if let Err(e) = diagnose::run_diagnostics() {
                eprintln!("Diagnostics failed: {}", e);
            }
        },
        Commands::Restore { file } => {
            if let Err(e) = restore::run_restore(&file) {
                 eprintln!("Restore failed: {}", e);
            }
        },
        Commands::Admin => {
            println!("Admin tools...");
        },
        Commands::Uninstall => {
            println!("Uninstalling Caramba...");
        }
    }
}
