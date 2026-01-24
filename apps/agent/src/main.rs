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

    info!("🔗 Panel URL: {}", args.panel_url);
    info!("🔑 Token: {}...", &args.token[0..4.min(args.token.len())]);
    info!("📁 Config Path: {}", args.config_path);

    // 3. Load current hash (if config exists)
    let mut state = AgentState {
        current_hash: load_current_hash(&args.config_path).await,
    };

    // 4. Main Loop
    let client = reqwest::Client::new();
    let mut failures = 0;
    
    let start_time = std::time::Instant::now();

    loop {
        let uptime = start_time.elapsed().as_secs();
        
        // Send Heartbeat
        match send_heartbeat(&client, &args, uptime, &state).await {
            Ok(resp) => {
                failures = 0;
                info!("💓 Heartbeat OK. Action: {:?}", resp.action);
                
                // Check if config update needed
                match resp.action {
                    exarobot_shared::api::AgentAction::UpdateConfig => {
                        info!("🔄 Config update requested");
                        if let Err(e) = update_config(&client, &args, &mut state).await {
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
            if let Err(e) = check_and_update_config(&client, &args, &mut state).await {
                error!("Config check failed: {}", e);
            }
        }
        
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn send_heartbeat(
    client: &reqwest::Client,
    args: &Args,
    uptime: u64,
    state: &AgentState,
) -> anyhow::Result<HeartbeatResponse> {
    let url = format!("{}/api/v2/node/heartbeat", args.panel_url);
    
    let payload = HeartbeatRequest {
        version: "0.2.0".to_string(),
        uptime,
        status: "running".to_string(),
        config_hash: state.current_hash.clone(),
        traffic_up: 0,
        traffic_down: 0,
    };
    
    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", args.token))
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
    args: &Args,
    state: &mut AgentState,
) -> anyhow::Result<()> {
    let url = format!("{}/api/v2/node/config", args.panel_url);
    
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", args.token))
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
        save_config(&args.config_path, &config_resp.content).await?;
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
    args: &Args,
    state: &mut AgentState,
) -> anyhow::Result<()> {
    check_and_update_config(client, args, state).await
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

fn restart_singbox() -> anyhow::Result<()> {
    info!("🔄 Restarting sing-box service...");
    
    let output = std::process::Command::new("systemctl")
        .args(&["restart", "sing-box"])
        .output()?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("systemctl restart failed: {}", stderr);
    }
    
    info!("✅ Service restarted");
    Ok(())
}
