use clap::Parser;
use tracing::{info, warn, error};
use std::time::Duration;
use std::path::Path;
use exarobot_shared::api::{HeartbeatRequest, HeartbeatResponse};
use exarobot_shared::config::ConfigResponse;

mod sni_check;
mod self_update;
mod decoy_service; // NEW

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
    // Kill Switch State
    last_successful_contact: std::time::Instant,
    kill_switch_enabled: bool,
    kill_switch_timeout: u64,
    vpn_stopped_by_kill_switch: bool,
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
        last_successful_contact: std::time::Instant::now(),
        kill_switch_enabled: false,
        kill_switch_timeout: 300,
        vpn_stopped_by_kill_switch: false,
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
    
    // 5. Start Decoy Service (Background)
    let decoy_svc = decoy_service::DecoyService::new(panel_url.clone(), token.clone());
    tokio::spawn(async move {
        decoy_svc.run_loop().await;
    });

    // 6. Main Loop
    let mut failures = 0;
    
    let start_time = std::time::Instant::now();

    loop {
        let uptime = start_time.elapsed().as_secs();
        
        // Send Heartbeat
        match send_heartbeat(&client, &panel_url, &token, uptime, &state).await {
            Ok(resp) => {
                failures = 0;
                state.last_successful_contact = std::time::Instant::now(); // Update contact time
                
                // If we were stopped by kill switch, revive!
                if state.vpn_stopped_by_kill_switch {
                    info!("✅ Connection restored! Reviving VPN service...");
                    if let Err(e) = restart_singbox() {
                        error!("Failed to revive VPN: {}", e);
                    } else {
                        state.vpn_stopped_by_kill_switch = false;
                    }
                }
                failures = 0;
                state.last_successful_contact = std::time::Instant::now(); // Update contact time

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
                // Check for Agent Update
                if let Some(target_ver) = resp.latest_version {
                    // Simple string comparison for now, or use semver crate if added
                    // Assuming versions are "x.y.z"
                    let current_version = "0.2.0";
                    if target_ver != current_version && target_ver != "0.0.0" {
                         info!("📣 New version available: {} (Current: {})", target_ver, current_version);
                         
                         // Fetch update info
                         let info_url = format!("{}/api/v2/node/update-info", panel_url);
                         match client.get(&info_url).header("Authorization", format!("Bearer {}", token)).send().await {
                             Ok(r) => {
                                 if let Ok(json) = r.json::<serde_json::Value>().await {
                                     let download_url = json["url"].as_str().unwrap_or("");
                                     let hash = json["hash"].as_str().unwrap_or("");
                                     
                                     if !download_url.is_empty() && !hash.is_empty() {
                                          if let Err(e) = self_update::perform_update(&client, download_url, hash).await {
                                               error!("❌ Self-update failed: {}", e);
                                          }
                                     }
                                 }
                             },
                             Err(e) => error!("Failed to fetch update info: {}", e)
                         }
                    }
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
            
            // Fetch Global Settings (Kill Switch / Decoy)
            if let Err(e) = fetch_global_settings(&client, &panel_url, &token, &mut state).await {
                error!("Failed to fetch settings: {}", e);
            }
            
            // Fetch Global Settings (Kill Switch / Decoy)
            if let Err(e) = fetch_global_settings(&client, &panel_url, &token, &mut state).await {
                error!("Failed to fetch settings: {}", e);
            }

            // SNI Health Check
            if let Some(current_sni) = sni_check::get_current_sni(&args.config_path).await {
                if !sni_check::check_reachability(&current_sni).await {
                    error!("⚠️ SNI {} is unreachable! Triggering rotation...", current_sni);
                    match rotate_sni(&client, &panel_url, &token, &current_sni).await {
                        Ok(new_sni) => {
                            info!("✅ SNI Rotated to {}. Updating config...", new_sni);
                            // Force immediate config update
                            if let Err(e) = update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
                                error!("Failed to update config after rotation: {}", e);
                            }
                        },
                        Err(e) => error!("❌ Failed to rotate SNI: {}", e),
                    }
                }
            }
        }
        }
        
        // KILL SWITCH MONITOR
        if state.kill_switch_enabled && !state.vpn_stopped_by_kill_switch {
            if state.last_successful_contact.elapsed().as_secs() > state.kill_switch_timeout {
                warn!("⚠️ EMERGENCY KILL SWITCH TRIGGERED! Lost connection for {}s (Timeout: {}s)", 
                    state.last_successful_contact.elapsed().as_secs(), state.kill_switch_timeout);
                
                if let Err(e) = stop_singbox() {
                    error!("❌ FAILED TO STOP VPN SERVICE: {}", e);
                } else {
                    state.vpn_stopped_by_kill_switch = true;
                    warn!("💀 VPN Service has been terminated.");
                }
            }
        }
        }
        
        // KILL SWITCH MONITOR
        if state.kill_switch_enabled && !state.vpn_stopped_by_kill_switch {
            if state.last_successful_contact.elapsed().as_secs() > state.kill_switch_timeout {
                warn!("⚠️ EMERGENCY KILL SWITCH TRIGGERED! Lost connection for {}s (Timeout: {}s)", 
                    state.last_successful_contact.elapsed().as_secs(), state.kill_switch_timeout);
                
                if let Err(e) = stop_singbox() {
                    error!("❌ FAILED TO STOP VPN SERVICE: {}", e);
                } else {
                    state.vpn_stopped_by_kill_switch = true;
                    warn!("💀 VPN Service has been terminated.");
                }
            }
        }
        
        // Long poll (replaces sleep(10))
        // This effectively makes the heartbeat interval ~30s (timeout) unless update occurs
        match poll_events(&client, &panel_url, &token).await {
            Ok(should_update) => {
                if should_update {
                    info!("⚡ Instant Update Received!");
                    if let Err(e) = update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
                        error!("Failed to update config: {}", e);
                    }
                }
            },
            Err(e) => {
                warn!("Long poll failed or timed out locally: {}. Backing off 5s.", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn poll_events(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
) -> anyhow::Result<bool> {
    let url = format!("{}/api/v2/node/updates/poll", panel_url);
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .timeout(Duration::from_secs(40)) // Allows 30s server wait + buffer
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(false);
    }

    let json: serde_json::Value = resp.json().await?;
    Ok(json.get("update").and_then(|v| v.as_bool()).unwrap_or(false))
}

async fn rotate_sni(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    current_sni: &str,
) -> anyhow::Result<String> {
    let url = format!("{}/api/v2/node/rotate-sni", panel_url);
    
    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "current_sni": current_sni,
            "reason": "Health check failed (Agent detected unreachable)"
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Rotation failed: {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await?;
    let new_sni = json.get("new_sni")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid response"))?
        .to_string();
        
    Ok(new_sni)
}

async fn send_heartbeat(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    uptime: u64,
    state: &AgentState,
) -> anyhow::Result<HeartbeatResponse> {
    let url = format!("{}/api/v2/node/heartbeat", panel_url);
    
    // Collect Telemetry
    let (latency, cpu, ram) = collect_telemetry(client).await;

    let payload = HeartbeatRequest {
        version: "0.2.0".to_string(),
        uptime,
        status: "running".to_string(),
        config_hash: state.current_hash.clone(),
        traffic_up: 0,
        traffic_down: 0,
        certificates: Some(check_certificates(&state.current_hash.as_ref().map(|_| "/etc/sing-box/config.json").unwrap_or("/etc/sing-box/config.json")).await),
        latency,
        cpu_usage: cpu,
        memory_usage: ram,
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

// Helper to stop sing-box
fn stop_singbox() -> anyhow::Result<()> {
    info!("🛑 Stopping sing-box service (Kill Switch Triggered)...");
    let output = std::process::Command::new("systemctl")
        .args(&["stop", "sing-box"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("systemctl stop failed");
    }
    Ok(())
}

async fn fetch_global_settings(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    state: &mut AgentState,
) -> anyhow::Result<()> {
    let url = format!("{}/api/v2/node/settings", panel_url);
    let resp = client.get(&url).header("Authorization", format!("Bearer {}", token)).send().await?;
    
    if resp.status().is_success() {
        let json: serde_json::Value = resp.json().await?;
        if let Some(ks) = json.get("kill_switch") {
            state.kill_switch_enabled = ks.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            state.kill_switch_timeout = ks.get("timeout").and_then(|v| v.as_u64()).unwrap_or(300);
        }
    }
    Ok(())
}

async fn collect_telemetry(client: &reqwest::Client) -> (Option<f64>, Option<f64>, Option<f64>) {
    // 1. Latency Check (HTTP HEAD to Google)
    let start = std::time::Instant::now();
    let latency = match client.head("https://www.google.com")
        .timeout(Duration::from_secs(3))
        .send()
        .await 
    {
        Ok(_) => Some(start.elapsed().as_millis() as f64),
        Err(_) => None,
    };

    // 2. System Stats (CPU/RAM)
    // sys-info calls are blocking but fast (read /proc)
    let cpu = sys_info::loadavg().map(|l| l.one).ok();
    
    let ram = sys_info::mem_info().map(|m| {
        if m.total == 0 { return 0.0; }
        let used = m.total - m.free; // Simple approximation
        (used as f64 / m.total as f64) * 100.0
    }).ok();

    (latency, cpu, ram)
}
