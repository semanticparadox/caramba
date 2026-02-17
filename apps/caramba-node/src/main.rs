use clap::Parser;
use tracing::{info, warn, error};
use sysinfo::System;
use std::time::Duration;
use std::path::Path;
use caramba_shared::api::{HeartbeatRequest, HeartbeatResponse};
use caramba_shared::config::ConfigResponse;

mod sni_check;
mod self_update;
mod decoy_service; 
mod scanner; // NEW

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
    cached_speed_mbps: Option<i32>,
    recent_discoveries: std::sync::Arc<tokio::sync::Mutex<Vec<caramba_shared::DiscoveredSni>>>,
    scan_trigger: tokio::sync::mpsc::Sender<()>, // NEW: Pulse for neighbor sniper
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
     // Initialize System Monitor
    let mut sys = System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(sysinfo::MemoryRefreshKind::everything())
    );
    sys.refresh_all();
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
    let (scan_tx, scan_rx) = tokio::sync::mpsc::channel::<()>(1);
    
    let mut state = AgentState {
        current_hash: load_current_hash(&args.config_path).await,
        last_successful_contact: std::time::Instant::now(),
        kill_switch_enabled: false,
        kill_switch_timeout: 300,
        vpn_stopped_by_kill_switch: false,
        cached_speed_mbps: None,
        recent_discoveries: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        scan_trigger: scan_tx,
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
    
    // 4.5. Run Initial Speed Test
    info!("🚀 Running initial speed test (this may take a moment)...");
    let speed = run_speed_test(&client).await;
    if let Some(s) = speed {
        info!("✅ Speed test result: {} Mbps", s);
        state.cached_speed_mbps = Some(s);
    } else {
        warn!("⚠️ Speed test failed or timed out.");
    }
    
    // 5. Start Decoy Service (Background)
    let decoy_svc = decoy_service::DecoyService::new(panel_url.clone(), token.clone());
    tokio::spawn(async move {
        decoy_svc.run_loop().await;
    });

    // 5.5 Start Neighbor Sniper (Phase 7)
    let discoveries = state.recent_discoveries.clone();
    tokio::spawn(async move {
        start_neighbor_sniper(discoveries, scan_rx).await;
    });

    // 6. Main Loop
    let mut failures = 0;
    
    let start_time = std::time::Instant::now();

    loop {
        let uptime = start_time.elapsed().as_secs();
        
        // Send Heartbeat
        match send_heartbeat(&client, &panel_url, &token, uptime, &state, &mut sys).await {
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
                info!("💓 Heartbeat OK. Action: {:?}", resp.action);
                
                match resp.action {
                    caramba_shared::api::AgentAction::UpdateConfig => {
                        info!("🔄 Config update requested");
                        if let Err(e) = update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
                            error!("Failed to update config: {}", e);
                        }
                    },
                    caramba_shared::api::AgentAction::CollectLogs => {
                        info!("📋 Log collection requested");
                        let panel_url_clone = panel_url.clone();
                        let token_clone = token.clone();
                        let config_path_clone = args.config_path.clone();
                        let client_clone = client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = report_logs(&client_clone, &panel_url_clone, &token_clone, &config_path_clone).await {
                                error!("Failed to report logs: {}", e);
                            }
                        });
                    },
                    _ => {}
                }
                // Check for Agent Update
                if let Some(target_ver) = resp.latest_version {
                    // Simple string comparison for now, or use semver crate if added
                    // Assuming versions are "x.y.z"
                    let current_version = env!("CARGO_PKG_VERSION");
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
            Ok(signal) => {
                match signal {
                    Some(SignalType::Update) => {
                        info!("⚡ Instant Update Received!");
                        if let Err(e) = update_config(&client, &panel_url, &token, &args.config_path, &mut state).await {
                            error!("Failed to update config: {}", e);
                        }
                    },
                    Some(SignalType::Scan) => {
                        info!("🔍 Manual Scan Signal Received!");
                        let _ = state.scan_trigger.try_send(());
                    },
                    None => {}
                }
            },
            Err(e) => {
                warn!("Long poll failed or timed out locally: {}. Backing off 5s.", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

#[derive(Debug)]
enum SignalType {
    Update,
    Scan,
}

async fn poll_events(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
) -> anyhow::Result<Option<SignalType>> {
    let url = format!("{}/api/v2/node/updates/poll", panel_url);
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .timeout(Duration::from_secs(40)) // Allows 30s server wait + buffer
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let json: serde_json::Value = resp.json().await?;
    
    // Check for "update": true or "scan": true (if we change panel to send scan:true)
    // Or check if a generic "message" field says "scan"
    if json.get("update").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Ok(Some(SignalType::Update));
    }
    
    // Support generic signal from PubSub message
    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
        if msg == "scan" { return Ok(Some(SignalType::Scan)); }
        if msg == "update" { return Ok(Some(SignalType::Update)); }
    }

    Ok(None)
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
    sys: &mut System,
) -> anyhow::Result<HeartbeatResponse> {
    let url = format!("{}/api/v2/node/heartbeat", panel_url);
    
    // Collect Telemetry
    let (latency, cpu, ram, connections, max_ram, cpu_cores, cpu_model) = collect_telemetry(client, sys).await;

    let payload = HeartbeatRequest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime,
        status: "running".to_string(),
        config_hash: state.current_hash.clone(),
        traffic_up: 0,
        traffic_down: 0,
        certificates: Some(check_certificates(&state.current_hash.as_ref().map(|_| "/etc/sing-box/config.json").unwrap_or("/etc/sing-box/config.json")).await),
        latency,
        cpu_usage: cpu,
        memory_usage: ram,
        speed_mbps: state.cached_speed_mbps,
        active_connections: connections, // Added Phase 3
        max_ram,
        cpu_cores,
        cpu_model,
        user_usage: None,
        discovered_snis: {
            let mut lock = state.recent_discoveries.lock().await;
            if lock.is_empty() { 
                None 
            } else {
                let items = lock.clone();
                lock.clear(); // Clear after sending
                Some(items)
            }
        },
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

async fn check_certificates(config_path: &str) -> Vec<caramba_shared::api::CertificateStatus> {
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
                            
                            statuses.push(caramba_shared::api::CertificateStatus {
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

async fn collect_telemetry(client: &reqwest::Client, sys: &mut System) -> (Option<f64>, Option<f64>, Option<f64>, Option<u32>, Option<u64>, Option<i32>, Option<String>) {
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
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu = Some(sys.global_cpu_usage() as f64);
    
    let total_mem = sys.total_memory();
    let ram = if total_mem > 0 {
        Some((sys.used_memory() as f64 / total_mem as f64) * 100.0)
    } else {
        None
    };



    let connections = count_active_connections();

    let max_ram = Some(sys.total_memory());
    let cpu_cores = Some(sys.cpus().len() as i32);
    let cpu_model = sys.cpus().first().map(|c| c.brand().to_string());

    (latency, cpu, ram, connections, max_ram, cpu_cores, cpu_model)
}


fn count_active_connections() -> Option<u32> {
    // count_active_connections now filters for sing-box
    // Try using `ss` (Socket Statistics) - Linux standard
    // ss -t -n -p state established
    // We want to count lines containing "sing-box"
    if let Ok(output) = std::process::Command::new("ss")
        .args(&["-t", "-n", "-p", "state", "established"])
        .output() 
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Count lines containing "sing-box"
            let count = stdout.lines()
                .filter(|line| line.contains("\"sing-box\"") || line.contains("sing-box"))
                .count();
            return Some(count as u32);
        }
    }
    
    // Fallback: Read /proc/net/tcp (More robust if ss missing)
    // But harder to filter by process without scanning /proc/pid/fd
    // For now, if ss fails, we return 0 or None to avoid "4" fallback
    // Or just return total count but warn it's inaccurate?
    // Let's return None to indicate "Can't determine VPN users"
    None
}

async fn run_speed_test(client: &reqwest::Client) -> Option<i32> {
    // Download 25MB from Cloudflare
    let url = "http://speed.cloudflare.com/__down?bytes=25000000"; 
    let start = std::time::Instant::now();
    
    match client.get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await 
    {
        Ok(resp) => {
            if !resp.status().is_success() {
                return None;
            }
            // Stream the body to avoid loading all in RAM? 
            // Or just check time to first byte + transfer time.
            // For simple bandwidth check, reading bytes is better.
            if let Ok(bytes) = resp.bytes().await {
                let duration = start.elapsed().as_secs_f64();
                if duration < 0.1 { return None; } // Too fast?
                
                let bits = bytes.len() as f64 * 8.0;
                let mbps = (bits / duration) / 1_000_000.0;
                return Some(mbps as i32);
            }
        },
        Err(e) => {
             warn!("Speedtest download failed: {}", e);
        }
    }
    None
}

async fn start_neighbor_sniper(
    discoveries: std::sync::Arc<tokio::sync::Mutex<Vec<caramba_shared::DiscoveredSni>>>,
    mut scan_rx: tokio::sync::mpsc::Receiver<()>,
) {
    info!("🚀 Neighbor Sniper background loop started.");
    
    let local_ip = match get_local_ip() {
        Some(ip) => ip,
        None => {
            error!("❌ Could not determine local IP. Neighbor Sniper disabled.");
            return;
        }
    };

    let scanner = scanner::NeighborScanner::new(local_ip);

    loop {
        info!("🔍 Neighbor Sniper: Starting scan cycle...");
        let results = scanner.scan_subnet().await;
        
        if !results.is_empty() {
             let mut lock = discoveries.lock().await;
             lock.extend(results);
             info!("✨ Neighbor Sniper: Found {} potential SNIs.", lock.len());
        }

        // Wait for EITHER 1 hour OR a manual scan signal
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                info!("🕒 Neighbor Sniper: Scheduled hourly scan starting.");
            }
            _ = scan_rx.recv() => {
                info!("⚡ Neighbor Sniper: Manual scan signal received!");
            }
        }
    }
}

fn get_local_ip() -> Option<std::net::IpAddr> {
    // Try using UdpSocket trick
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}
async fn report_logs(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    config_path: &str,
) -> anyhow::Result<()> {
    let mut logs = std::collections::HashMap::new();
    let services = vec!["sing-box", "caramba-node", "nginx", "caddy"];

    for service in services {
        let output = std::process::Command::new("journalctl")
            .args(&["-u", service, "-n", "100", "--no-pager"])
            .output();

        match output {
            Ok(out) => {
                let content = String::from_utf8_lossy(&out.stdout).to_string();
                if !content.is_empty() {
                    logs.insert(service.to_string(), content);
                } else {
                    logs.insert(service.to_string(), "No logs found or service not installed.".to_string());
                }
            },
            Err(e) => {
                logs.insert(service.to_string(), format!("Failed to fetch logs: {}", e));
            }
        }
    }

    // Include config
    if let Ok(config_content) = tokio::fs::read_to_string(config_path).await {
        logs.insert("config.json".to_string(), config_content);
    }

    let url = format!("{}/api/v2/node/logs", panel_url);
    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&caramba_shared::api::LogResponse { logs })
        .send()
        .await?;

    if resp.status().is_success() {
        info!("✅ Logs reported successfully");
    } else {
        warn!("⚠️ Failed to report logs: {}", resp.status());
    }

    Ok(())
}
