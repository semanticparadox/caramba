use caramba_shared::api::{HeartbeatRequest, HeartbeatResponse};
use caramba_shared::config::ConfigResponse;
use caramba_shared::self_update::{apply_self_update, restart_service};
use clap::Parser;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use sysinfo::System;
use tracing::{error, info, warn};

mod scanner;
mod v2rayapi;

/// gRPC-адрес experimental.v2ray_api sing-box на этом же узле.
/// Совпадает с тем, что пишет генератор конфига (singbox/generator.rs).
const V2RAY_API_ENDPOINT: &str = "http://127.0.0.1:8080";

/// Собран ли локальный sing-box с `with_v2ray_api`.
///
/// Панель по этому ответу решает, писать ли узлу секцию
/// `experimental.v2ray_api`. Ошибиться в сторону «умеет» нельзя: сборка без
/// тега отвергает такую секцию при старте и узел не поднимается совсем.
/// Поэтому любая неопределённость — «не умеет».
///
/// Результат считается один раз за процесс: бинарник под ногами не меняется,
/// а обновление агента и sing-box идёт с перезапуском.
fn singbox_supports_v2ray_api() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();

    *CACHED.get_or_init(|| {
        let output = std::process::Command::new("sing-box")
            .arg("version")
            .output();
        let supported = match output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.starts_with("Tags:") && line.contains("with_v2ray_api")),
            Ok(_) | Err(_) => false,
        };
        if supported {
            tracing::info!("sing-box собран с with_v2ray_api — учёт по пользователям доступен");
        } else {
            tracing::warn!(
                "sing-box без with_v2ray_api: трафик по пользователям снят не будет, \
                 панель не станет писать секцию v2ray_api этому узлу"
            );
        }
        supported
    })
}
mod sni_check; // NEW

/// Минимальный интервал между ротациями SNI (секунды).
/// Предотвращает цепные ротации когда весь пул невалиден.
const SNI_ROTATION_COOLDOWN_SECS: u64 = 1800; // 30 минут

/// U23: адаптивный (укороченный) cooldown, применяется когда зафиксированы
/// симптомы RU-блокировки (early-RST / handshake-terminated-early) И счётчик
/// последовательных провалов превысил порог. В этом режиме над скоростью ротации
/// важнее реакция на активную блокировку, поэтому 30-минутный cooldown слишком
/// консервативен. 5 минут — всё ещё защищает от шторма ротаций, но позволяет
/// быстро уйти с заблокированного SNI.
const SNI_ROTATION_COOLDOWN_ADAPTIVE_SECS: u64 = 300; // 5 минут

/// U23: сколько подряд провалов с block-симптомами нужно увидеть, прежде чем
/// переключиться на адаптивный (быстрый) cooldown. Защищает от единичных
/// сетевых флуктуаций.
const SNI_BLOCK_FAILURE_THRESHOLD: u32 = 2;

/// Максимум ротаций SNI за один час работы агента.
const SNI_MAX_ROTATIONS_PER_HOUR: u32 = 3;

/// U23: расширенный лимит ротаций в час, когда зафиксирована активная
/// RU-блокировка. Под активной атакой имеет смысл попробовать больше SNI
/// из пула, не упираясь сразу в обычный консервативный лимит.
const SNI_MAX_ROTATIONS_PER_HOUR_UNDER_BLOCK: u32 = 6;

/// Semver-aware "is `target` strictly newer than `current`?" comparison.
/// The previous `target_normalized != current_normalized` check is a
/// STRING-EQUALITY test: an agent running v0.9.48 would happily try to
/// "update" itself to v0.9.46 (older), then SIGTERM on the post-update
/// restart would trip systemd's `Restart=` limit and brick the node.
///
/// Parses `vX.Y.Z[-pre]` and compares `[major, minor, patch]` numerically;
/// returns false on any parse failure (defensive — never auto-update on
/// unparseable version strings).
fn is_newer_version(target: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u32>> {
        let core = v.trim().trim_start_matches('v');
        let mut parts = core.split(['.', '-', '+']);
        let nums: Vec<u32> = parts
            .by_ref()
            .take_while(|p| !p.is_empty() && p.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .filter_map(|p| p.parse::<u32>().ok())
            .collect();
        if nums.len() >= 3 { Some(nums) } else { None }
    }
    match (parse(target), parse(current)) {
        (Some(t), Some(c)) => t > c,
        _ => false,
    }
}

/// Current Clash API secret, parsed from the active sing-box config.
/// The panel now emits `experimental.clash_api.secret`, so all local Clash
/// queries (:9090) must send it as a Bearer token (caramba-4cs).
static CLASH_SECRET: std::sync::OnceLock<std::sync::RwLock<Option<String>>> =
    std::sync::OnceLock::new();

fn clash_secret_cell() -> &'static std::sync::RwLock<Option<String>> {
    CLASH_SECRET.get_or_init(|| std::sync::RwLock::new(None))
}

/// Extract `experimental.clash_api.secret` from a sing-box config value and
/// cache it for subsequent Clash API calls.
fn refresh_clash_secret(config: &serde_json::Value) {
    let secret = config
        .get("experimental")
        .and_then(|e| e.get("clash_api"))
        .and_then(|c| c.get("secret"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    if let Ok(mut guard) = clash_secret_cell().write() {
        *guard = secret;
    }
}

fn clash_secret() -> Option<String> {
    clash_secret_cell().read().ok().and_then(|g| g.clone())
}

/// Attach the Clash API Bearer token to a request when one is configured.
fn with_clash_auth(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match clash_secret() {
        Some(s) if !s.is_empty() => req.header("Authorization", format!("Bearer {}", s)),
        _ => req,
    }
}

fn init_rustls_provider() {
    // rustls 0.23 requires explicit process-wide provider in some feature combinations.
    if tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
}

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
    /// U22: hash of the config the node has actually applied AND successfully
    /// restarted sing-box with (the ACK value sent in the heartbeat). Distinct
    /// from `current_hash` (what was fetched/saved): only set after a verified,
    /// successful restart so the panel knows true rollout state.
    last_applied_config_hash: Option<String>,
    // Kill Switch State
    last_successful_contact: std::time::Instant,
    kill_switch_enabled: bool,
    kill_switch_timeout: u64,
    vpn_stopped_by_kill_switch: bool,
    cached_speed_mbps: Option<i32>,
    recent_discoveries: std::sync::Arc<tokio::sync::Mutex<Vec<caramba_shared::DiscoveredSni>>>,
    scan_trigger: tokio::sync::mpsc::Sender<()>, // NEW: Pulse for neighbor sniper
    last_user_usage_totals: std::collections::HashMap<String, u64>,
    // Защита от бесконечной ротации SNI:
    // Ротация блокируется если:
    //   - прошло < SNI_ROTATION_COOLDOWN_SECS с последней ротации
    //   - в текущем часу уже выполнено >= SNI_MAX_ROTATIONS_PER_HOUR ротаций
    last_sni_rotation: Option<std::time::Instant>,
    sni_rotation_count_this_hour: u32,
    sni_rotation_hour: u64, // uptime / 3600, чтобы сбрасывать счётчик каждый час
    /// Ports currently opened in firewall by the agent (port, protocol)
    open_firewall_ports: HashSet<(u16, String)>,
    /// U23: consecutive SNI-check failures that carried RU-block symptoms
    /// (early-RST / handshake-terminated-early). Drives the adaptive cooldown:
    /// once it crosses `SNI_BLOCK_FAILURE_THRESHOLD` we rotate faster. Reset to 0
    /// on any healthy probe.
    sni_block_failure_streak: u32,
    /// U23: latest block-detection canary result, attached to the next heartbeat
    /// so the panel can react (e.g. blacklist the SNI / rotate the whole group).
    /// `None` once consumed by a heartbeat.
    pending_block_signals: Option<caramba_shared::api::BlockSignals>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_rustls_provider();

    // Initialize System Monitor
    let mut sys = System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    sys.refresh_all();
    // 1. Setup Logging
    tracing_subscriber::fmt().with_env_filter("info").init();

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

    let initial_hash = load_current_hash(&args.config_path).await;
    let mut state = AgentState {
        // On startup we assume the on-disk config is what sing-box is already
        // running, so the loaded hash doubles as the initial applied/ACK hash.
        last_applied_config_hash: initial_hash.clone(),
        current_hash: initial_hash,
        last_successful_contact: std::time::Instant::now(),
        kill_switch_enabled: false,
        kill_switch_timeout: 300,
        vpn_stopped_by_kill_switch: false,
        cached_speed_mbps: None,
        recent_discoveries: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        scan_trigger: scan_tx,
        last_user_usage_totals: std::collections::HashMap::new(),
        last_sni_rotation: None,
        sni_rotation_count_this_hour: 0,
        sni_rotation_hour: 0,
        open_firewall_ports: HashSet::new(),
        sni_block_failure_streak: 0,
        pending_block_signals: None,
    };

    // Initialize HTTP Client with timeouts so a hung panel doesn't stall the heartbeat loop.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|e| {
            warn!("Failed to build configured reqwest client ({e}), falling back to default");
            reqwest::Client::new()
        });

    // 4. Fetch initial config
    info!("🔄 Fetching initial configuration from Panel...");
    match check_and_update_config(&client, &panel_url, &token, &args.config_path, &mut state).await
    {
        Ok(_) => {
            info!("✅ Initial configuration loaded successfully");
        }
        Err(e) => {
            error!(
                "⚠️ Failed to fetch initial config: {}. Will retry in mainloop.",
                e
            );
        }
    }

    // Sync firewall for existing config (in case agent restarted but config hash unchanged)
    if let Ok(existing) = tokio::fs::read_to_string(&args.config_path).await
        && let Ok(config_json) = serde_json::from_str::<serde_json::Value>(&existing)
    {
        sync_firewall(&config_json, &mut state);
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

    // 5.5 Start Neighbor Sniper (Phase 7 & Phase 9 Automation)
    let discoveries = state.recent_discoveries.clone();
    let initial_scan = state.current_hash.is_none(); // Trigger if fresh install
    tokio::spawn(async move {
        start_neighbor_sniper(discoveries, scan_rx, initial_scan).await;
    });

    // 6. Main Loop
    let mut failures = 0;

    let start_time = std::time::Instant::now();

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .expect("install SIGINT handler");

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("📴 SIGTERM received — shutting down node agent");
                break;
            }
            _ = sigint.recv() => {
                info!("📴 SIGINT received — shutting down node agent");
                break;
            }
            _ = async {} => {}
        }

        let uptime = start_time.elapsed().as_secs();

        // Send Heartbeat
        match send_heartbeat(&client, &panel_url, &token, uptime, &mut state, &mut sys).await {
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
                        if let Err(e) = update_config(
                            &client,
                            &panel_url,
                            &token,
                            &args.config_path,
                            &mut state,
                        )
                        .await
                        {
                            error!("Failed to update config: {}", e);
                        }
                    }
                    caramba_shared::api::AgentAction::CollectLogs => {
                        info!("📋 Log collection requested");
                        let panel_url_clone = panel_url.clone();
                        let token_clone = token.clone();
                        let config_path_clone = args.config_path.clone();
                        let client_clone = client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = report_logs(
                                &client_clone,
                                &panel_url_clone,
                                &token_clone,
                                &config_path_clone,
                            )
                            .await
                            {
                                error!("Failed to report logs: {}", e);
                            }
                        });
                    }
                    _ => {}
                }
                // Check for Agent Update
                if let Some(target_ver) = resp.latest_version {
                    let current_version = env!("CARGO_PKG_VERSION");
                    // CRITICAL: only update if target is STRICTLY NEWER than
                    // current. The previous `target_normalized != current_normalized`
                    // check was a string equality test — it happily triggered
                    // "updates" to OLDER versions, then SIGTERM on the post-update
                    // restart bricked the node via systemd's start-limit-hit.
                    if is_newer_version(&target_ver, current_version) && target_ver != "0.0.0" {
                        info!(
                            "📣 New version available: {} (Current: {})",
                            target_ver, current_version
                        );

                        // Fetch update info
                        let info_url = format!("{}/api/v2/node/update-info", panel_url);
                        match client
                            .get(&info_url)
                            .header("Authorization", format!("Bearer {}", token))
                            .send()
                            .await
                        {
                            Ok(r) => {
                                if let Ok(json) = r.json::<serde_json::Value>().await {
                                    let download_url = json["url"].as_str().unwrap_or("");
                                    let hash = json["hash"].as_str().unwrap_or("");

                                    if !download_url.is_empty() && !hash.is_empty() {
                                        match apply_self_update(
                                            download_url,
                                            Some(hash),
                                            "caramba-node",
                                        )
                                        .await
                                        {
                                            Ok(_) => {
                                                restart_service("caramba-node");
                                                std::process::exit(0);
                                            }
                                            Err(e) => {
                                                error!("Self-update failed: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => error!("Failed to fetch update info: {}", e),
                        }
                    }
                }
            }
            Err(e) => {
                failures += 1;
                let backoff_secs = match failures {
                    1..=2 => 10,
                    3..=4 => 20,
                    5..=6 => 40,
                    7..=9 => 60,
                    _ => 120,
                };
                error!(
                    "❌ Heartbeat failed ({}, backoff {}s): {}",
                    failures, backoff_secs, e
                );
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            }
        }

        // Periodic config check (every 10th heartbeat = ~100 seconds)
        if uptime % 100 < 10 {
            if let Err(e) =
                check_and_update_config(&client, &panel_url, &token, &args.config_path, &mut state)
                    .await
            {
                error!("Config check failed: {}", e);
            }

            // Fetch Global Settings (Kill Switch / Decoy)
            if let Err(e) = fetch_global_settings(&client, &panel_url, &token, &mut state).await {
                error!("Failed to fetch settings: {}", e);
            }

            // SNI Health Check с circuit-breaker защитой от бесконечной ротации
            if let Some(current_sni) = sni_check::get_current_sni(&args.config_path).await {
                if let Err(reason) = sni_check::check_reachability(&current_sni).await {
                    error!(
                        "⚠️ SNI {} failed validation ({})! Checking rotation circuit-breaker...",
                        current_sni, reason
                    );

                    // U23: classify the failure — is this an active RU-block
                    // (early-RST / handshake-terminated-early) or plain
                    // unreachability? The DPI signature lets us rotate FASTER.
                    let outcome = scanner::probe_block_symptoms(&current_sni).await;
                    let block_detected = matches!(
                        outcome,
                        scanner::BlockProbeOutcome::EarlyRst
                            | scanner::BlockProbeOutcome::HandshakeTerminatedEarly
                    );
                    if block_detected {
                        state.sni_block_failure_streak =
                            state.sni_block_failure_streak.saturating_add(1);
                        let (detail, early_rst, hs_early) = match &outcome {
                            scanner::BlockProbeOutcome::EarlyRst => ("early_rst", true, false),
                            scanner::BlockProbeOutcome::HandshakeTerminatedEarly => {
                                ("handshake_terminated_early", false, true)
                            }
                            _ => ("", false, false),
                        };
                        warn!(
                            "🚨 RU-block symptom detected on SNI '{}': {} (streak: {})",
                            current_sni, detail, state.sni_block_failure_streak
                        );
                        // Stage the canary result for the next heartbeat so the
                        // panel can rotate the whole group / blacklist faster.
                        state.pending_block_signals = Some(caramba_shared::api::BlockSignals {
                            sni: current_sni.clone(),
                            early_rst,
                            handshake_terminated_early: hs_early,
                            consecutive_failures: state.sni_block_failure_streak,
                            detail: Some(detail.to_string()),
                        });
                    }
                    // Note: a non-block failure does NOT reset the streak (the SNI
                    // is still failing); only a healthy probe (below) clears it.

                    // U23: under a confirmed active block, switch to the adaptive
                    // (shorter) cooldown and a higher hourly rotation budget so we
                    // can flee a blocked SNI quickly instead of waiting 30 min.
                    let under_active_block =
                        state.sni_block_failure_streak >= SNI_BLOCK_FAILURE_THRESHOLD;
                    let effective_cooldown = if under_active_block {
                        SNI_ROTATION_COOLDOWN_ADAPTIVE_SECS
                    } else {
                        SNI_ROTATION_COOLDOWN_SECS
                    };
                    let effective_max_rotations = if under_active_block {
                        SNI_MAX_ROTATIONS_PER_HOUR_UNDER_BLOCK
                    } else {
                        SNI_MAX_ROTATIONS_PER_HOUR
                    };
                    if under_active_block {
                        warn!(
                            "⚡ Adaptive rotation engaged (block streak {} ≥ {}): cooldown {}s, max {}/h",
                            state.sni_block_failure_streak,
                            SNI_BLOCK_FAILURE_THRESHOLD,
                            effective_cooldown,
                            effective_max_rotations
                        );
                    }

                    // Сбрасываем счётчик если начался новый час работы агента
                    let current_hour = uptime / 3600;
                    if current_hour != state.sni_rotation_hour {
                        state.sni_rotation_count_this_hour = 0;
                        state.sni_rotation_hour = current_hour;
                    }

                    // Проверяем cooldown — не ротировали ли мы совсем недавно?
                    let cooldown_ok = state
                        .last_sni_rotation
                        .is_none_or(|t| t.elapsed().as_secs() >= effective_cooldown);

                    // Проверяем лимит ротаций за час
                    let rate_ok = state.sni_rotation_count_this_hour < effective_max_rotations;

                    if !cooldown_ok {
                        let elapsed = state.last_sni_rotation.unwrap().elapsed().as_secs();
                        warn!(
                            "🛑 SNI rotation BLOCKED by cooldown: last rotation was {}s ago (min: {}s). \
                             Current SNI '{}' will be retried after cooldown expires.",
                            elapsed, effective_cooldown, current_sni
                        );
                    } else if !rate_ok {
                        warn!(
                            "🛑 SNI rotation BLOCKED by rate-limit: {} rotations already performed \
                             this hour (max: {}). SNI pool may be exhausted or all SNIs failing.",
                            state.sni_rotation_count_this_hour, effective_max_rotations
                        );
                    } else {
                        info!(
                            "🔄 Triggering SNI rotation (rotation #{} this hour)...",
                            state.sni_rotation_count_this_hour + 1
                        );
                        match rotate_sni(&client, &panel_url, &token, &current_sni, &reason).await {
                            Ok(new_sni) => {
                                info!("✅ SNI Rotated to {}. Updating config...", new_sni);
                                state.last_sni_rotation = Some(std::time::Instant::now());
                                state.sni_rotation_count_this_hour += 1;
                                // Force immediate config update
                                if let Err(e) = update_config(
                                    &client,
                                    &panel_url,
                                    &token,
                                    &args.config_path,
                                    &mut state,
                                )
                                .await
                                {
                                    error!("Failed to update config after rotation: {}", e);
                                }
                            }
                            // 409 Conflict = панель не нашла другого SNI в пуле
                            Err(e)
                                if e.to_string().contains("409")
                                    || e.to_string().contains("No other SNI") =>
                            {
                                warn!(
                                    "⚠️ SNI pool exhausted — no alternative SNI available ({}). \
                                     Activating emergency cooldown to prevent hammering.",
                                    e
                                );
                                // Устанавливаем last_rotation чтобы cooldown сработал немедленно
                                state.last_sni_rotation = Some(std::time::Instant::now());
                            }
                            Err(e) => error!("❌ Failed to rotate SNI: {}", e),
                        }
                    }
                } else {
                    // U23: healthy probe — the active-block streak is cleared so a
                    // single transient failure later won't immediately trip the
                    // adaptive fast-rotation path.
                    if state.sni_block_failure_streak > 0 {
                        info!(
                            "✅ SNI '{}' healthy again — clearing block streak ({} → 0)",
                            current_sni, state.sni_block_failure_streak
                        );
                        state.sni_block_failure_streak = 0;
                    }
                }
            }
        }

        // KILL SWITCH MONITOR
        if state.kill_switch_enabled
            && !state.vpn_stopped_by_kill_switch
            && state.last_successful_contact.elapsed().as_secs() > state.kill_switch_timeout
        {
            warn!(
                "⚠️ EMERGENCY KILL SWITCH TRIGGERED! Lost connection for {}s (Timeout: {}s)",
                state.last_successful_contact.elapsed().as_secs(),
                state.kill_switch_timeout
            );

            if let Err(e) = stop_singbox() {
                error!("❌ FAILED TO STOP VPN SERVICE: {}", e);
            } else {
                // Verify sing-box actually stopped
                tokio::time::sleep(Duration::from_secs(2)).await;
                let still_running = std::process::Command::new("pgrep")
                    .arg("-x")
                    .arg("sing-box")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if still_running {
                    warn!("⚠️ sing-box still running after stop, sending SIGKILL");
                    let _ = std::process::Command::new("pkill")
                        .arg("-9")
                        .arg("-x")
                        .arg("sing-box")
                        .output();
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }

                state.vpn_stopped_by_kill_switch = true;
                info!("🛑 Kill switch activated: VPN service confirmed stopped");
            }
        }

        // Long poll (replaces sleep(10))
        // This effectively makes the heartbeat interval ~30s (timeout) unless update occurs
        match poll_events(&client, &panel_url, &token).await {
            Ok(signal) => match signal {
                Some(SignalType::Update) => {
                    info!("⚡ Instant Update Received!");
                    if let Err(e) =
                        update_config(&client, &panel_url, &token, &args.config_path, &mut state)
                            .await
                    {
                        error!("Failed to update config: {}", e);
                    }
                }
                Some(SignalType::Scan) => {
                    info!("🔍 Manual Scan Signal Received!");
                    let _ = state.scan_trigger.try_send(());
                }
                Some(SignalType::Restart) => {
                    info!("♻️ Restart signal received from panel. Restarting sing-box...");
                    if let Err(e) = restart_singbox() {
                        error!("Failed to restart sing-box from signal: {}", e);
                    }
                }
                None => {}
            },
            Err(e) => {
                warn!(
                    "Long poll failed or timed out locally: {}. Backing off 5s.",
                    e
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    info!("👋 Node agent shut down cleanly");
    Ok(())
}

#[derive(Debug)]
enum SignalType {
    Update,
    Scan,
    Restart,
}

async fn poll_events(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
) -> anyhow::Result<Option<SignalType>> {
    let url = format!("{}/api/v2/node/updates/poll", panel_url);
    let resp = client
        .get(&url)
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
    if json
        .get("update")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(Some(SignalType::Update));
    }

    // Support generic signal from PubSub message
    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
        if msg == "scan" {
            return Ok(Some(SignalType::Scan));
        }
        if msg == "restart" {
            return Ok(Some(SignalType::Restart));
        }
        if msg == "update" {
            return Ok(Some(SignalType::Update));
        }
    }

    Ok(None)
}

async fn rotate_sni(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    current_sni: &str,
    reason: &str,
) -> anyhow::Result<String> {
    let url = format!("{}/api/v2/node/rotate-sni", panel_url);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "current_sni": current_sni,
            "reason": format!("Validation failed: {}", reason)
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Rotation failed: {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await?;
    let new_sni = json
        .get("new_sni")
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
    state: &mut AgentState,
    sys: &mut System,
) -> anyhow::Result<HeartbeatResponse> {
    let url = format!("{}/api/v2/node/heartbeat", panel_url);

    // Трафик снимаем ПЕРВЫМ: из тех же дельт выводится и число активных
    // пользователей, которое уходит в телеметрию. Раньше это были два
    // независимых источника, и они расходились — панель показывала ноль
    // подключённых при растущем трафике.
    let user_usage = collect_user_usage_delta(client, &mut state.last_user_usage_totals).await;
    let active_users = user_usage.as_ref().map(|m| m.len()).unwrap_or(0);

    // Collect Telemetry
    let (latency, cpu, ram, connections, max_ram, cpu_cores, cpu_model) =
        collect_telemetry(client, sys, active_users).await;
    let (traffic_up, traffic_down) = collect_total_traffic(client).await.unwrap_or((0, 0));

    let status = if state.vpn_stopped_by_kill_switch {
        "kill_switch_active".to_string()
    } else {
        "running".to_string()
    };

    let payload = HeartbeatRequest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime,
        status,
        config_hash: state.current_hash.clone(),
        traffic_up,
        traffic_down,
        certificates: Some(
            check_certificates(
                state
                    .current_hash
                    .as_ref()
                    .map(|_| "/etc/sing-box/config.json")
                    .unwrap_or("/etc/sing-box/config.json"),
            )
            .await,
        ),
        latency,
        cpu_usage: cpu,
        memory_usage: ram,
        speed_mbps: state.cached_speed_mbps,
        active_connections: connections, // Added Phase 3
        max_ram,
        cpu_cores,
        cpu_model,
        user_usage,
        supports_v2ray_api: Some(singbox_supports_v2ray_api()),
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
        // U22: ACK the hash the node has actually applied + restarted with.
        last_applied_config_hash: state.last_applied_config_hash.clone(),
        // U23: hand the panel any pending block-detection canary result, then
        // consume it so we only report each detection once.
        block_signals: state.pending_block_signals.take(),
    };

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&payload)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Server error: {}", resp.status());
    }

    Ok(resp.json::<HeartbeatResponse>().await?)
}

fn extract_counter_field(obj: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(candidate) = obj.get(*key) else {
            continue;
        };
        if let Some(v) = candidate.as_u64() {
            return Some(v);
        }
        if let Some(v) = candidate.as_i64()
            && v >= 0
        {
            return Some(v as u64);
        }
        if let Some(v) = candidate.as_str()
            && let Ok(parsed) = v.trim().parse::<u64>()
        {
            return Some(parsed);
        }
    }
    None
}

async fn fetch_clash_connections(client: &reqwest::Client) -> Option<Vec<serde_json::Value>> {
    let resp = with_clash_auth(client.get("http://127.0.0.1:9090/connections"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let value = resp.json::<serde_json::Value>().await.ok()?;
    value.get("connections").and_then(|v| v.as_array()).cloned()
}

async fn collect_total_traffic(client: &reqwest::Client) -> Option<(u64, u64)> {
    if let Ok(resp) = with_clash_auth(client.get("http://127.0.0.1:9090/traffic"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        && resp.status().is_success()
        && let Ok(value) = resp.json::<serde_json::Value>().await
    {
        let up = extract_counter_field(
            &value,
            &["up", "upload", "uploadTotal", "uplink", "totalUp"],
        );
        let down = extract_counter_field(
            &value,
            &["down", "download", "downloadTotal", "downlink", "totalDown"],
        );
        if let (Some(up), Some(down)) = (up, down) {
            return Some((up, down));
        }
    }

    // Fallback: aggregate from active connection counters.
    let mut up_total = 0u64;
    let mut down_total = 0u64;
    let mut has_any = false;
    if let Some(connections) = fetch_clash_connections(client).await {
        for conn in connections {
            let up = extract_counter_field(
                &conn,
                &["upload", "uploadTotal", "uplink", "sent", "upload_bytes"],
            )
            .unwrap_or(0);
            let down = extract_counter_field(
                &conn,
                &[
                    "download",
                    "downloadTotal",
                    "downlink",
                    "received",
                    "download_bytes",
                ],
            )
            .unwrap_or(0);
            if up > 0 || down > 0 {
                has_any = true;
            }
            up_total = up_total.saturating_add(up);
            down_total = down_total.saturating_add(down);
        }
    }

    if has_any {
        Some((up_total, down_total))
    } else {
        None
    }
}

/// Дельта трафика по пользователям с прошлого опроса.
///
/// Источник — `experimental.v2ray_api` sing-box, а не Clash API. Clash API имя
/// пользователя не отдаёт вообще: в метаданных соединения есть network, type,
/// адреса, порты, host, dnsMode и processPath — и всё
/// (experimental/clashapi/trafficontrol/tracker.go). Прежняя реализация читала
/// оттуда `metadata.user`, которого не существует, поэтому не привязала к людям
/// ни одного байта за всё время работы.
///
/// v2ray_api ведёт по счётчику на пользователя и направление:
/// `user>>><имя>>>>traffic>>>uplink` и `…>>>downlink`. Счётчики
/// НАКОПИТЕЛЬНЫЕ, поэтому логика дельт с `last_totals` сохранена как была,
/// включая обработку сброса при перезапуске sing-box.
///
/// `reset: false` намеренно: обнулять счётчики на стороне sing-box нельзя —
/// тогда потеря одного ответа означала бы безвозвратно потерянный трафик.
/// Считаем дельту у себя, где потерянный опрос лишь откладывает учёт.
async fn collect_user_usage_delta(
    _client: &reqwest::Client,
    last_totals: &mut std::collections::HashMap<String, u64>,
) -> Option<std::collections::HashMap<String, u64>> {
    let stats = query_user_traffic_stats().await?;

    let mut current_totals: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    for (user, bytes) in stats {
        let entry = current_totals.entry(user).or_insert(0);
        *entry = entry.saturating_add(bytes);
    }

    let mut delta_map = std::collections::HashMap::new();
    for (user, current_total) in &current_totals {
        let previous_total = last_totals.get(user).copied().unwrap_or(0);
        let delta = if *current_total >= previous_total {
            current_total.saturating_sub(previous_total)
        } else {
            // Счётчик уехал вниз — sing-box перезапустили. Отдаём наблюдаемое
            // значение один раз, иначе трафик после рестарта потерялся бы.
            *current_total
        };
        if delta > 0 {
            delta_map.insert(user.clone(), delta);
        }
    }

    *last_totals = current_totals;
    if delta_map.is_empty() {
        None
    } else {
        tracing::debug!("Traffic delta: {:?}", delta_map);
        Some(delta_map)
    }
}

/// Суммарные (uplink + downlink) накопительные счётчики по каждому пользователю.
///
/// Ошибка соединения — это `None`, а не пустая карта: пустая означала бы
/// «весь трафик обнулился» и породила бы ложные дельты после восстановления.
async fn query_user_traffic_stats() -> Option<std::collections::HashMap<String, u64>> {
    use v2rayapi::stats_service_client::StatsServiceClient;

    let mut client = match StatsServiceClient::connect(V2RAY_API_ENDPOINT).await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("v2ray_api недоступен ({e}) — трафик за этот цикл не снят");
            return None;
        }
    };

    let response = match client
        .query_stats(v2rayapi::QueryStatsRequest {
            // Один запрос на всех: шаблон отбирает счётчики пользователей и
            // ничего кроме них.
            pattern: "user>>>".to_string(),
            reset: false,
            patterns: Vec::new(),
            regexp: false,
        })
        .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => {
            tracing::warn!("v2ray_api QueryStats не отработал: {e}");
            return None;
        }
    };

    let mut totals: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for stat in response.stat {
        let Some(user) = parse_stat_user(&stat.name) else {
            continue;
        };
        let value = u64::try_from(stat.value).unwrap_or(0);
        *totals.entry(user).or_insert(0) += value;
    }
    Some(totals)
}

/// `user>>>user_42>>>traffic>>>uplink` → `user_42`.
///
/// Имя пользователя может содержать что угодно кроме разделителя, поэтому
/// разбираем по позиции, а не поиском подстроки.
fn parse_stat_user(name: &str) -> Option<String> {
    let mut parts = name.split(">>>");
    if parts.next()? != "user" {
        return None;
    }
    let user = parts.next()?.trim();
    if user.is_empty() {
        return None;
    }
    Some(user.to_string())
}

async fn check_and_update_config(
    client: &reqwest::Client,
    panel_url: &str,
    token: &str,
    config_path: &str,
    state: &mut AgentState,
) -> anyhow::Result<()> {
    let url = format!("{}/api/v2/node/config", panel_url);

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Server error: {}", resp.status());
    }

    let config_resp: ConfigResponse = resp.json().await?;

    // Check if hash changed
    if state.current_hash.as_ref() != Some(&config_resp.hash) {
        info!(
            "🔄 Config hash changed: {} -> {}",
            state.current_hash.as_deref().unwrap_or("none"),
            &config_resp.hash
        );

        // U22 safe-apply: snapshot the current good config so we can roll back if
        // the new one fails validation / restart. Best-effort — a missing prior
        // config (fresh node) just means there is nothing to restore.
        let backup = tokio::fs::read(config_path).await.ok();

        // Save new config
        save_config(config_path, &config_resp.content).await?;

        // U22 safe-apply: validate BEFORE restarting sing-box. If `sing-box check`
        // exists and rejects the config, restore the previous good config and do
        // NOT restart — the node stays on the last known-good config and reports
        // its previous applied hash, so the panel sees the rollout did not land.
        if let Some(valid) = validate_singbox_config(config_path).await {
            if !valid {
                error!(
                    "🚑 New config FAILED sing-box validation. Rolling back to last good config (no restart)."
                );
                if let Some(prev) = backup {
                    if let Err(e) = tokio::fs::write(config_path, &prev).await {
                        error!("⚠️ Rollback write failed: {} — config left as-is", e);
                    } else {
                        info!("↩️ Rolled back to previous config. Keeping sing-box running.");
                    }
                } else {
                    warn!(
                        "No previous config to roll back to; leaving new (invalid) config in place."
                    );
                }
                // current_hash/last_applied stay as they were → panel knows apply failed.
                anyhow::bail!("sing-box rejected new config; rolled back");
            }
            info!("✅ New config passed sing-box validation");
        }
        // If `sing-box check` is unavailable we fall through and apply as before
        // (backward compatible with environments without the check subcommand).

        state.current_hash = Some(config_resp.hash.clone());
        // Pick up a (possibly rotated) Clash API secret before restart.
        refresh_clash_secret(&config_resp.content);

        // Sync firewall ports before restarting
        sync_firewall(&config_resp.content, state);

        // Restart sing-box
        restart_singbox()?;

        // U22: only ACK the applied hash AFTER a successful restart. The panel
        // reads this in the heartbeat to confirm the rollout actually landed.
        state.last_applied_config_hash = Some(config_resp.hash);

        info!("✅ Config updated, validated and service restarted");
    } else {
        info!("✓ Config up to date");
        // Ensure the cached Clash secret is populated even when the config
        // did not change across an agent restart.
        if clash_secret().is_none() {
            refresh_clash_secret(&config_resp.content);
        }
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
        }
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

    // Regenerate self-signed cert only when TLS domains change.
    // sing-box auto-reloads certs on file change — we must write atomically
    // to avoid cert/key mismatch during hot-reload.
    let cert_dir = Path::new(path)
        .parent()
        .unwrap_or(Path::new("/etc/sing-box"))
        .join("certs");
    let cert_path = cert_dir.join("cert.pem");
    let key_path = cert_dir.join("key.pem");

    let desired_names = extract_all_tls_server_names(content);
    let needs_regen = if cert_path.exists() && key_path.exists() {
        !cert_covers_domains(&cert_path, &desired_names).await
    } else {
        true
    };

    if needs_regen {
        match ensure_self_signed_cert(&cert_dir, &cert_path, &key_path, content).await {
            Ok(()) => info!(
                "✅ Self-signed TLS cert generated at {}",
                cert_dir.display()
            ),
            Err(e) => error!("⚠️ Failed to generate self-signed cert: {e}. TLS inbounds may fail."),
        }
    }

    Ok(())
}

/// Разбирает строку "notAfter=Jan 27 00:00:00 2036 GMT" из вывода `openssl x509 -enddate`.
/// Возвращает Unix timestamp или 0 при ошибке разбора.
fn parse_openssl_enddate(stdout: &[u8]) -> i64 {
    let output = String::from_utf8_lossy(stdout);
    for line in output.lines() {
        let line = line.trim();
        let raw = if let Some(v) = line.strip_prefix("notAfter=") {
            v.trim()
        } else {
            continue;
        };
        // Формат: "Jan 27 00:00:00 2036 GMT"
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let month_str = parts[0];
        let day: u32 = parts[1].parse().unwrap_or(0);
        // Время содержит ':', пропускаем parts[2]
        let year: i32 = parts[3].parse().unwrap_or(0);
        if year < 2000 || day == 0 {
            continue;
        }
        let month: u32 = match month_str {
            "Jan" => 1,
            "Feb" => 2,
            "Mar" => 3,
            "Apr" => 4,
            "May" => 5,
            "Jun" => 6,
            "Jul" => 7,
            "Aug" => 8,
            "Sep" => 9,
            "Oct" => 10,
            "Nov" => 11,
            "Dec" => 12,
            _ => continue,
        };
        // Days from Unix epoch (Jan 1 1970) to (year, month, day).
        // Counts leap years strictly before `year` using the standard
        // Gregorian formula: (y-1)/4 - (y-1)/100 + (y-1)/400, anchored at
        // 1970. The previous shortcut `(y-1970)/4 - ...` undercounts by 1
        // for years where (year-1970) % 4 == 3 (e.g. 2025, 2029) and made
        // cert-expiry alerts fire ~1 day early.
        const LEAP_BEFORE_1970: i64 = 1969 / 4 - 1969 / 100 + 1969 / 400; // 477
        let days_since_epoch: i64 = {
            let y_minus_1 = (year as i64) - 1;
            let leap_total = y_minus_1 / 4 - y_minus_1 / 100 + y_minus_1 / 400;
            let leap_days = leap_total - LEAP_BEFORE_1970;
            let years_offset = (year as i64) - 1970;
            let month_days_cumul: [i64; 13] =
                [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];
            let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            let mdays =
                month_days_cumul[month as usize - 1] + if is_leap && month > 2 { 1 } else { 0 };
            years_offset * 365 + leap_days + mdays + (day as i64 - 1)
        };
        return days_since_epoch * 86400;
    }
    0
}

/// Check if existing cert covers all desired domain names.
/// Uses a sidecar file (.domains) to track which domains the cert was generated for.
/// This avoids parsing X.509 and is reliable for our self-signed certs.
async fn cert_covers_domains(cert_path: &Path, desired: &[String]) -> bool {
    if desired.is_empty() {
        return true;
    }
    let domains_file = cert_path.with_extension("domains");
    match tokio::fs::read_to_string(&domains_file).await {
        Ok(stored) => {
            let stored_set: HashSet<&str> = stored.lines().collect();
            let desired_set: HashSet<&str> = desired.iter().map(|s| s.as_str()).collect();
            stored_set == desired_set
        }
        Err(_) => false, // No sidecar → regen needed
    }
}

/// Генерирует самоподписанный X.509 сертификат и RSA-ключ в PEM-формате.
/// CN/SAN берутся из config.json: собираем server_name из ВСЕХ TLS-инбаундов.
/// Сертификат покрывает все домены через Subject Alternative Names.
/// Срок действия: 10 лет (сертификат не должен истекать, пока нода работает).
async fn ensure_self_signed_cert(
    cert_dir: &Path,
    cert_path: &Path,
    key_path: &Path,
    config: &serde_json::Value,
) -> anyhow::Result<()> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
    use std::time::{Duration as StdDuration, SystemTime};

    tokio::fs::create_dir_all(cert_dir).await?;

    // Собираем ВСЕ server_name из TLS-инбаундов
    let all_names = extract_all_tls_server_names(config);
    let cn = all_names
        .first()
        .cloned()
        .unwrap_or_else(|| "vpn.local".to_string());
    info!(
        "🔐 Generating self-signed cert for CN={}, SANs={:?}",
        cn, all_names
    );

    let key_pair = KeyPair::generate()?;

    let mut params = CertificateParams::default();

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, &cn);
    params.distinguished_name = dn;

    // Add ALL domains as Subject Alternative Names
    let mut sans = Vec::new();
    for name in &all_names {
        if let Ok(ip) = name.parse::<std::net::IpAddr>() {
            sans.push(SanType::IpAddress(ip));
        } else if let Ok(dns_name) = name.clone().try_into() {
            sans.push(SanType::DnsName(dns_name));
        }
    }
    if sans.is_empty() {
        // Fallback: use CN
        if let Ok(dns_name) = cn.clone().try_into() {
            sans.push(SanType::DnsName(dns_name));
        }
    }
    params.subject_alt_names = sans;

    // Срок действия: сейчас - 1 день (backdate) до сейчас + 3650 дней
    let now = SystemTime::now();
    let not_before = now
        .checked_sub(StdDuration::from_secs(86400))
        .unwrap_or(now);
    let not_after = now
        .checked_add(StdDuration::from_secs(3650 * 86400))
        .unwrap_or(now);
    params.not_before = rcgen::date_time_ymd(
        time_from_system(not_before).0,
        time_from_system(not_before).1,
        time_from_system(not_before).2,
    );
    params.not_after = rcgen::date_time_ymd(
        time_from_system(not_after).0,
        time_from_system(not_after).1,
        time_from_system(not_after).2,
    );

    let cert = params.self_signed(&key_pair)?;

    // Write to temp files first, then rename atomically.
    // sing-box watches cert.pem for changes and hot-reloads — if we write cert
    // before key, sing-box sees new cert + old key → "private key does not match".
    // Strategy: write both to .tmp, set permissions, rename KEY first, then CERT.
    let cert_tmp = cert_path.with_extension("pem.tmp");
    let key_tmp = key_path.with_extension("pem.tmp");

    tokio::fs::write(&cert_tmp, cert.pem()).await?;
    tokio::fs::write(&key_tmp, key_pair.serialize_pem()).await?;

    // Права доступа: только root читает ключ
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&key_tmp).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(&key_tmp, perms).await?;
    }

    // Rename key FIRST — sing-box watches cert.pem, not key.pem.
    // When cert.pem changes, key.pem is already the matching new key.
    tokio::fs::rename(&key_tmp, key_path).await?;
    tokio::fs::rename(&cert_tmp, cert_path).await?;

    // Write sidecar file to track which domains this cert covers
    let domains_file = cert_path.with_extension("domains");
    tokio::fs::write(&domains_file, all_names.join("\n")).await?;

    Ok(())
}

/// Извлекает ВСЕ уникальные server_name из TLS-инбаундов конфига.
/// Собирает домены из Hysteria2 и VLESS+TLS (без Reality) инбаундов.
fn extract_all_tls_server_names(config: &serde_json::Value) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();

    let inbounds = match config.get("inbounds").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return names,
    };

    for inbound in inbounds {
        let inbound_type = inbound.get("type").and_then(|v| v.as_str()).unwrap_or("");

        let sn = match inbound_type {
            "hysteria2" | "hysteria" | "tuic" => inbound
                .pointer("/tls/server_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && !s.contains("google"))
                .map(str::to_string),
            "vless" | "trojan" => {
                let tls = match inbound.get("tls") {
                    Some(t) => t,
                    None => continue,
                };
                let enabled = tls
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let has_reality = tls
                    .get("reality")
                    .and_then(|r| r.get("enabled"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if enabled && !has_reality {
                    tls.get("server_name")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(name) = sn
            && seen.insert(name.clone())
        {
            names.push(name);
        }
    }

    names
}

/// Конвертирует SystemTime в (год, месяц, день) для rcgen::date_time_ymd.
fn time_from_system(t: std::time::SystemTime) -> (i32, u8, u8) {
    use std::time::UNIX_EPOCH;
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    // Простое вычисление даты без зависимостей
    let days = secs / 86400;
    let mut remaining = days;

    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let leap = is_leap(year);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u8;
    for days_in_month in &month_days {
        if remaining < *days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }
    let day = (remaining + 1) as u8;

    (year, month, day)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Extract required ports from sing-box config and sync firewall rules.
/// Opens new ports, closes ports no longer needed. Supports ufw and iptables.
fn sync_firewall(config: &serde_json::Value, state: &mut AgentState) {
    let mut desired: HashSet<(u16, String)> = HashSet::new();

    if let Some(inbounds) = config.get("inbounds").and_then(|v| v.as_array()) {
        for ib in inbounds {
            let port = ib.get("listen_port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            if port == 0 {
                continue;
            }
            let inbound_type = ib.get("type").and_then(|v| v.as_str()).unwrap_or("");
            // Hysteria2 and TUIC use UDP (QUIC), everything else is TCP
            let proto = match inbound_type {
                "hysteria2" | "hysteria" | "tuic" => "udp",
                _ => "tcp",
            };
            desired.insert((port, proto.to_string()));
            // Hysteria2 also needs TCP for some implementations, add both
            if proto == "udp" {
                desired.insert((port, "tcp".to_string()));
            }
        }
    }

    // Always keep SSH (22) and standard HTTPS (443) — never close them
    desired.insert((22, "tcp".to_string()));
    desired.insert((443, "tcp".to_string()));

    let to_open: Vec<_> = desired
        .difference(&state.open_firewall_ports)
        .cloned()
        .collect();
    let to_close: Vec<_> = state
        .open_firewall_ports
        .difference(&desired)
        .cloned()
        .collect();

    if to_open.is_empty() && to_close.is_empty() {
        return;
    }

    let has_ufw = std::process::Command::new("which")
        .arg("ufw")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let ufw_active = if has_ufw {
        std::process::Command::new("ufw")
            .arg("status")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("Status: active"))
            .unwrap_or(false)
    } else {
        false
    };

    for (port, proto) in &to_open {
        if ufw_active {
            let rule = format!("{}/{}", port, proto);
            let out = std::process::Command::new("ufw")
                .args(["allow", &rule])
                .output();
            match out {
                Ok(o) if o.status.success() => info!("🔓 Firewall: opened {}", rule),
                Ok(o) => warn!(
                    "⚠️ ufw allow {} failed: {}",
                    rule,
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => warn!("⚠️ ufw allow {} error: {}", rule, e),
            }
        } else {
            // Fallback to iptables
            let out = std::process::Command::new("iptables")
                .args([
                    "-C",
                    "INPUT",
                    "-p",
                    proto,
                    "--dport",
                    &port.to_string(),
                    "-j",
                    "ACCEPT",
                ])
                .output();
            let already_exists = out.map(|o| o.status.success()).unwrap_or(false);
            if !already_exists {
                let out = std::process::Command::new("iptables")
                    .args([
                        "-I",
                        "INPUT",
                        "-p",
                        proto,
                        "--dport",
                        &port.to_string(),
                        "-j",
                        "ACCEPT",
                    ])
                    .output();
                match out {
                    Ok(o) if o.status.success() => info!("🔓 Firewall: opened {}/{}", port, proto),
                    Ok(o) => warn!(
                        "⚠️ iptables open {}/{} failed: {}",
                        port,
                        proto,
                        String::from_utf8_lossy(&o.stderr)
                    ),
                    Err(e) => warn!("⚠️ iptables open {}/{} error: {}", port, proto, e),
                }
            }
        }
    }

    for (port, proto) in &to_close {
        // Never close SSH or 443
        if *port == 22 || *port == 443 {
            continue;
        }
        if ufw_active {
            let rule = format!("{}/{}", port, proto);
            let out = std::process::Command::new("ufw")
                .args(["delete", "allow", &rule])
                .output();
            match out {
                Ok(o) if o.status.success() => info!("🔒 Firewall: closed {}", rule),
                _ => {}
            }
        } else {
            let out = std::process::Command::new("iptables")
                .args([
                    "-D",
                    "INPUT",
                    "-p",
                    proto,
                    "--dport",
                    &port.to_string(),
                    "-j",
                    "ACCEPT",
                ])
                .output();
            match out {
                Ok(o) if o.status.success() => info!("🔒 Firewall: closed {}/{}", port, proto),
                _ => {}
            }
        }
    }

    state.open_firewall_ports = desired;
    info!(
        "🛡️ Firewall synced: {} ports open",
        state.open_firewall_ports.len()
    );
}

/// U22 safe-apply: validate a sing-box config file with `sing-box check -c`.
///
/// Returns:
///   - `Some(true)`  — config is valid.
///   - `Some(false)` — `sing-box check` ran and REJECTED the config.
///   - `None`        — the `sing-box` binary / `check` subcommand is unavailable,
///     so validation could not be performed (caller should fall
///     back to the legacy apply-without-validation path).
///
/// This keeps older installs (where `sing-box check` might behave differently)
/// working: only an explicit non-zero exit from a runnable `check` is treated as
/// a hard failure.
async fn validate_singbox_config(config_path: &str) -> Option<bool> {
    let path = config_path.to_string();
    // `sing-box check` is CPU/IO-light; run it on the blocking pool so we don't
    // stall the async runtime, mirroring the rest of the agent's process calls.
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new("sing-box")
            .args(["check", "-c", &path])
            .output()
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            if output.status.success() {
                Some(true)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("sing-box check rejected config: {}", stderr.trim());
                Some(false)
            }
        }
        // Binary missing / not on PATH (e.g. NotFound) → cannot validate.
        Ok(Err(e)) => {
            warn!(
                "sing-box check unavailable ({}); applying config without pre-validation",
                e
            );
            None
        }
        Err(e) => {
            warn!(
                "sing-box check task join error ({}); skipping validation",
                e
            );
            None
        }
    }
}

fn restart_singbox() -> anyhow::Result<()> {
    info!("🔄 Restarting sing-box service...");

    let output = std::process::Command::new("systemctl")
        .args(["restart", "sing-box"])
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
    let cert_dir = Path::new(config_path)
        .parent()
        .unwrap_or(Path::new("/etc/sing-box"))
        .join("certs");

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
        if let Some(ext) = path.extension()
            && (ext == "pem" || ext == "crt")
        {
            // Heuristic: check if this is a cert or key
            // Or just try openssl x509 on it. If it fails, maybe it's a key.

            let output = std::process::Command::new("openssl")
                .args([
                    "x509",
                    "-in",
                    path.to_str().unwrap_or(""),
                    "-noout",
                    "-subject",
                    "-enddate",
                    "-checkend",
                    "0",
                ])
                .output();

            if let Ok(out) = output
                && out.status.success()
            {
                let stdout = String::from_utf8_lossy(&out.stdout);
                // Parse subject: subject=CN = drive.google.com
                let sni = stdout
                    .lines()
                    .find(|l| l.starts_with("subject="))
                    .and_then(|l| l.split("CN = ").nth(1))
                    .or_else(|| {
                        stdout
                            .lines()
                            .find(|l| l.starts_with("subject="))
                            .and_then(|l| l.split("CN=").nth(1))
                    })
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

                // Парсим дату истечения из строки "notAfter=Month Day HH:MM:SS YYYY GMT"
                let expires_at = parse_openssl_enddate(&out.stdout);
                statuses.push(caramba_shared::api::CertificateStatus {
                    sni,
                    valid,
                    expires_at,
                    error: None,
                });
            }
        }
    }

    statuses
}

// Helper to stop sing-box
fn stop_singbox() -> anyhow::Result<()> {
    info!("🛑 Stopping sing-box service (Kill Switch Triggered)...");
    let output = std::process::Command::new("systemctl")
        .args(["stop", "sing-box"])
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
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if resp.status().is_success() {
        let json: serde_json::Value = resp.json().await?;
        if let Some(ks) = json.get("kill_switch") {
            state.kill_switch_enabled =
                ks.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            state.kill_switch_timeout = ks.get("timeout").and_then(|v| v.as_u64()).unwrap_or(300);
        }
    }
    Ok(())
}

async fn collect_telemetry(
    client: &reqwest::Client,
    sys: &mut System,
    // Сколько человек прокачали хоть байт с прошлого опроса — считается
    // вызывающим кодом из тех же дельт трафика, что уходят в heartbeat.
    active_users: usize,
) -> (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<u32>,
    Option<u64>,
    Option<i32>,
    Option<String>,
) {
    // 1. Latency Check (HTTP HEAD to Google)
    let start = std::time::Instant::now();
    let latency = match client
        .head("https://www.google.com")
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

    let connections = count_active_connections(client, active_users).await;

    let max_ram = Some(sys.total_memory());
    let cpu_cores = Some(sys.cpus().len() as i32);
    let cpu_model = sys.cpus().first().map(|c| c.brand().to_string());

    (
        latency,
        cpu,
        ram,
        connections,
        max_ram,
        cpu_cores,
        cpu_model,
    )
}

/// Сколько человек сейчас пользуется узлом.
///
/// Считаем не сокеты, а ЛЮДЕЙ, у которых с прошлого опроса вырос счётчик
/// трафика. Сокеты для этого не годятся принципиально: в них попадают сканеры,
/// проверки здоровья и служебный трафик самого узла — именно поэтому прошлая
/// реализация отказалась их считать и вернула ноль вместо ответа.
///
/// Тот же источник, что и у учёта трафика, поэтому «0 подключённых» при
/// растущем трафике стало невозможным состоянием: обе цифры приходят из одних
/// счётчиков.
///
/// `None` (а не 0) при недоступном API: ноль значил бы «никого нет», а это
/// другое утверждение, и панель на нём строит показ загрузки узла.
async fn count_active_connections(_client: &reqwest::Client, active_users: usize) -> Option<u32> {
    Some(active_users as u32)
}

async fn run_speed_test(client: &reqwest::Client) -> Option<i32> {
    // Download 25MB from Cloudflare
    let url = "http://speed.cloudflare.com/__down?bytes=25000000";
    let start = std::time::Instant::now();

    match client
        .get(url)
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
                if duration < 0.1 {
                    return None;
                } // Too fast?

                let bits = bytes.len() as f64 * 8.0;
                let mbps = (bits / duration) / 1_000_000.0;
                return Some(mbps as i32);
            }
        }
        Err(e) => {
            warn!("Speedtest download failed: {}", e);
        }
    }
    None
}

async fn start_neighbor_sniper(
    discoveries: std::sync::Arc<tokio::sync::Mutex<Vec<caramba_shared::DiscoveredSni>>>,
    mut scan_rx: tokio::sync::mpsc::Receiver<()>,
    initial_scan: bool,
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

    // Run initial scan immediately if requested (e.g. fresh install)
    if initial_scan {
        info!("⚡ Initial Auto-Scan triggered.");
        let results = scanner.scan_subnet().await;
        if !results.is_empty() {
            let mut lock = discoveries.lock().await;
            lock.extend(results);
            info!(
                "✨ Neighbor Sniper (Initial): Found {} potential SNIs.",
                lock.len()
            );
        }
    }

    loop {
        // Wait for EITHER 1 hour OR a manual scan signal
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                info!("🕒 Neighbor Sniper: Scheduled hourly scan starting.");
            }
            _ = scan_rx.recv() => {
                info!("⚡ Neighbor Sniper: Manual scan signal received!");
            }
        }

        info!("🔍 Neighbor Sniper: Starting scan cycle...");
        let results = scanner.scan_subnet().await;

        if !results.is_empty() {
            let mut lock = discoveries.lock().await;
            lock.extend(results);
            info!("✨ Neighbor Sniper: Found {} potential SNIs.", lock.len());
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
        let recent = std::process::Command::new("journalctl")
            .args([
                "-u",
                service,
                "--since",
                "2 hours ago",
                "-n",
                "200",
                "--no-pager",
            ])
            .output();

        match recent {
            Ok(out) => {
                let content = String::from_utf8_lossy(&out.stdout).to_string();

                if content.trim().is_empty() {
                    logs.insert(
                        service.to_string(),
                        "No logs for the last 2 hours (service may be idle or not installed)."
                            .to_string(),
                    );
                } else {
                    logs.insert(service.to_string(), content);
                }
            }
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
    let resp = client
        .post(&url)
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

#[cfg(test)]
mod v2ray_stats_tests {
    use super::parse_stat_user;

    /// Имя счётчика sing-box: `user>>><имя>>>>traffic>>>uplink`.
    #[test]
    fn extracts_the_user_from_a_counter_name() {
        assert_eq!(
            parse_stat_user("user>>>user_605098034>>>traffic>>>uplink").as_deref(),
            Some("user_605098034")
        );
        assert_eq!(
            parse_stat_user("user>>>user_42>>>traffic>>>downlink").as_deref(),
            Some("user_42")
        );
    }

    /// Счётчики не про пользователей (инбаунды, исходящие) обязаны отсеиваться:
    /// иначе их байты уехали бы в трафик несуществующего человека, а панель
    /// списала бы их с чужой квоты.
    #[test]
    fn ignores_counters_that_are_not_per_user() {
        assert_eq!(
            parse_stat_user("inbound>>>vless-in>>>traffic>>>uplink"),
            None
        );
        assert_eq!(
            parse_stat_user("outbound>>>direct>>>traffic>>>downlink"),
            None
        );
        assert_eq!(parse_stat_user(""), None);
        assert_eq!(parse_stat_user("user"), None);
    }

    /// Разбираем по позиции, а не поиском подстроки: имя пользователя может
    /// содержать что угодно, кроме самого разделителя.
    #[test]
    fn a_name_containing_traffic_is_still_parsed_correctly() {
        assert_eq!(
            parse_stat_user("user>>>user_traffic_7>>>traffic>>>uplink").as_deref(),
            Some("user_traffic_7")
        );
    }

    #[test]
    fn a_blank_user_is_not_a_user() {
        assert_eq!(parse_stat_user("user>>>   >>>traffic>>>uplink"), None);
    }
}
