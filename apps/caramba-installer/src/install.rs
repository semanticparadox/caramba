use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use crate::assets::Assets;

pub fn check_root() -> Result<()> {
    if whoami::username() != "root" {
        return Err(anyhow!("This installer must be run as root."));
    }
    Ok(())
}

fn run_command(cmd: &str, args: &[&str], msg: &str) -> Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
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

fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn normalize_panel_url(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    if !value.starts_with("http://") && !value.starts_with("https://") {
        value = format!("https://{}", value);
    }
    while value.ends_with('/') {
        value.pop();
    }
    value
}

fn release_asset_url(version: &str, asset: &str) -> String {
    format!(
        "https://github.com/semanticparadox/caramba/releases/download/{}/{}",
        version, asset
    )
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

fn is_stable_semver_tag(tag: &str) -> bool {
    if !tag.starts_with('v') || tag.contains('-') {
        return false;
    }

    let trimmed = &tag[1..];
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 {
        return false;
    }

    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

pub async fn resolve_version(version_hint: &str) -> Result<String> {
    let hint = version_hint.trim();
    if !hint.is_empty() && hint != "latest" {
        return Ok(hint.to_string());
    }

    let client = reqwest::Client::new();
    let releases_resp = client
        .get("https://api.github.com/repos/semanticparadox/caramba/releases")
        .header("User-Agent", "caramba-installer")
        .send()
        .await?;

    if releases_resp.status().is_success() {
        let releases: Vec<GitHubRelease> = releases_resp.json().await.unwrap_or_default();
        for release in releases {
            if is_stable_semver_tag(&release.tag_name) {
                return Ok(release.tag_name);
            }
        }
    }

    let latest_resp = client
        .get("https://api.github.com/repos/semanticparadox/caramba/releases/latest")
        .header("User-Agent", "caramba-installer")
        .send()
        .await?;
    if !latest_resp.status().is_success() {
        return Err(anyhow!(
            "Failed to resolve latest release version (status {})",
            latest_resp.status()
        ));
    }
    let latest: GitHubRelease = latest_resp.json().await?;
    if !is_stable_semver_tag(&latest.tag_name) {
        return Err(anyhow!(
            "Latest release tag '{}' is not a stable semantic version",
            latest.tag_name
        ));
    }
    Ok(latest.tag_name)
}

pub fn generate_internal_api_token() -> String {
    format!("int_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
}

fn write_role_env_file(
    install_dir: &str,
    file_name: &str,
    vars: Vec<(String, String)>,
) -> Result<()> {
    std::fs::create_dir_all(install_dir)?;
    let mut content = String::new();
    for (key, value) in vars {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        content.push_str(&format!("{}={}\n", key, trimmed));
    }

    let path = format!("{}/{}", install_dir.trim_end_matches('/'), file_name);
    std::fs::write(&path, content)?;
    println!("✅ Written {}", path);
    Ok(())
}

pub async fn install_dependencies() -> Result<()> {
    println!("\n{}", style("Installing System Dependencies...").bold());

    // 1. Update Apt
    run_command("apt-get", &["update"], "Updating package lists")?;

    // 2. Pre-installation for Caddy
    let pre_packages = [
        "debian-keyring",
        "debian-archive-keyring",
        "apt-transport-https",
        "curl",
        "gnupg",
    ];
    for pkg in pre_packages {
        run_command(
            "apt-get",
            &["install", "-y", pkg],
            &format!("Installing prereq: {}", pkg),
        )?;
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

    run_command(
        "apt-get",
        &["update"],
        "Updating package lists (with Caddy)",
    )?;

    // 4. Install Packages
    let packages = [
        "postgresql",
        "postgresql-contrib",
        "redis-server",
        "caddy",
        "unzip",
        "curl",
        "ufw",
        "nodejs",
    ];

    for pkg in packages {
        run_command(
            "apt-get",
            &["install", "-y", pkg],
            &format!("Installing {}", pkg),
        )?;
    }

    if command_exists("npm") {
        println!("✅ npm is already available (bundled with nodejs or preinstalled).");
    } else {
        // npm is not available in some Debian/Ubuntu combinations.
        // Treat it as optional to avoid aborting installation.
        let npm_status = Command::new("apt-get")
            .args(["install", "-y", "npm"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match npm_status {
            Ok(status) if status.success() => {
                println!("✅ Installed npm");
            }
            _ => {
                println!(
                    "⚠️ npm package is unavailable via apt on this host. Continuing installer."
                );
            }
        }
    }

    // Rust is not required for release-binary installation, but report status for diagnostics.
    if command_exists("rustc") && command_exists("cargo") {
        println!("✅ Rust toolchain detected (rustc + cargo).");
    } else {
        println!("ℹ️ Rust toolchain not found. This is OK for release-binary installation.");
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

pub fn install_service(service_name: &str, install_dir: &str) -> Result<()> {
    let file_content = Assets::get(service_name)
        .ok_or_else(|| anyhow!("Service file '{}' not found in assets", service_name))?;
    let raw = std::str::from_utf8(file_content.data.as_ref())
        .map_err(|e| anyhow!("Service file '{}' is not valid UTF-8: {}", service_name, e))?;
    let normalized_dir = install_dir.trim_end_matches('/');
    let rendered = raw.replace("/opt/caramba", normalized_dir);

    let path = format!("/etc/systemd/system/{}", service_name);
    println!("Installing service to {}", path);

    let mut file = std::fs::File::create(&path)?;
    file.write_all(rendered.as_bytes())?;

    run_command("systemctl", &["daemon-reload"], "Reloading systemd")?;
    run_command(
        "systemctl",
        &["enable", service_name],
        &format!("Enabling {}", service_name),
    )?;

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
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} Downloading {msg}")
            .unwrap(),
    );
    pb.set_message(path.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to download file from {}: Status {}",
            url,
            response.status()
        ));
    }

    let content = response.bytes().await?;
    std::fs::write(path, content)?;

    run_command("chmod", &["+x", path], "Making executable")?;

    pb.finish_with_message(format!("✅ Downloaded {}", path));
    Ok(())
}

async fn try_install_mini_app_assets(version: &str, install_dir: &str) -> Result<()> {
    let url = release_asset_url(version, "caramba-app-dist.tar.gz");
    let archive_path = format!(
        "/tmp/caramba-app-dist-{}.tar.gz",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    );

    let response = match reqwest::get(&url).await {
        Ok(resp) => resp,
        Err(e) => {
            println!(
                "⚠️ Mini app asset request failed ({}). Skipping /app assets.",
                e
            );
            return Ok(());
        }
    };

    if !response.status().is_success() {
        println!(
            "⚠️ Mini app asset not found at {} (status {}). Skipping /app assets.",
            url,
            response.status()
        );
        return Ok(());
    }

    let content = response.bytes().await?;
    std::fs::write(&archive_path, content)?;

    std::fs::create_dir_all(install_dir)?;
    let args_owned = [
        "-xzf".to_string(),
        archive_path.clone(),
        "-C".to_string(),
        install_dir.to_string(),
    ];
    let args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
    run_command("tar", &args, "Extracting mini app assets")?;
    let _ = std::fs::remove_file(&archive_path);
    Ok(())
}

pub async fn install_panel(install_dir: &str, version: &str) -> Result<()> {
    std::fs::create_dir_all(install_dir)?;
    let binary_path = format!("{}/caramba-panel", install_dir.trim_end_matches('/'));
    download_file(&release_asset_url(version, "caramba-panel"), &binary_path).await?;
    try_install_mini_app_assets(version, install_dir).await?;
    install_service("caramba-panel.service", install_dir)?;
    Ok(())
}

#[derive(Serialize)]
struct RegisterNodeRequest {
    enrollment_key: String,
    hostname: String,
    ip: Option<String>,
}

#[derive(Deserialize)]
struct RegisterNodeResponse {
    join_token: String,
}

fn detect_hostname() -> String {
    for args in [&["-f"][..], &[][..]] {
        if let Ok(output) = Command::new("hostname").args(args).output() {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !value.is_empty() {
                    return value;
                }
            }
        }
    }
    "caramba-node".to_string()
}

async fn resolve_node_token(panel_url: &str, token: &str) -> Result<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Node token must not be empty"));
    }

    if !trimmed.starts_with("EXA-ENROLL-") {
        return Ok(trimmed.to_string());
    }

    println!("Enrollment key detected. Registering node on panel...");
    let req = RegisterNodeRequest {
        enrollment_key: trimmed.to_string(),
        hostname: detect_hostname(),
        ip: None,
    };

    let url = format!("{}/api/v2/node/register", panel_url);
    let client = reqwest::Client::new();
    let resp = client.post(url).json(&req).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Failed to enroll node (status {}): {}",
            status,
            body
        ));
    }

    let data: RegisterNodeResponse = resp.json().await?;
    Ok(data.join_token)
}

pub async fn install_node(
    install_dir: &str,
    version: &str,
    panel_url: &str,
    token: &str,
) -> Result<()> {
    std::fs::create_dir_all(install_dir)?;
    let panel_url = normalize_panel_url(panel_url);
    let join_token = resolve_node_token(&panel_url, token).await?;

    let binary_path = format!("{}/caramba-node", install_dir.trim_end_matches('/'));
    download_file(&release_asset_url(version, "caramba-node"), &binary_path).await?;
    write_role_env_file(
        install_dir,
        "node.env",
        vec![
            ("PANEL_URL".to_string(), panel_url),
            ("NODE_TOKEN".to_string(), join_token),
        ],
    )?;
    install_service("caramba-node.service", install_dir)?;
    Ok(())
}

pub async fn install_sub(
    install_dir: &str,
    version: &str,
    domain: &str,
    panel_url: &str,
    token: &str,
    region: &str,
    listen_port: u16,
) -> Result<()> {
    std::fs::create_dir_all(install_dir)?;
    let panel_url = normalize_panel_url(panel_url);

    let binary_path = format!("{}/caramba-sub", install_dir.trim_end_matches('/'));
    download_file(&release_asset_url(version, "caramba-sub"), &binary_path).await?;
    try_install_mini_app_assets(version, install_dir).await?;
    write_role_env_file(
        install_dir,
        "sub.env",
        vec![
            ("FRONTEND_DOMAIN".to_string(), domain.trim().to_string()),
            ("PANEL_URL".to_string(), panel_url),
            ("AUTH_TOKEN".to_string(), token.trim().to_string()),
            ("REGION".to_string(), region.trim().to_string()),
            ("LISTEN_PORT".to_string(), listen_port.to_string()),
        ],
    )?;
    install_service("caramba-sub.service", install_dir)?;
    Ok(())
}

pub async fn install_bot(
    install_dir: &str,
    version: &str,
    panel_url: &str,
    bot_token: &str,
    panel_token: Option<&str>,
) -> Result<()> {
    std::fs::create_dir_all(install_dir)?;
    let panel_url = normalize_panel_url(panel_url);

    let binary_path = format!("{}/caramba-bot", install_dir.trim_end_matches('/'));
    download_file(&release_asset_url(version, "caramba-bot"), &binary_path).await?;

    let mut vars = vec![
        ("PANEL_URL".to_string(), panel_url),
        ("BOT_TOKEN".to_string(), bot_token.trim().to_string()),
    ];
    if let Some(v) = panel_token {
        if !v.trim().is_empty() {
            vars.push(("PANEL_TOKEN".to_string(), v.trim().to_string()));
        }
    }
    write_role_env_file(install_dir, "bot.env", vars)?;
    install_service("caramba-bot.service", install_dir)?;
    Ok(())
}

pub async fn install_hub(
    config: &crate::setup::InstallConfig,
    version: &str,
    internal_api_token: &str,
    node_token: Option<&str>,
    bot_token: Option<&str>,
    region: Option<&str>,
) -> Result<()> {
    println!("{}", style("\nInstalling Hub Components...").bold());
    std::fs::create_dir_all(&config.install_dir)?;

    // Panel (core control plane)
    install_panel(&config.install_dir, version).await?;

    // Sub/frontend on hub server.
    let sub_domain = config
        .sub_domain
        .clone()
        .unwrap_or_else(|| config.domain.clone());
    let panel_url = normalize_panel_url(&config.domain);
    install_sub(
        &config.install_dir,
        version,
        &sub_domain,
        &panel_url,
        internal_api_token,
        region.unwrap_or("hub"),
        8080,
    )
    .await?;

    // Optional bot on hub.
    if let Some(bt) = bot_token {
        if !bt.trim().is_empty() {
            install_bot(&config.install_dir, version, &panel_url, bt, None).await?;
        } else {
            println!("⚠️ BOT_TOKEN is empty. Skipping bot service install.");
        }
    } else {
        println!("ℹ️ BOT_TOKEN not provided. Skipping bot service install.");
    }

    // Optional node on hub.
    if let Some(nt) = node_token {
        if !nt.trim().is_empty() {
            install_node(&config.install_dir, version, &panel_url, nt).await?;
        } else {
            println!("⚠️ Node token is empty. Skipping node service install.");
        }
    } else {
        println!("ℹ️ Node token not provided. Skipping node service install.");
    }

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
    run_command(
        "sudo",
        &["-u", "postgres", "psql", "-c", &psql_cmd],
        "Setting DB password",
    )?;

    // Create DB
    let _ = Command::new("sudo")
        .args(&["-u", "postgres", "createdb", "-O", "caramba", "caramba"])
        .output(); // ignore if exists

    Ok(())
}

pub fn create_env_file(
    config: &crate::setup::InstallConfig,
    internal_api_token: Option<&str>,
) -> Result<()> {
    println!("Creating .env file...");
    std::fs::create_dir_all(&config.install_dir)?;

    let panel_url = normalize_panel_url(&config.domain);
    let mut env_content = format!(
        r#"DATABASE_URL=postgres://caramba:{}@localhost/caramba
REDIS_URL=redis://127.0.0.1:6379
SERVER_DOMAIN={}
API_DOMAIN={}
PANEL_URL={}
ADMIN_PATH={}
PANEL_PORT=3000
SESSION_SECRET={}
"#,
        config.db_pass,
        config.domain,
        config.domain, // API same as domain for now
        panel_url,
        config.admin_path,
        uuid::Uuid::new_v4().to_string()
    );

    if let Some(token) = internal_api_token {
        if !token.trim().is_empty() {
            env_content.push_str(&format!("INTERNAL_API_TOKEN={}\n", token.trim()));
        }
    }

    let path = format!("{}/.env", config.install_dir);
    std::fs::write(&path, env_content)?;
    println!("✅ Written .env to {}", path);
    Ok(())
}
