use clap::Parser;
use tracing::{info, warn, error};
use std::time::Duration;
use std::path::Path;
use exarobot_shared::api::{HeartbeatRequest, HeartbeatResponse};
use exarobot_shared::config::ConfigResponse;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Panel URL (e.g. https://panel.example.com)
    #[arg(short, long, env = "PANEL_URL")]
    panel_url: String,

    /// Node Registration Token
    #[arg(short, long, env = "NODE_TOKEN")]
    token: String,

    /// Node ID (optional, usually auto-generated)
    #[arg(short, long, env = "NODE_ID")]
    node_id: Option<String>,
    
    /// Config path (default: /etc/sing-box/config.json)
    #[arg(long, env = "CONFIG_PATH", default_value = "/etc/sing-box/config.json")]
    config_path: String,
}

struct AgentState {
    current_hash: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("🚀 EXA ROBOT Node Agent v0.2.0 Starting...");

    // 2. Load Config
    dotenvy::dotenv().ok();
    let args = Args::parse();

    // Normalize URL
    let mut panel_url = args.panel_url.trim().to_string();
    if !panel_url.starts_with("http://") && !panel_url.starts_with("https://") {
        panel_url = format!("https://{}", panel_url);
    }
    // Remove trailing slash
    if panel_url.ends_with('/') {
        panel_url.pop();
    }
    
    // Normalize Token
    let token = args.token.trim().to_string();

    info!("🔗 Panel URL: {}", panel_url);
    info!("🔑 Token: {}...", &token[0..4.min(token.len())]);
    info!("📁 Config Path: {}", args.config_path);

    // 3. Load current hash (if config exists)
    let mut state = AgentState {
        current_hash: load_current_hash(&args.config_path).await,
    };

    // Initialize HTTP Client
    let client = reqwest::Client::new();

    // 4. Fetch initial config
    info!("🔄 Fetching initial configuration from Panel...");
    match check_and_update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
        Ok(_) => {
            info!("✅ Initial configuration loaded successfully");
        }
        Err(e) => {
            error!("⚠️ Failed to fetch initial config: {}. Will retry in mainloop.", e);
        }
    }
    
    // 5. Main Loop
    let mut failures = 0;
    
    let start_time = std::time::Instant::now();

    loop {
        let uptime = start_time.elapsed().as_secs();
        
        // Send Heartbeat
        match send_heartbeat(&client, &panel_url, &token, uptime, &state).await {
            Ok(resp) => {
                failures = 0;
                info!("💓 Heartbeat OK. Action: {:?}", resp.action);
                
                // Check if config update needed
                match resp.action {
                    exarobot_shared::api::AgentAction::UpdateConfig => {
                        info!("🔄 Config update requested");
                        if let Err(e) = update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
                            error!("Failed to update config: {}", e);
                        }
                    },
                    _ => {}
                }
            }
            Err(e) => {
                failures += 1;
                error!("❌ Heartbeat failed ({}/10): {}", failures, e);
                if failures >= 10 {
                    warn!("⚠️ Too many failures, backing off...");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            }
        }
        
        // Periodic config check (every 10th heartbeat = ~100 seconds)
        if uptime % 100 < 10 {
            if let Err(e) = check_and_update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
                error!("Config check failed: {}", e);
            }
        }
        
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn send_heartbeat(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    uptime: u64,
    state: &AgentState,
) -> anyhow::Result<HeartbeatResponse> {
    let url = format!("{}/api/v2/node/heartbeat", panel_url);
    
    let payload = HeartbeatRequest {
        version: "0.2.0".to_string(),
        uptime,
        status: "running".to_string(),
        config_hash: state.current_hash.clone(),
        traffic_up: 0,
        traffic_down: 0,
        certificates: Some(check_certificates(&state.current_hash.as_ref().map(|_| "/etc/sing-box/config.json").unwrap_or("/etc/sing-box/config.json")).await),
    };
    
    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&payload)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Server error: {}", resp.status());
    }
    
    Ok(resp.json::<HeartbeatResponse>().await?)
}

async fn check_and_update_config(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    config_path: &str,
    state: &mut AgentState,
) -> anyhow::Result<()> {
    let url = format!("{}/api/v2/node/config", panel_url);
    
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Server error: {}", resp.status());
    }
    
    let config_resp: ConfigResponse = resp.json().await?;
    
    // Check if hash changed
    if state.current_hash.as_ref() != Some(&config_resp.hash) {
        info!("🔄 Config hash changed: {} -> {}", 
            state.current_hash.as_deref().unwrap_or("none"), 
            &config_resp.hash);
        
        // Save new config
        save_config(config_path, &config_resp.content).await?;
        state.current_hash = Some(config_resp.hash);
        
        // Restart sing-box
        restart_singbox()?;
        
        info!("✅ Config updated and service restarted");
    } else {
        info!("✓ Config up to date");
    }
    
    Ok(())
}

async fn update_config(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    config_path: &str,
    state: &mut AgentState,
) -> anyhow::Result<()> {
    check_and_update_config(client, panel_url, token, config_path, state).await
}

async fn load_current_hash(config_path: &str) -> Option<String> {
    if !Path::new(config_path).exists() {
        return None;
    }
    
    match tokio::fs::read_to_string(config_path).await {
        Ok(content) => {
            let hash = format!("{:x}", md5::compute(content.as_bytes()));
            info!("📄 Loaded config hash: {}", hash);
            Some(hash)
        },
        Err(_) => None,
    }
}

async fn save_config(path: &str, content: &serde_json::Value) -> anyhow::Result<()> {
    let json_str = serde_json::to_string_pretty(content)?;
    
    // Ensure directory exists
    if let Some(parent) = Path::new(path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    
    tokio::fs::write(path, json_str).await?;
    info!("💾 Config saved to {}", path);
    Ok(())
}

    info!("✅ Service restarted");
    Ok(())
}

async fn check_certificates(config_path: &str) -> Vec<exarobot_shared::api::CertificateStatus> {
    let mut statuses = Vec::new();
    let cert_dir = Path::new(config_path).parent().unwrap_or(Path::new("/etc/sing-box")).join("certs");
    
    if !cert_dir.exists() {
        return statuses;
    }

    // Read dir
    let mut entries = match tokio::fs::read_dir(&cert_dir).await {
        Ok(e) => e,
        Err(_) => return statuses,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        // Only check .pem files likely to be certs (not keys)
        // Convention: cert.pem or *.crt
        if let Some(ext) = path.extension() {
            if ext == "pem" || ext == "crt" {
                // Heuristic: check if this is a cert or key
                // Or just try openssl x509 on it. If it fails, maybe it's a key.
                
                let output = std::process::Command::new("openssl")
                    .args(&["x509", "-in", path.to_str().unwrap_or(""), "-noout", "-subject", "-enddate", "-checkend", "0"])
                    .output();

                match output {
                    Ok(out) => {
                        if out.status.success() {
                            let stdout = String::from_utf8_lossy(&out.stdout);
                            // Parse subject: subject=CN = drive.google.com
                            let sni = stdout.lines()
                                .find(|l| l.starts_with("subject="))
                                .and_then(|l| l.split("CN = ").nth(1))
                                .or_else(|| stdout.lines().find(|l| l.starts_with("subject=")).and_then(|l| l.split("CN=").nth(1))) 
                                // Handling both "CN = val" and "CN=val"
                                .map(|s| s.trim().to_string())
                                .unwrap_or_else(|| "unknown".to_string());

                            // Parse expiry
                            // openssl -checkend 0 returns 0 if valid (not expired), 1 if expired
                            // But we also want the date for display.
                            // We don't parse date strictly here for now to avoid chrono dep complexity if not present, 
                            // but we can trust checkend for valid flag.
                            let valid = out.status.code() == Some(0);

                            // For expires_at, we might need to parse "notAfter=Jan 27 00:00:00 2036 GMT"
                            // For MVP, just return current timestamp + 1 year if valid? 
                            // Or better: use openssl -enddate -noout -> "notAfter=..."
                            // Implementation detail: Shared struct requires expires_at: i64.
                            // We can use 0 for now or implement parsing.
                            
                            statuses.push(exarobot_shared::api::CertificateStatus {
                                sni,
                                valid,
                                expires_at: 0, // TODO: Parse logic
                                error: None,
                            });
                        }
                    },
                    Err(_) => {}
                }
            }
        }
    }
    
    statuses
}
