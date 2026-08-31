use crate::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use caramba_shared::api::{AgentAction, HeartbeatRequest, HeartbeatResponse};
use caramba_shared::config::ConfigResponse;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{error, info, warn};

#[derive(Deserialize)]
struct IpApiResponse {
    #[serde(rename = "countryCode")]
    country_code: String,
    country: String,
    city: String,
    lat: f64,
    lon: f64,
}

fn country_code_to_flag(code: &str) -> String {
    let code = code.to_uppercase();
    let chars: Vec<char> = code.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if chars.len() != 2 {
        return "🌐".to_string();
    }
    let offset = 127397u32;
    let first = chars[0] as u32 + offset;
    let second = chars[1] as u32 + offset;
    match (char::from_u32(first), char::from_u32(second)) {
        (Some(f), Some(s)) => format!("{}{}", f, s),
        _ => "🌐".to_string(),
    }
}

/// Agent Heartbeat
/// POST /api/v2/node/heartbeat
pub async fn heartbeat(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let remote_ip = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0")
        .to_string();

    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    // Заодно читаем status и is_relay ДО обновления, чтобы определить первое подключение
    let (node_id, node_country_code, node_country, pre_update_status, node_is_relay) = match sqlx::query_as::<
        _,
        (i64, Option<String>, Option<String>, Option<String>, bool),
    >(
        "SELECT id, country_code, country, status, COALESCE(is_relay, false) FROM nodes WHERE join_token = $1",
    )
    .bind(&token)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some((id, cc, c, st, relay))) => (id, cc, c, st, relay),
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("does not exist") {
                match sqlx::query_scalar::<_, i64>("SELECT id FROM nodes WHERE join_token = $1")
                    .bind(&token)
                    .fetch_optional(&state.pool)
                    .await
                {
                    Ok(Some(id)) => (id, None, None, None, false),
                    Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
                    Err(e2) => {
                        error!("DB Error in heartbeat fallback: {}", e2);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
                    }
                }
            } else {
                error!("DB Error in heartbeat: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
            }
        }
    };

    // 3. Update Status (Optimized: Removed Heavy Telemetry Updates)
    // We only update critical fields (last_seen, ip, version) here to keep the API response fast.
    // Detailed telemetry (CPU, RAM, Connections) is handled asynchronously in TelemetryService.
    // Fixed logic: Preserve 'provisioning' status until SNI scanning is complete.
    let update_result = sqlx::query("UPDATE nodes SET last_seen = CURRENT_TIMESTAMP, status = CASE WHEN status = 'disabled' THEN 'disabled' WHEN status = 'provisioning' THEN 'provisioning' ELSE 'active' END, ip = CASE WHEN ip LIKE 'pending-%' OR ip = '0.0.0.0' THEN $1 ELSE ip END, version = $2 WHERE id = $3")
        .bind(&remote_ip)
        .bind(&req.version)
        .bind(node_id)
        .execute(&state.pool)
        .await;

    if let Err(e) = update_result {
        warn!(
            "Primary node heartbeat update failed for node {}: {}",
            node_id, e
        );
    }

    // Auto-provision: если нода впервые подключилась (статус 'provisioning' = ещё не сконфигурирована),
    // запускаем провизионирование в фоне чтобы не блокировать ответ heartbeat.
    if pre_update_status.as_deref() == Some("provisioning") {
        let pool_clone = state.pool.clone();
        let orch_clone = state.orchestration_service.clone();
        tokio::spawn(async move {
            if node_is_relay {
                if let Err(e) = auto_provision_relay(node_id, &pool_clone, &orch_clone).await {
                    error!("Auto-provision relay failed for node {}: {}", node_id, e);
                } else {
                    info!("Auto-provision relay complete for node {}", node_id);
                }
            } else {
                if let Err(e) = auto_provision_exit(node_id, &pool_clone, &orch_clone).await {
                    error!("Auto-provision exit failed for node {}: {}", node_id, e);
                } else {
                    info!("Auto-provision exit complete for node {}", node_id);
                }
            }
        });
    }

    // GeoIP Check (Async) — trigger if country_code OR country/city/flag are missing
    if node_country_code.is_none() || node_country.is_none() {
        let pool = state.pool.clone();
        let ip_target = remote_ip.clone();
        tokio::spawn(async move {
            let url = format!(
                "http://ip-api.com/json/{}?fields=countryCode,country,city,lat,lon",
                ip_target
            );
            match reqwest::get(&url).await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<IpApiResponse>().await {
                        let flag = country_code_to_flag(&json.country_code);
                        let _ = sqlx::query("UPDATE nodes SET country_code = $1, country = $2, city = $3, flag = $4, latitude = $5, longitude = $6 WHERE id = $7")
                             .bind(&json.country_code)
                             .bind(&json.country)
                             .bind(&json.city)
                             .bind(&flag)
                             .bind(json.lat)
                             .bind(json.lon)
                             .bind(node_id)
                             .execute(&pool)
                             .await;
                        info!(
                            "🗺️ [GeoIP] Detected location {} {}, {} ({}, {}) for node {}",
                            flag, json.city, json.country, json.lat, json.lon, node_id
                        );
                    }
                }
                Err(e) => error!("GeoIP failed: {}", e),
            }
        });
    }

    // 4. Process Per-User Traffic Usage
    // Orchestrator tags sing-box users as "user_{tg_id}" (Telegram ID).
    // Вместо N*2 отдельных запросов используем пакетную обработку:
    //   1 SELECT для разрешения всех tg_id → user_id
    //   1 UPDATE с unnest() для записи трафика всех пользователей сразу
    let mut touched_subscriptions: HashSet<i64> = HashSet::new();
    if let Some(ref usage_map) = req.user_usage {
        let mut relay_legacy_usage_bytes: u64 = 0;
        let mut attributed_count = 0u32;
        let mut unresolved_count = 0u32;

        // Собираем пары (tg_id, bytes) для пользователей и отдельно relay_legacy
        let mut tg_id_bytes: Vec<(i64, u64)> = Vec::new();
        for (tag, bytes) in usage_map {
            if let Some(tg_id) = crate::services::user_tag::parse_user_tag(tag) {
                tg_id_bytes.push((tg_id, *bytes));
            }
            if tag.starts_with("relay_") && tag.ends_with("_legacy") {
                relay_legacy_usage_bytes = relay_legacy_usage_bytes.saturating_add(*bytes);
            }
        }

        if !tg_id_bytes.is_empty() {
            // Шаг 1: один запрос для разрешения всех tg_id в user_id
            let all_tg_ids: Vec<i64> = tg_id_bytes.iter().map(|(tg_id, _)| *tg_id).collect();

            let rows: Vec<(i64, i64)> =
                sqlx::query_as("SELECT id, tg_id FROM users WHERE tg_id = ANY($1)")
                    .bind(&all_tg_ids)
                    .fetch_all(&state.pool)
                    .await
                    .unwrap_or_default();

            // tg_id → user_id
            let tg_to_uid: HashMap<i64, i64> =
                rows.into_iter().map(|(id, tg_id)| (tg_id, id)).collect();

            // Логируем пользователей, которых не нашли в БД
            for (tg_id, bytes) in &tg_id_bytes {
                if !tg_to_uid.contains_key(tg_id) {
                    tracing::warn!(
                        "Traffic: tg_id={} not found in users table, {} bytes lost",
                        tg_id,
                        bytes
                    );
                    unresolved_count += 1;
                }
            }

            // Шаг 2: один bulk UPDATE через unnest() для всех найденных пользователей
            let mut user_ids: Vec<i64> = Vec::new();
            let mut bytes_vec: Vec<i64> = Vec::new();
            for (tg_id, bytes) in &tg_id_bytes {
                if let Some(&uid) = tg_to_uid.get(tg_id) {
                    user_ids.push(uid);
                    bytes_vec.push(*bytes as i64);
                }
            }

            if !user_ids.is_empty() {
                // Обновляем трафик одним запросом; RETURNING id для отслеживания квот
                let resolved_count = user_ids.len() as u32;
                let updated_ids: Vec<i64> = sqlx::query_scalar(
                    r#"
                    UPDATE subscriptions s
                    SET used_traffic = used_traffic + c.bytes,
                        traffic_updated_at = NOW()
                    FROM (
                        SELECT unnest($1::bigint[]) AS user_id,
                               unnest($2::bigint[]) AS bytes
                    ) c
                    WHERE s.user_id = c.user_id
                      AND s.status = 'active'
                    RETURNING s.id
                    "#,
                )
                .bind(&user_ids)
                .bind(&bytes_vec)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();

                let updated_sub_count = updated_ids.len() as u32;
                attributed_count += updated_sub_count;
                touched_subscriptions.extend(updated_ids);

                // Параллельно с накопительным счётчиком подписки пишем подневную
                // дельту трафика на пользователя — это источник графика трафика в
                // standalone-приложении (GET /api/v2/app/traffic). Узел отдаёт один
                // счётчик байт на пользователя, поэтому весь объём идёт в down_bytes,
                // up_bytes остаётся 0 до появления раздельных счётчиков у агента.
                // Ошибка записи истории не должна ломать приём heartbeat'а.
                let traffic_repo = caramba_db::repositories::traffic_repo::TrafficRepository::new(
                    state.pool.clone(),
                );
                if let Err(e) = traffic_repo.record_usage_bulk(&user_ids, &bytes_vec).await {
                    tracing::warn!(error = %e, "app: failed to record daily traffic history");
                }

                // Пользователи, у которых нет активной подписки — UPDATE их не затронул
                let no_sub_count = resolved_count.saturating_sub(updated_sub_count);
                if no_sub_count > 0 {
                    tracing::warn!(
                        "Traffic: {} user(s) had no active subscription, bytes lost",
                        no_sub_count
                    );
                    unresolved_count += no_sub_count;
                }
            }
        }

        if attributed_count > 0 || unresolved_count > 0 {
            tracing::debug!(
                "Traffic heartbeat: {} tags attributed, {} unresolved",
                attributed_count,
                unresolved_count
            );
        }

        // Record last observed legacy relay traffic, used by relay auth guardrail.
        if relay_legacy_usage_bytes > 0 {
            let _ = state
                .settings
                .set("relay_legacy_usage_last_seen_at", &Utc::now().to_rfc3339())
                .await;
            let _ = state
                .settings
                .set(
                    "relay_legacy_usage_last_seen_bytes",
                    &relay_legacy_usage_bytes.to_string(),
                )
                .await;
        }
    }

    if !touched_subscriptions.is_empty() {
        let touched_vec: Vec<i64> = touched_subscriptions.into_iter().collect();
        match state
            .subscription_service
            .expire_over_quota_candidates(&touched_vec)
            .await
        {
            Ok(expired_rows) if !expired_rows.is_empty() => {
                warn!(
                    "Heartbeat quota enforcement expired {} subscriptions on node {}",
                    expired_rows.len(),
                    node_id
                );

                let connection_service = state.connection_service.clone();
                let orchestration_service = state.orchestration_service.clone();
                tokio::spawn(async move {
                    // Fan node notifications out by plan: subscriptions.node_id
                    // can be NULL (or cover only one of several nodes serving
                    // the plan), which would leave expired users in the other
                    // nodes' configs.
                    let mut plans_to_regen: HashSet<i64> = HashSet::new();
                    for row in expired_rows {
                        plans_to_regen.insert(row.plan_id);
                        if let Err(e) = connection_service
                            .kill_subscription_connections(row.subscription_id)
                            .await
                        {
                            error!(
                                "Failed to terminate sessions for quota-expired subscription {}: {}",
                                row.subscription_id, e
                            );
                        }
                    }

                    let plan_ids: Vec<i64> = plans_to_regen.into_iter().collect();
                    if let Err(e) = orchestration_service
                        .notify_nodes_for_plans(&plan_ids)
                        .await
                    {
                        error!(
                            "Failed to notify nodes after quota expiration update for plans {:?}: {}",
                            plan_ids, e
                        );
                    }
                });
            }
            Ok(_) => {}
            Err(e) => {
                error!(
                    "Failed to enforce quota from heartbeat candidates on node {}: {}",
                    node_id, e
                );
            }
        }
    }

    // 6. Process Telemetry (Optimized)
    // Run in background to not block heartbeat response.
    // TelemetryService now handles all resource stats updates to avoid double-writes.
    let telemetry_svc = state.telemetry_service.clone();
    let req_clone = req.clone();

    tokio::spawn(async move {
        if let Err(e) = telemetry_svc
            .process_heartbeat(
                node_id,
                req_clone.active_connections,
                req_clone.traffic_up,
                req_clone.traffic_down,
                req_clone.speed_mbps,
                req_clone.discovered_snis,
                req_clone.uptime,
                req_clone.latency,
                req_clone.cpu_usage,
                req_clone.memory_usage,
                req_clone.max_ram,
                req_clone.cpu_cores,
                req_clone.cpu_model,
            )
            .await
        {
            error!("Telemetry processing failed for node {}: {}", node_id, e);
        }
    });

    // U22 (config versioning/ACK): record the hash the node has actually applied
    // + restarted with, so rollout state is observable and SNI-rotation logic can
    // avoid races. Stored in Redis (no schema change needed; backward compatible —
    // older nodes simply omit the field and we skip the write). TTL 3 days so it
    // survives normal operation but self-cleans for dead nodes.
    if let Some(applied) = req.last_applied_config_hash.as_deref()
        && !applied.is_empty()
    {
        let _ = state
            .redis
            .set(
                &format!("node_applied_config_hash:{}", node_id),
                applied,
                3 * 24 * 3600,
            )
            .await;
    }

    // U23 (RU-side block detection canary): the node reports early-RST /
    // handshake-terminated-early symptoms against its current SNI. Persist the
    // latest signal (short TTL) for the panel's SNI monitor / dashboards, and log
    // loudly so an active block is visible. Rotation itself is still driven by the
    // node (which already requests a faster rotation under block); here we make the
    // event observable and durable for cross-node correlation.
    if let Some(ref signals) = req.block_signals
        && (signals.early_rst || signals.handshake_terminated_early)
    {
        warn!(
            "🚨 [U23] Node {} reports RU-block symptoms on SNI '{}': early_rst={}, handshake_terminated_early={}, streak={}",
            node_id,
            signals.sni,
            signals.early_rst,
            signals.handshake_terminated_early,
            signals.consecutive_failures
        );
        if let Ok(payload) = serde_json::to_string(signals) {
            // Per-node latest signal (short TTL — block state is transient).
            let _ = state
                .redis
                .set(
                    &format!("node_block_signal:{}", node_id),
                    &payload,
                    900, // 15 min
                )
                .await;
        }
    }

    // 5. Agent Update Logic (Phase 67)
    let auto_update_agents: bool = state
        .settings
        .get_or_default("auto_update_agents", "true")
        .await
        .parse()
        .unwrap_or(true);
    let latest_version: String = state
        .settings
        .get_or_default("agent_latest_version", "0.0.0")
        .await;

    // Per-node target_version (set by "Rollout Now") always takes priority
    let stored_target: Option<String> = sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(target_version, '') FROM nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
    .filter(|v: &String| !v.is_empty() && v != "0.0.0");

    let target_version = stored_target.or_else(|| {
        // Fall back to global agent_latest_version only if auto-update is ON
        if auto_update_agents && latest_version != "0.0.0" {
            Some(latest_version)
        } else {
            None
        }
    });

    // 6. Action Trigger (Log Collection, Config Update, etc.)
    // Читаем auto_configure и pending_log_collection одним запросом.
    // auto_configure выставляется при провизионировании; при успешном чтении сбрасываем флаг.
    let (auto_configure, pending_logs): (bool, bool) = sqlx::query_as(
        "SELECT COALESCE(auto_configure, false), COALESCE(pending_log_collection, false) FROM nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((false, false));

    // Сбрасываем флаг auto_configure, чтобы следующий heartbeat не повторял UpdateConfig
    if auto_configure {
        let _ = sqlx::query("UPDATE nodes SET auto_configure = false WHERE id = $1")
            .bind(node_id)
            .execute(&state.pool)
            .await;
    }

    let mut action = AgentAction::None;
    if auto_configure {
        // UpdateConfig имеет приоритет: нода должна сначала подтянуть конфиг
        action = AgentAction::UpdateConfig;
    } else if pending_logs {
        action = AgentAction::CollectLogs;
    }

    (
        StatusCode::OK,
        Json(HeartbeatResponse {
            success: true,
            action,
            latest_version: target_version,
        }),
    )
        .into_response()
}

// ... (existing code) ...

/// Get Agent Update Info
/// GET /api/v2/node/update-info
pub async fn get_update_info(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Token (Quick Check)
    let valid: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM nodes WHERE join_token = $1")
        .bind(&token)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !valid {
        return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
    }

    // 3. Fetch Update Info from Settings
    let version = state
        .settings
        .get_or_default("agent_latest_version", "0.0.0")
        .await;
    let mut url = state.settings.get_or_default("agent_update_url", "").await;
    let hash = state.settings.get_or_default("agent_update_hash", "").await;

    // Resolve relative URL
    if url.starts_with('/')
        && let Some(host) = headers.get("host").and_then(|h| h.to_str().ok())
    {
        let protocol = if host.contains("localhost") || host.contains("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        url = format!("{}://{}{}", protocol, host, url);
    }

    Json(serde_json::json!({
        "version": version,
        "url": url,
        "hash": hash
    }))
    .into_response()
}

/// Get Node Configuration
/// GET /api/v2/node/config
pub async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    // 2. Validate Node
    // Using simple query_as to avoid compilation failure if DB migration is not applied locally yet.
    // At runtime, it will fail if column is missing, but it unblocks build.
    let (node_id, is_enabled) = match sqlx::query_as::<_, (i64, bool)>(
        "SELECT id, is_enabled FROM nodes WHERE join_token = $1",
    )
    .bind(&token)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some((id, enabled))) => (id, enabled),
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("does not exist") {
                match sqlx::query_scalar::<_, i64>("SELECT id FROM nodes WHERE join_token = $1")
                    .bind(&token)
                    .fetch_optional(&state.pool)
                    .await
                {
                    Ok(Some(id)) => (id, true),
                    Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
                    Err(e2) => {
                        error!("DB Error in get_config fallback: {}", e2);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
                    }
                }
            } else {
                error!("DB Error in get_config: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
            }
        }
    };

    if !is_enabled {
        return (StatusCode::FORBIDDEN, "Node is disabled").into_response();
    }

    // 3. Generate Config
    match state
        .orchestration_service
        .generate_node_config_json(node_id)
        .await
    {
        Ok((_, config_value)) => {
            let config_str: String = config_value.to_string();
            let hash = format!("{:x}", md5::compute(config_str.as_bytes()));

            // Update last_synced_at
            let _ =
                sqlx::query("UPDATE nodes SET last_synced_at = CURRENT_TIMESTAMP WHERE id = $1")
                    .bind(node_id)
                    .execute(&state.pool)
                    .await;

            (
                StatusCode::OK,
                Json(ConfigResponse {
                    hash,
                    content: config_value,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Config generation failed for node {}: {}", node_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}

/// Rotate SNI for a node
/// POST /api/v2/node/rotate-sni
///
/// Делегирует всю логику в security_service::rotate_node_sni, который:
///   — проверяет pinned SNI → global favorites → relay whitelist → global pool
///   — обновляет nodes.reality_sni и nodes.last_sni_rotation
///   — добавляет старый SNI в blocklist если причина "Validation failed:" или "Auto-Heal:"
///   — пишет лог в sni_rotation_log
pub async fn rotate_sni(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node by join_token
    let node_res: Result<Option<(i64,)>, sqlx::Error> =
        sqlx::query_as("SELECT id FROM nodes WHERE join_token = $1")
            .bind(&token)
            .fetch_optional(&state.pool)
            .await;

    let node_id: i64 = match node_res {
        Ok(Some((id,))) => id,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
            error!("DB Error validating join_token in rotate_sni: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("Node-Initiated Rotation");

    // 3. Delegate полностью в security_service (canonical path, нет дублирования логики)
    match state
        .security_service
        .rotate_node_sni(node_id, reason)
        .await
    {
        Ok((old_sni, new_sni, rotation_id)) => {
            info!(
                "SNI rotated for node {} (reason: {}): {} → {} (log #{})",
                node_id, reason, old_sni, new_sni, rotation_id
            );

            // Уведомляем ноду о смене конфига (async, не блокируем ответ)
            let orchestration = state.orchestration_service.clone();
            tokio::spawn(async move {
                if let Err(e) = orchestration.notify_node_update(node_id).await {
                    error!(
                        "Failed to notify node {} after SNI rotation: {}",
                        node_id, e
                    );
                }
            });

            // Уведомляем пользователей (async, non-blocking)
            if let Ok(bot) = state
                .bot_manager
                .get_bot()
                .await
                .map(|b| b as teloxide::Bot)
            {
                let notification_service = state.notification_service.clone();
                let old_clone = old_sni.clone();
                let new_clone = new_sni.clone();
                tokio::spawn(async move {
                    if let Err(e) = notification_service
                        .notify_sni_rotation(&bot, node_id, &old_clone, &new_clone, rotation_id)
                        .await
                    {
                        error!("Failed to send SNI rotation notifications: {}", e);
                    }
                });
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "rotated",
                    "old_sni": old_sni,
                    "new_sni": new_sni,
                    "rotation_id": rotation_id
                })),
            )
                .into_response()
        }
        Err(e) if e.to_string().contains("No other SNI") => {
            warn!("SNI pool exhausted for node {}: {}", node_id, e);
            (StatusCode::CONFLICT, "No other SNI available").into_response()
        }
        Err(e) => {
            error!("Failed to rotate SNI for node {}: {}", node_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Rotation failed: {}", e),
            )
                .into_response()
        }
    }
}

/// Long Polling for Config Updates
/// GET /api/v2/node/updates/poll
pub async fn poll_updates(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node (Cache or DB)
    // For polling, we might want to use cache to avoid hitting DB every 30s x 1000 nodes?
    // But validate_token usually hits DB.
    // Let's assume hitting DB is fine for now (once per 30s per node is low load).
    // Or we can rely on Redis.
    // For now, simple DB query.
    let node_res: Result<Option<i64>, sqlx::Error> =
        sqlx::query_scalar("SELECT id FROM nodes WHERE join_token = $1")
            .bind(&token)
            .fetch_optional(&state.pool)
            .await;

    let node_id = match node_res {
        Ok(Some(id)) => id,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
        Err(e) => {
            error!("DB Error in poll_updates: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    // 3. Check State Before Waiting (Fixes Race Condition)
    // If client's config hash is stale, update immediately without waiting for PubSub.
    if let Some(client_hash) = headers.get("X-Config-Hash").and_then(|h| h.to_str().ok())
        && !client_hash.trim().is_empty()
    {
        // Need to know what the current hash *should* be.
        // Since storing the exact current hash in DB is expensive on every generation,
        // we rely on `last_synced_at` or `last_sync_trigger`.
        // But a true hash check requires generating or caching the config.
        //
        // Optimization: If `last_sync_trigger` is recent, force update.
        // Or better: Let's assume if the client is polling, they want to know if something CHANGED.
        //
        // For now, we trust the PubSub. But to fix the "missed message" race:
        // We check if `last_synced_at` < `last_rotated_at` or similar? No.
        //
        // Best approach: "State Check".
        // We don't have the current server-side hash readily available without generating it.
        // However, we can use a "config_version" or "sequence" if we had one.
        //
        // Fallback: If X-Config-Hash is provided, we can check if it matches a cached value.
        // If not cached, we wait.
        //
        // Actually, the issue is often that the "Update" signal was sent WHILE the agent was restarting its poll loop.
        // We can check Redis for a "pending_update" flag for this node?
        // Or simply rely on the fact that if a user clicks "Sync", we publish.
        //
        // To truly fix "Timeout" where the user waits:
        // If this is a manual "Sync" action from UI, the UI sets a flag in Redis `node_sync_pending:{id}`.
        // We check that flag here.
        let pending_key = format!("node_sync_pending:{}", node_id);
        if let Ok(exists) = state.redis.exists(&pending_key).await
            && exists
        {
            let _ = state.redis.del(&pending_key).await; // Consume the flag
            return (
                StatusCode::OK,
                Json(serde_json::json!({"update": true, "message": "pending_sync"})),
            )
                .into_response();
        }
    }

    // 4. Wait for update
    let rx = state.pubsub.wait_for(&format!("node_events:{}", node_id));

    // 5. Select with timeout (30s)
    match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
        Ok(Ok(payload)) => {
            // Message received from pubsub.
            let signal = payload.trim().to_ascii_lowercase();
            if signal == "scan" {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"update": false, "message": "scan"})),
                )
                    .into_response()
            } else if signal == "restart" {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"update": true, "message": "restart"})),
                )
                    .into_response()
            } else {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"update": true, "message": signal})),
                )
                    .into_response()
            }
        }
        Ok(Err(_)) => {
            // Sender dropped
            (StatusCode::OK, Json(serde_json::json!({"update": false}))).into_response()
        }
        Err(_) => {
            // Timeout
            (StatusCode::OK, Json(serde_json::json!({"update": false}))).into_response()
        }
    }
}

/// Get Agent Settings (Decoy, etc)
/// GET /api/v2/node/settings
pub async fn get_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Token (Quick Check)
    let valid: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM nodes WHERE join_token = $1")
        .bind(&token)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !valid {
        return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response();
    }

    // 3. Fetch Decoy Settings
    let decoy_enabled: bool = state
        .settings
        .get_or_default("decoy_enabled", "false")
        .await
        .parse()
        .unwrap_or(false);
    let decoy_urls_str = state
        .settings
        .get_or_default(
            "decoy_urls",
            "[\"https://www.google.com\", \"https://www.azure.com\", \"https://www.netflix.com\"]",
        )
        .await;
    let min_interval: u64 = state
        .settings
        .get_or_default("decoy_min_interval", "60")
        .await
        .parse()
        .unwrap_or(60);
    let max_interval: u64 = state
        .settings
        .get_or_default("decoy_max_interval", "600")
        .await
        .parse()
        .unwrap_or(600);

    let decoy_urls: Vec<String> = serde_json::from_str(&decoy_urls_str).unwrap_or_default();

    // 4. Fetch Kill Switch Settings
    let kill_switch_enabled: bool = state
        .settings
        .get_or_default("kill_switch_enabled", "false")
        .await
        .parse()
        .unwrap_or(false);
    let kill_switch_timeout: u64 = state
        .settings
        .get_or_default("kill_switch_timeout", "300")
        .await
        .parse()
        .unwrap_or(300);

    Json(serde_json::json!({
        "decoy": {
            "enabled": decoy_enabled,
            "urls": decoy_urls,
            "min_interval": min_interval,
            "max_interval": max_interval
        },
        "kill_switch": {
            "enabled": kill_switch_enabled,
            "timeout": kill_switch_timeout
        }
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct RegisterNodeRequest {
    pub enrollment_key: String,
    pub hostname: String,
    pub ip: Option<String>,
    pub node_type: Option<String>,
}

#[derive(Serialize)]
pub struct RegisterNodeResponse {
    pub node_id: i64,
    pub join_token: String,
}

/// Register a new node using an Enrollment Key
/// POST /api/v2/node/register
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterNodeRequest>,
) -> impl IntoResponse {
    // 1. Validate API Key
    let api_key_res: Result<Option<caramba_db::models::api_key::ApiKey>, _> =
        sqlx::query_as("SELECT * FROM api_keys WHERE key = $1 AND is_active = TRUE")
            .bind(&payload.enrollment_key)
            .fetch_optional(&state.pool)
            .await;

    let api_key = match api_key_res {
        Ok(Some(k)) => k,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid API Key").into_response(),
        Err(e) => {
            error!("DB Error checking API key: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }
    };

    if api_key.key_type != "enrollment" {
        return (StatusCode::FORBIDDEN, "Invalid Key Type").into_response();
    }

    if let Some(max) = api_key.max_uses
        && api_key.current_uses >= max
    {
        return (StatusCode::FORBIDDEN, "Key Usage Limit Reached").into_response();
    }

    // License gate (P4, contract E): block self-register beyond max_nodes. Only
    // the INSERT path is gated here; heartbeat/config/rotate for already
    // registered nodes are never affected.
    let limits = crate::license::effective_limits(&state).await;
    let node_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    if let Err(e) = crate::license::check_can_add_node(&limits, node_count) {
        warn!("Node self-register blocked by license: {}", e);
        return (StatusCode::FORBIDDEN, e.to_string()).into_response();
    }

    // 2. Increment Usage
    let _ = sqlx::query("UPDATE api_keys SET current_uses = current_uses + 1 WHERE id = $1")
        .bind(api_key.id)
        .execute(&state.pool)
        .await;

    // 3. Create Node
    let join_token = uuid::Uuid::new_v4().to_string();
    // Default to pending IP to ensure it's updated later. OR use 0.0.0.0.
    // Use "pending-" prefix so our heartbeat logic picks it up!
    let ip = payload
        .ip
        .unwrap_or_else(|| format!("pending-{}", &join_token[0..8]));
    let node_type = match payload
        .node_type
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("relay") => "relay",
        _ => "exit",
    };
    let is_relay = node_type == "relay";

    // Start in provisioning state to wait for first SNI scan.
    let node_id_res = sqlx::query("INSERT INTO nodes (name, ip, join_token, status, is_enabled, node_type, is_relay) VALUES ($1, $2, $3, 'provisioning', TRUE, $4, $5) RETURNING id")
        .bind(&payload.hostname)
        .bind(&ip)
        .bind(&join_token)
        .bind(node_type)
        .bind(is_relay)
        .fetch_one(&state.pool)
        .await;

    match node_id_res {
        Ok(row) => {
            use sqlx::Row;
            let node_id: i64 = row.get("id");
            info!(
                "✅ Node registered via API Key {}: {} (ID: {})",
                api_key.name, payload.hostname, node_id
            );

            (
                StatusCode::OK,
                Json(RegisterNodeResponse {
                    node_id,
                    join_token,
                }),
            )
                .into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("node_type") && msg.contains("does not exist") {
                let fallback = sqlx::query(
                    "INSERT INTO nodes (name, ip, join_token, status, is_enabled, is_relay) VALUES ($1, $2, $3, 'provisioning', TRUE, $4) RETURNING id",
                )
                .bind(&payload.hostname)
                .bind(&ip)
                .bind(&join_token)
                .bind(is_relay)
                .fetch_one(&state.pool)
                .await;

                match fallback {
                    Ok(row) => {
                        use sqlx::Row;
                        let node_id: i64 = row.get("id");
                        info!(
                            "✅ Node registered via API Key {}: {} (ID: {})",
                            api_key.name, payload.hostname, node_id
                        );

                        (
                            StatusCode::OK,
                            Json(RegisterNodeResponse {
                                node_id,
                                join_token,
                            }),
                        )
                            .into_response()
                    }
                    Err(inner) => {
                        error!("Failed to create node (fallback): {}", inner);
                        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create node").into_response()
                    }
                }
            } else {
                error!("Failed to create node: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create node").into_response()
            }
        }
    }
}

/// Report Logs from Agent
/// POST /api/v2/node/logs
pub async fn report_node_logs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<caramba_shared::api::LogResponse>,
) -> impl IntoResponse {
    // 1. Extract Token
    let token = match headers.get("Authorization") {
        Some(hv) => hv.to_str().unwrap_or("").replace("Bearer ", ""),
        None => return (StatusCode::UNAUTHORIZED, "Missing Token").into_response(),
    };

    // 2. Validate Node
    let node_id: i64 = match sqlx::query_scalar("SELECT id FROM nodes WHERE join_token = $1")
        .bind(&token)
        .fetch_one(&state.pool)
        .await
    {
        Ok(id) => id,
        Err(_) => return (StatusCode::UNAUTHORIZED, "Invalid Token").into_response(),
    };

    // 3. Store Logs (In Redis for quick retrieval, and reset pending flag)
    let logs_json = serde_json::to_string(&req.logs).unwrap_or_default();
    let _ = state
        .redis
        .set(&format!("node_logs:{}", node_id), &logs_json, 300)
        .await; // Store for 5 mins

    let _ = sqlx::query("UPDATE nodes SET pending_log_collection = FALSE WHERE id = $1")
        .bind(node_id)
        .execute(&state.pool)
        .await;

    // 4. Notify UI via PubSub
    let _ = state
        .pubsub
        .publish(&format!("node_events:{}", node_id), "logs_ready")
        .await;

    info!("✅ Logs received and stored for node {}", node_id);
    StatusCode::OK.into_response()
}

/// Авто-провизионирование relay-ноды.
///
/// Relay-ноды не сканируют SNI самостоятельно — они получают whitelist из пула.
/// Выбираем лучший premium/favorite SNI, обновляем reality_sni, ставим status=active,
/// auto_configure=true и уведомляем orchestration, чтобы нода получила обновлённый конфиг.
async fn auto_provision_relay(
    node_id: i64,
    pool: &sqlx::PgPool,
    orch: &crate::services::orchestration_service::OrchestrationService,
) -> anyhow::Result<()> {
    // Выбираем лучший premium/favorite SNI, не попавший в blacklist
    let sni: Option<String> = sqlx::query_scalar(
        r#"
        SELECT domain FROM sni_pool
        WHERE (is_premium = true OR is_favorite = true)
          AND domain NOT IN (SELECT domain FROM sni_blacklist)
        ORDER BY COALESCE(health_score, 100) DESC, latency_ms ASC NULLS LAST
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    let Some(domain) = sni else {
        anyhow::bail!(
            "No premium/favorite SNI available for relay node {}",
            node_id
        );
    };

    // Обновляем ноду: reality_sni, status=active, auto_configure=true
    sqlx::query(
        "UPDATE nodes SET reality_sni = $1, status = 'active', auto_configure = true WHERE id = $2",
    )
    .bind(&domain)
    .bind(node_id)
    .execute(pool)
    .await?;

    info!(
        "Auto-provisioned relay node {} with SNI '{}'",
        node_id, domain
    );

    // Сигнализируем агенту подтянуть новый конфиг
    orch.notify_node_update(node_id).await?;

    Ok(())
}

/// Авто-провизионирование exit-ноды.
///
/// Exit-ноды используют favorite SNI из пула для Reality-конфигурации.
/// Выбираем лучший favorite SNI, обновляем reality_sni, ставим status=active,
/// auto_configure=true и уведомляем orchestration.
async fn auto_provision_exit(
    node_id: i64,
    pool: &sqlx::PgPool,
    orch: &crate::services::orchestration_service::OrchestrationService,
) -> anyhow::Result<()> {
    // Выбираем лучший favorite SNI, не попавший в blacklist
    let sni: Option<String> = sqlx::query_scalar(
        r#"
        SELECT domain FROM sni_pool
        WHERE is_favorite = true
          AND domain NOT IN (SELECT domain FROM sni_blacklist)
        ORDER BY COALESCE(health_score, 100) DESC NULLS LAST
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    let Some(domain) = sni else {
        anyhow::bail!("No favorite SNI available for exit node {}", node_id);
    };

    // Обновляем ноду: reality_sni, status=active, auto_configure=true
    sqlx::query(
        "UPDATE nodes SET reality_sni = $1, status = 'active', auto_configure = true WHERE id = $2",
    )
    .bind(&domain)
    .bind(node_id)
    .execute(pool)
    .await?;

    info!(
        "Auto-provisioned exit node {} with SNI '{}'",
        node_id, domain
    );

    // Сигнализируем агенту подтянуть новый конфиг
    orch.notify_node_update(node_id).await?;

    Ok(())
}
