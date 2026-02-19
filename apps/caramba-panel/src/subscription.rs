use axum::{
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{error, warn};

use crate::AppState;

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>, // "clash" | "v2ray" | "singbox"
    pub node_id: Option<i64>,
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    req: Request,
) -> Response {
    // 0. Smart Routing: Redirect if subscription_domain is set and we are not on it
    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    if !sub_domain.is_empty() {
        if let Some(host) = req
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
        {
            let host_clean = host.split(':').next().unwrap_or(host);
            let sub_domain_clean = sub_domain.split(':').next().unwrap_or(&sub_domain);

            if host_clean != sub_domain_clean {
                let proto = "https";
                let full_url = format!("{}://{}/sub/{}", proto, sub_domain, uuid);
                return axum::response::Redirect::permanent(&full_url).into_response();
            }
        }
    }

    // 0.5 Extract IP and User-Agent for tracking
    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let client_ip = req
        .headers()
        .get("cf-connecting-ip")
        .or_else(|| req.headers().get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0")
        .to_string();

    // 1. Rate Limit (30 req / min per UUID)
    let rate_key = format!("rate:sub:{}", uuid);
    match state.redis.check_rate_limit(&rate_key, 30, 60).await {
        Ok(allowed) => {
            if !allowed {
                warn!("Rate limit exceeded for subscription {}", uuid);
                return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
            }
        }
        Err(e) => {
            error!("Rate limit check failed: {}", e);
        }
    }

    // 2. Get subscription
    let sub = match state
        .subscription_service
        .get_subscription_by_uuid(&uuid)
        .await
    {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "Subscription not found").into_response();
        }
    };

    // 3. Check if active
    if sub.status != "active" {
        return (StatusCode::FORBIDDEN, "Subscription inactive or expired").into_response();
    }

    // 3.5 Enforce device limit (Phase 7)
    let active_ips = state
        .subscription_service
        .get_active_ips(sub.id)
        .await
        .unwrap_or_default();
    let current_ip = &client_ip;

    // Check if this is a new IP or if we're already at the limit
    let is_new_device = !active_ips.iter().any(|rec| rec.client_ip == *current_ip);

    if is_new_device {
        let device_limit = state
            .subscription_service
            .get_subscription_device_limit(sub.id)
            .await
            .unwrap_or(0);
        if device_limit > 0 && active_ips.len() >= device_limit as usize {
            warn!(
                "Device limit reached for subscription {}. Limit: {}, Active: {}",
                uuid,
                device_limit,
                active_ips.len()
            );
            return (StatusCode::FORBIDDEN, "Device limit reached").into_response();
        }
    }

    // 4. Update access tracking
    let _ = state
        .subscription_service
        .track_access(sub.id, &client_ip, user_agent.as_deref())
        .await;

    // 4.5 Prepare Usage Headers (for Hiddify/Sing-box)
    let plan_details = match state
        .subscription_service
        .get_user_subscriptions(sub.user_id)
        .await
    {
        Ok(subs) => subs
            .iter()
            .find(|s| s.sub.id == sub.id)
            .map(|s| (s.plan_name.clone(), s.traffic_limit_gb.unwrap_or(0)))
            .unwrap_or(("VPN Plan".to_string(), 0)),
        Err(_) => ("VPN Plan".to_string(), 0),
    };

    let total_traffic_bytes = (plan_details.1 as i64) * 1024 * 1024 * 1024;
    let used_traffic_bytes = sub.used_traffic as i64;
    let expire_timestamp = sub.expires_at.timestamp();

    // upload=0; download=used; total=limit; expire=timestamp
    let user_info_header = format!(
        "upload=0; download={}; total={}; expire={}",
        used_traffic_bytes, total_traffic_bytes, expire_timestamp
    );

    // ===================================================================
    // client autodetection or raw config mode
    // ===================================================================
    let mut selected_client = params.client.clone();

    // Autodetect if client is not specified
    if selected_client.is_none() {
        let detected = state
            .subscription_service
            .detect_client_type(user_agent.as_deref());
        if detected != "html" {
            selected_client = Some(detected);
        }
    }

    // If still no client (or it's explicitly "html" detected), serve HTML
    if selected_client.is_none() {
        // Use already fetched plan_details
        let plan_name = plan_details;

        let used_gb = sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
        let limit_gb = plan_name.1;
        let traffic_pct = if limit_gb > 0 {
            ((used_gb / limit_gb as f64) * 100.0).min(100.0) as i32
        } else {
            0
        };
        let days_left = (sub.expires_at - chrono::Utc::now()).num_days().max(0);
        let duration_days = (sub.expires_at - sub.created_at).num_days();

        // Build base URL for config links
        let panel_url_setting = state.settings.get_or_default("panel_url", "").await;
        let base_url = if !sub_domain.is_empty() {
            if sub_domain.starts_with("http") {
                sub_domain.clone()
            } else {
                format!("https://{}", sub_domain)
            }
        } else if !panel_url_setting.is_empty() {
            if panel_url_setting.starts_with("http") {
                panel_url_setting.clone()
            } else {
                format!("https://{}", panel_url_setting)
            }
        } else {
            let panel = std::env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string());
            if panel.starts_with("http") {
                panel
            } else {
                format!("https://{}", panel)
            }
        };
        let sub_url = format!("{}/sub/{}", base_url, uuid);

        let expires_display = if duration_days == 0 {
            "No expiration (Traffic Plan)".to_string()
        } else {
            format!(
                "{} ({} days left)",
                sub.expires_at.format("%Y-%m-%d"),
                days_left
            )
        };

        let traffic_display = if limit_gb > 0 {
            format!("{:.2} GB / {} GB", used_gb, limit_gb)
        } else {
            format!("{:.2} GB / ∞", used_gb)
        };

        let html = format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>CARAMBA — Subscription</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap');
body{{
  font-family:'Inter',system-ui,sans-serif;
  background:#0D0D1A;
  color:#E8E8F0;
  min-height:100vh;
  display:flex;
  justify-content:center;
  padding:24px 16px;
}}
.container{{max-width:460px;width:100%}}
.logo{{text-align:center;margin-bottom:32px}}
.logo h1{{
  font-size:28px;font-weight:800;
  background:linear-gradient(135deg,#7C3AED 0%,#3B82F6 50%,#06B6D4 100%);
  -webkit-background-clip:text;-webkit-text-fill-color:transparent;
}}
.logo p{{color:rgba(255,255,255,0.4);font-size:13px;margin-top:4px}}
.card{{
  background:rgba(255,255,255,0.06);
  border:1px solid rgba(255,255,255,0.08);
  border-radius:16px;
  padding:20px;
  margin-bottom:16px;
  backdrop-filter:blur(20px);
}}
.plan-name{{font-size:20px;font-weight:700}}
.badge{{
  display:inline-block;
  padding:4px 12px;border-radius:20px;
  font-size:11px;font-weight:600;text-transform:uppercase;
}}
.badge-active{{background:rgba(16,185,129,0.15);color:#10B981}}
.header-row{{display:flex;align-items:center;justify-content:space-between;margin-bottom:16px}}
.stat-row{{display:flex;justify-content:space-between;font-size:13px;color:rgba(255,255,255,0.6);margin-bottom:8px}}
.progress{{height:6px;background:rgba(255,255,255,0.06);border-radius:3px;overflow:hidden;margin:8px 0 16px}}
.progress-fill{{height:100%;border-radius:3px;background:linear-gradient(90deg,#7C3AED,#3B82F6)}}
.section-label{{font-size:11px;text-transform:uppercase;letter-spacing:1px;color:rgba(255,255,255,0.3);margin-bottom:12px}}
.config-grid{{display:flex;flex-direction:column;gap:10px}}
.config-btn{{
  display:flex;align-items:center;gap:12px;
  background:rgba(255,255,255,0.04);
  border:1px solid rgba(255,255,255,0.08);
  border-radius:12px;padding:14px 16px;
  color:#E8E8F0;font-size:14px;font-weight:500;
  cursor:pointer;text-decoration:none;
  transition:all 0.2s;
}}
.config-btn:hover{{background:rgba(255,255,255,0.08);border-color:rgba(124,58,237,0.3)}}
.config-btn .icon{{font-size:20px;width:32px;text-align:center}}
.config-btn .label{{flex:1}}
.config-btn .dl{{color:rgba(255,255,255,0.3);font-size:12px}}
.copy-section{{margin-top:16px}}
.link-input{{
  width:100%;padding:12px 14px;
  background:rgba(255,255,255,0.04);
  border:1px solid rgba(255,255,255,0.08);
  border-radius:10px;
  color:#E8E8F0;font-family:'SF Mono','Fira Code',monospace;
  font-size:11px;outline:none;
}}
.link-input:focus{{border-color:rgba(124,58,237,0.4)}}
.copy-btn{{
  width:100%;margin-top:10px;padding:14px;
  background:linear-gradient(135deg,#7C3AED 0%,#3B82F6 100%);
  border:none;border-radius:12px;
  color:white;font-size:14px;font-weight:600;
  cursor:pointer;transition:opacity 0.2s;
}}
.copy-btn:active{{opacity:0.8}}
.copy-btn.copied{{background:linear-gradient(135deg,#10B981 0%,#059669 100%)}}
.qr-wrap{{
  display:flex;justify-content:center;
  margin:16px 0;
  padding:16px;background:white;border-radius:12px;
}}
.footer{{text-align:center;margin-top:24px;font-size:11px;color:rgba(255,255,255,0.2)}}
</style>
</head>
<body>
<div class="container">
  <div class="logo">
    <h1>🚀 CARAMBA</h1>
    <p>Your VPN Subscription</p>
  </div>

  <div class="card">
    <div class="header-row">
      <span class="plan-name">{plan_name}</span>
      <span class="badge badge-active">✅ Active</span>
    </div>
    <div class="stat-row"><span>📊 Traffic</span><span>{traffic_display}</span></div>
    {progress_bar}
    <div class="stat-row"><span>⏳ Expires</span><span>{expires_display}</span></div>
  </div>

  <div class="card">
    <div class="section-label">Download Config</div>
    <div class="config-grid">
      <a href="{sub_url}?client=singbox" class="config-btn">
        <span class="icon">📦</span>
        <span class="label">Sing-box / Hiddify</span>
        <span class="dl">JSON →</span>
      </a>
      <a href="{sub_url}?client=v2ray" class="config-btn">
        <span class="icon">⚡</span>
        <span class="label">V2Ray / Xray</span>
        <span class="dl">Base64 →</span>
      </a>
      <a href="{sub_url}?client=clash" class="config-btn">
        <span class="icon">🔥</span>
        <span class="label">Clash / Clash Meta</span>
        <span class="dl">YAML →</span>
      </a>
    </div>
  </div>

  <div class="card">
    <div class="section-label">Subscription Link</div>
    <div class="qr-wrap">
      <img src="https://api.qrserver.com/v1/create-qr-code/?size=180x180&data={sub_url_encoded}" width="180" height="180" alt="QR Code" />
    </div>
    <div class="copy-section">
      <input type="text" class="link-input" id="subLink" value="{sub_url}" readonly onclick="this.select()" />
      <button class="copy-btn" id="copyBtn" onclick="copyLink()">📋 Copy Link</button>
    </div>
  </div>

  <div class="footer">CARAMBA VPN Panel · Powered by Xray</div>
</div>
<script>
function copyLink(){{
  const btn=document.getElementById('copyBtn');
  const input=document.getElementById('subLink');
  navigator.clipboard.writeText(input.value).then(()=>{{
    btn.textContent='✓ Copied!';
    btn.classList.add('copied');
    setTimeout(()=>{{btn.textContent='📋 Copy Link';btn.classList.remove('copied')}},2000);
  }});
}}
</script>
</body>
</html>"##,
            plan_name = plan_name.0,
            traffic_display = traffic_display,
            expires_display = expires_display,
            sub_url = sub_url,
            sub_url_encoded = urlencoding::encode(&sub_url),
            progress_bar = if limit_gb > 0 {
                format!(
                    r#"<div class="progress"><div class="progress-fill" style="width:{}%"></div></div>"#,
                    traffic_pct
                )
            } else {
                String::new()
            },
        );

        return (
            [
                (header::CONTENT_TYPE, "text/html"),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    user_info_header.as_str(),
                ),
                (header::HeaderName::from_static("profile-title"), "CARAMBA"),
            ],
            html,
        )
            .into_response();
    }

    // ===================================================================
    // Raw config mode: ?client=clash|v2ray|singbox
    // ===================================================================

    // 5. Get user keys
    let user_keys = match state.subscription_service.get_user_keys(&sub).await {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to get user keys for sub {}: {}", uuid, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Fetch and filter nodes (Refactored Phase 1.8: Use Plan Groups)
    let nodes_raw = match state.store_service.get_user_nodes(sub.user_id).await {
        Ok(nodes) => nodes,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response(),
    };

    let filtered_nodes = if let Some(nid) = params.node_id {
        nodes_raw
            .into_iter()
            .filter(|n| n.id == nid)
            .collect::<Vec<_>>()
    } else if let Some(pinned_id) = sub.node_id {
        nodes_raw
            .into_iter()
            .filter(|n| n.id == pinned_id)
            .collect::<Vec<_>>()
    } else {
        nodes_raw
    };

    if filtered_nodes.is_empty() {
        return (StatusCode::NOT_FOUND, "Requested server not found").into_response();
    }

    let node_infos = match state
        .subscription_service
        .get_node_infos_with_relays(&filtered_nodes)
        .await
    {
        Ok(infos) => infos,
        Err(e) => {
            error!("Failed to generate node infos: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to process nodes").into_response();
        }
    };

    // Check Redis Cache & Generate
    let client_type = selected_client.as_deref().unwrap_or("singbox");
    let cache_node_id = params.node_id.unwrap_or(0);
    let cache_key = format!("sub_config_v2:{}:{}:{}", uuid, client_type, cache_node_id);

    if let Ok(Some(cached_config)) = state.redis.get(&cache_key).await {
        let filename = match client_type {
            "clash" => "config.yaml",
            "v2ray" => "config.txt",
            _ => "config.json",
        };
        let content_type = match client_type {
            "clash" => "application/yaml",
            "v2ray" => "text/plain",
            _ => "application/json",
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (
                    header::CONTENT_DISPOSITION,
                    format!("inline; filename={}", filename).as_str(),
                ),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    user_info_header.as_str(),
                ),
                (header::HeaderName::from_static("profile-title"), "CARAMBA"),
            ],
            cached_config,
        )
            .into_response();
    }

    let (content, content_type, filename): (String, &'static str, &'static str) = match client_type
    {
        "clash" => {
            match state
                .subscription_service
                .generate_clash(&sub, &node_infos, &user_keys)
            {
                Ok(c) => (c, "application/yaml", "config.yaml"),
                Err(e) => {
                    error!("Clash gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed")
                        .into_response();
                }
            }
        }
        "v2ray" => {
            match state
                .subscription_service
                .generate_v2ray(&sub, &node_infos, &user_keys)
            {
                Ok(c) => (c, "text/plain", "config.txt"),
                Err(e) => {
                    error!("V2Ray gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed")
                        .into_response();
                }
            }
        }
        _ => {
            match state
                .subscription_service
                .generate_singbox(&sub, &node_infos, &user_keys)
            {
                Ok(c) => (c, "application/json", "config.json"),
                Err(e) => {
                    error!("Singbox gen failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed")
                        .into_response();
                }
            }
        }
    };

    // Cache
    let _ = state.redis.set(&cache_key, &content, 60).await; // 1 min cache

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename={}", filename).as_str(),
            ),
            (
                header::HeaderName::from_static("subscription-userinfo"),
                user_info_header.as_str(),
            ),
            (header::HeaderName::from_static("profile-title"), "CARAMBA"),
        ],
        content,
    )
        .into_response()
}
