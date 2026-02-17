use std::process::{Command, Stdio};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use anyhow::{Result, anyhow};
use std::io::Write;
use crate::assets::Assets;

pub fn check_root() -> Result<()> {
    if whoami::username() != "root" {
        return Err(anyhow!("This installer must be run as root."));
    }
    Ok(())
}

fn run_command(cmd: &str, args: &[&str], msg: &str) -> Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
    pb.set_message(format!("{}", msg));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let status = Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if status.success() {
        pb.finish_with_message(format!("✅ {}", msg));
        Ok(())
    } else {
        pb.finish_with_message(format!("❌ {}", msg));
        Err(anyhow!("Command failed: {} {:?}", cmd, args))
    }
}

pub async fn install_dependencies() -> Result<()> {
    println!("\n{}", style("Installing System Dependencies...").bold());

    // 1. Update Apt
    run_command("apt-get", &["update"], "Updating package lists")?;

    // 2. Pre-installation for Caddy
    let pre_packages = ["debian-keyring", "debian-archive-keyring", "apt-transport-https", "curl", "gnupg"];
    for pkg in pre_packages {
        run_command("apt-get", &["install", "-y", pkg], &format!("Installing prereq: {}", pkg))?;
    }

    // 3. Add Caddy Repo
    println!("Adding Caddy repository...");
    let gpg_cmd = "curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg";
    let status = Command::new("sh").arg("-c").arg(gpg_cmd).status()?;
    if !status.success() {
        return Err(anyhow!("Failed to add Caddy GPG key"));
    }

    let repo_cmd = "curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | tee /etc/apt/sources.list.d/caddy-stable.list";
    let status = Command::new("sh").arg("-c").arg(repo_cmd).status()?;
    if !status.success() {
        return Err(anyhow!("Failed to add Caddy repo"));
    }

    run_command("apt-get", &["update"], "Updating package lists (with Caddy)")?;

    // 4. Install Packages
    let packages = [
        "postgresql", "postgresql-contrib",
        "redis-server",
        "caddy",
        "unzip", "curl", "ufw",
        "nodejs", "npm"
    ];

    for pkg in packages {
        run_command("apt-get", &["install", "-y", pkg], &format!("Installing {}", pkg))?;
    }

    Ok(())
}

pub async fn configure_firewall() -> Result<()> {
    println!("\n{}", style("Configuring Firewall (UFW)...").bold());
    
    run_command("ufw", &["allow", "22/tcp"], "Allowing SSH (22)")?;
    run_command("ufw", &["allow", "80/tcp"], "Allowing HTTP (80)")?;
    run_command("ufw", &["allow", "443/tcp"], "Allowing HTTPS (443)")?;
    run_command("ufw", &["allow", "3000/tcp"], "Allowing Panel (3000)")?; // Optional if proxied

    Ok(())
}


pub fn install_service(service_name: &str) -> Result<()> {
    let file_content = Assets::get(service_name)
        .ok_or_else(|| anyhow!("Service file '{}' not found in assets", service_name))?;
    
    let path = format!("/etc/systemd/system/{}", service_name);
    println!("Installing service to {}", path);
    
    let mut file = std::fs::File::create(&path)?;
    file.write_all(file_content.data.as_ref())?;
    
    run_command("systemctl", &["daemon-reload"], "Reloading systemd")?;
    run_command("systemctl", &["enable", service_name], &format!("Enabling {}", service_name))?;
    
    Ok(())
}

pub fn write_caddyfile(content: &str) -> Result<()> {
    println!("Configuring Caddy...");
    std::fs::write("/etc/caddy/Caddyfile", content)?;
    run_command("systemctl", &["reload", "caddy"], "Reloading Caddy")?;
    Ok(())
}

pub async fn download_file(url: &str, path: &str) -> Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} Downloading {msg}").unwrap());
    pb.set_message(path.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
         return Err(anyhow!("Failed to download file from {}: Status {}", url, response.status()));
    }
    
    let content = response.bytes().await?;
    std::fs::write(path, content)?;
    
    run_command("chmod", &["+x", path], "Making executable")?;

    pb.finish_with_message(format!("✅ Downloaded {}", path));
    Ok(())
}

pub async fn install_hub(config: &crate::setup::InstallConfig, version: &str) -> Result<()> {
    println!("{}", style("\nInstalling Hub Components...").bold());

    // 1. Prepare directories
    std::fs::create_dir_all(&config.install_dir)?;
    
    // 2. Download Binaries
    let base_url = format!("https://github.com/semanticparadox/caramba/releases/download/{}", version);
    
    let panel_path = format!("{}/caramba-panel", config.install_dir);
    download_file(&format!("{}/caramba-panel", base_url), &panel_path).await?;
    
    let sub_path = format!("{}/caramba-sub", config.install_dir);
    download_file(&format!("{}/caramba-sub", base_url), &sub_path).await?;
    
    let bot_path = format!("{}/caramba-bot", config.install_dir);
    download_file(&format!("{}/caramba-bot", base_url), &bot_path).await?;
    
    let node_path = format!("{}/caramba-node", config.install_dir);
    download_file(&format!("{}/caramba-node", base_url), &node_path).await?;
    
    // 3. Install Services
    install_service("caramba-panel.service")?;
    install_service("caramba-sub.service")?;
    install_service("caramba-bot.service")?;
    install_service("caramba-node.service")?;
    
    Ok(())
}

pub fn setup_database(config: &crate::setup::InstallConfig) -> Result<()> {
    println!("{}", style("\nConfiguring Database...").bold());

    // Check if user exists (ignoring error if they do)
    let _ = Command::new("sudo")
        .args(&["-u", "postgres", "createuser", "caramba"])
        .output(); // ignore error

    // Set password
    let psql_cmd = format!("ALTER USER caramba WITH PASSWORD '{}';", config.db_pass);
    run_command("sudo", &["-u", "postgres", "psql", "-c", &psql_cmd], "Setting DB password")?;

    // Create DB
    let _ = Command::new("sudo")
        .args(&["-u", "postgres", "createdb", "-O", "caramba", "caramba"])
        .output(); // ignore if exists

    Ok(())
}

pub fn create_env_file(config: &crate::setup::InstallConfig) -> Result<()> {
    println!("Creating .env file...");
    let env_content = format!(
r#"DATABASE_URL=postgres://caramba:{}@localhost/caramba
REDIS_URL=redis://127.0.0.1:6379
SERVER_DOMAIN={}
API_DOMAIN={}
ADMIN_PATH={}
PANEL_PORT=3000
SESSION_SECRET={}
"#,
        config.db_pass,
        config.domain,
        config.domain, // API same as domain for now
        config.admin_path,
        uuid::Uuid::new_v4().to_string()
    );

    let path = format!("{}/.env", config.install_dir);
    std::fs::write(&path, env_content)?;
    println!("✅ Written .env to {}", path);
    Ok(())
}
