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

fn parse_ip_maybe(value: &str) -> Option<std::net::IpAddr> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Ok(ip) = value.parse::<std::net::IpAddr>() {
        return Some(canonicalize_ip(ip));
    }
    if let Ok(sock) = value.parse::<std::net::SocketAddr>() {
        return Some(canonicalize_ip(sock.ip()));
    }
    if let Some((host, _port)) = value.rsplit_once(':') {
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            return Some(canonicalize_ip(ip));
        }
    }
    None
}

fn canonicalize_ip(ip: std::net::IpAddr) -> std::net::IpAddr {
    match ip {
        std::net::IpAddr::V6(v6) => v6
            .to_ipv4()
            .map(std::net::IpAddr::V4)
            .unwrap_or(std::net::IpAddr::V6(v6)),
        other => other,
    }
}

fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
    let raw = headers
        .get("cf-connecting-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0");

    parse_ip_maybe(raw)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

/// Check if an IP belongs to major Russian ASN allocations.
/// Covers primary Russian CIDR blocks (Rostelecom, MTS, Beeline, MegaFon, Yandex).
fn is_russian_ip(ip_str: &str) -> bool {
    let ip: std::net::IpAddr = match ip_str.parse() {
        Ok(ip) => ip,
        Err(_) => return false,
    };
    let ipv4 = match ip {
        std::net::IpAddr::V4(v4) => v4,
        std::net::IpAddr::V6(_) => return false,
    };
    let octets = ipv4.octets();
    let (a, b) = (octets[0], octets[1]);
    #[allow(clippy::match_like_matches_macro)]
    match (a, b) {
        // Rostelecom, MTS, Beeline, MegaFon, Yandex and other major RU allocations
        (2, 60..=63)
        | (5, 3..=5)
        | (5, 8..=9)
        | (5, 16..=18)
        | (5, 34..=35)
        | (5, 44..=45)
        | (5, 100..=104)
        | (5, 128..=131)
        | (5, 164..=167)
        | (5, 187..=189)
        | (5, 228..=229)
        | (5, 248..=255)
        | (31, 13)
        | (31, 28..=31)
        | (31, 40..=44)
        | (31, 128..=134)
        | (31, 173)
        | (31, 180..=184)
        | (37, 1)
        | (37, 9)
        | (37, 18..=19)
        | (37, 26..=29)
        | (37, 44..=45)
        | (37, 110)
        | (37, 112)
        | (37, 140)
        | (37, 144..=145)
        | (37, 192..=193)
        | (37, 204..=205)
        | (37, 228..=230)
        | (46, 0..=3)
        | (46, 8..=10)
        | (46, 16..=20)
        | (46, 28..=30)
        | (46, 34..=48)
        | (46, 146..=148)
        | (46, 158..=160)
        | (46, 172..=175)
        | (46, 226..=228)
        | (46, 236..=243)
        | (62, 76..=77)
        | (62, 89)
        | (62, 109)
        | (62, 117..=118)
        | (62, 133)
        | (62, 176..=177)
        | (62, 181..=182)
        | (62, 205)
        | (62, 213)
        | (62, 220..=221)
        | (77, 34..=37)
        | (77, 40..=41)
        | (77, 50..=51)
        | (77, 66..=67)
        | (77, 72..=75)
        | (77, 82..=83)
        | (77, 88)
        | (77, 91)
        | (77, 94..=95)
        | (77, 105..=106)
        | (77, 220..=223)
        | (77, 232..=247)
        | (78, 25)
        | (78, 29..=31)
        | (78, 36..=37)
        | (78, 85)
        | (78, 106..=107)
        | (78, 153..=155)
        | (79, 104..=105)
        | (79, 120..=126)
        | (79, 133..=134)
        | (79, 137..=142)
        | (79, 164..=175)
        | (80, 64..=69)
        | (80, 73)
        | (80, 76..=77)
        | (80, 83..=91)
        | (80, 237..=254)
        | (81, 16..=26)
        | (81, 176..=177)
        | (81, 195)
        | (81, 200..=201)
        | (81, 211)
        | (81, 222)
        | (83, 149)
        | (83, 166..=167)
        | (83, 172..=173)
        | (83, 219..=222)
        | (83, 234..=237)
        | (85, 21..=22)
        | (85, 26)
        | (85, 90)
        | (85, 92..=95)
        | (85, 115..=116)
        | (85, 140..=143)
        | (85, 172..=176)
        | (85, 192..=199)
        | (85, 234..=237)
        | (85, 249)
        | (87, 103..=104)
        | (87, 117)
        | (87, 224..=226)
        | (87, 228..=250)
        | (88, 68..=69)
        | (88, 81..=87)
        | (88, 135)
        | (88, 196..=210)
        | (89, 22)
        | (89, 109..=110)
        | (89, 169)
        | (89, 175..=179)
        | (89, 184..=185)
        | (89, 204..=208)
        | (89, 218..=222)
        | (89, 249..=253)
        | (90, 150..=155)
        | (90, 176..=179)
        | (90, 188..=189)
        | (91, 77..=79)
        | (91, 103..=105)
        | (91, 122)
        | (91, 132..=133)
        | (91, 188..=228)
        | (91, 234..=245)
        | (92, 37..=39)
        | (92, 50..=51)
        | (92, 100..=101)
        | (92, 112)
        | (92, 124..=125)
        | (92, 240..=243)
        | (93, 72)
        | (93, 80..=85)
        | (93, 100..=101)
        | (93, 153)
        | (93, 170..=171)
        | (93, 178..=191)
        | (94, 19..=20)
        | (94, 24..=26)
        | (94, 41..=43)
        | (94, 50..=51)
        | (94, 72..=73)
        | (94, 79..=80)
        | (94, 100..=102)
        | (94, 124..=125)
        | (94, 130..=131)
        | (94, 137)
        | (94, 139..=143)
        | (94, 153..=159)
        | (94, 180..=181)
        | (94, 228..=251)
        | (95, 24..=25)
        | (95, 54..=55)
        | (95, 69..=79)
        | (95, 104..=106)
        | (95, 142..=143)
        | (95, 154..=167)
        | (95, 173..=174)
        | (95, 180..=189)
        | (109, 110..=111)
        | (109, 124..=127)
        | (109, 164..=167)
        | (109, 184..=185)
        | (109, 194..=207)
        | (109, 224..=227)
        | (109, 234..=253)
        | (176, 14..=15)
        | (176, 28)
        | (176, 36)
        | (176, 49..=51)
        | (176, 59..=60)
        | (176, 96..=119)
        | (176, 195..=215)
        | (176, 226)
        | (176, 233..=234)
        | (178, 16..=19)
        | (178, 34..=49)
        | (178, 66..=72)
        | (178, 120..=121)
        | (178, 126..=127)
        | (178, 140)
        | (178, 166..=179)
        | (178, 208..=238)
        | (188, 16..=17)
        | (188, 32..=45)
        | (188, 64..=69)
        | (188, 113)
        | (188, 116..=134)
        | (188, 162..=170)
        | (188, 186..=187)
        | (188, 226..=234)
        | (193, 0..=3)
        | (193, 18..=27)
        | (193, 32..=33)
        | (193, 41)
        | (193, 58)
        | (193, 104..=109)
        | (193, 124)
        | (193, 148..=150)
        | (193, 164)
        | (193, 176)
        | (193, 232..=243)
        | (194, 8..=9)
        | (194, 28..=29)
        | (194, 50..=58)
        | (194, 67)
        | (194, 79)
        | (194, 84..=87)
        | (194, 105)
        | (194, 135)
        | (194, 150..=152)
        | (194, 186..=190)
        | (194, 226..=228)
        | (195, 2..=3)
        | (195, 16..=19)
        | (195, 34..=47)
        | (195, 54..=56)
        | (195, 62)
        | (195, 68..=70)
        | (195, 80..=91)
        | (195, 128..=133)
        | (195, 140..=149)
        | (195, 161..=162)
        | (195, 170..=171)
        | (195, 182..=189)
        | (195, 208..=211)
        | (195, 218..=219)
        | (195, 230..=243)
        | (212, 1)
        | (212, 3..=5)
        | (212, 14..=22)
        | (212, 32..=34)
        | (212, 41..=42)
        | (212, 46..=62)
        | (212, 106..=120)
        | (212, 148..=166)
        | (212, 176..=195)
        | (212, 220..=222)
        | (213, 5)
        | (213, 24..=25)
        | (213, 33..=34)
        | (213, 59)
        | (213, 79..=80)
        | (213, 87)
        | (213, 108..=109)
        | (213, 166..=180)
        | (213, 219..=234)
        | (217, 8..=10)
        | (217, 14..=15)
        | (217, 24..=25)
        | (217, 65..=73)
        | (217, 106..=119)
        | (217, 148..=150)
        | (217, 170..=175)
        | (217, 195..=197)
        | (217, 212) => true,
        _ => false,
    }
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
    let client_ip = extract_client_ip(req.headers());

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

    // 3.2 Check traffic quota immediately on subscription fetch to enforce limits in real-time.
    match state
        .subscription_service
        .ensure_subscription_within_quota(sub.id)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                "Traffic limit reached. Subscription is expired.",
            )
                .into_response();
        }
        Err(e) => {
            error!(
                "Failed to evaluate quota for subscription {}: {}",
                sub.id, e
            );
        }
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
    // Fallback to all active nodes if plan bindings are temporarily missing.
    let mut nodes_raw = match state.store_service.get_user_nodes(sub.user_id).await {
        Ok(nodes) if !nodes.is_empty() => nodes,
        Ok(_) => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
            }
        },
        Err(_) => match state.store_service.get_active_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => {
                return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
            }
        },
    };
    if nodes_raw.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "No servers available").into_response();
    }

    let mut filtered_nodes = if let Some(nid) = params.node_id {
        nodes_raw
            .iter()
            .filter(|n| n.id == nid)
            .cloned()
            .collect::<Vec<_>>()
    } else if let Some(pinned_id) = sub.node_id {
        nodes_raw
            .iter()
            .filter(|n| n.id == pinned_id)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        nodes_raw.clone()
    };

    // If subscription has stale pinned node (deleted/disabled), fall back to full set.
    if filtered_nodes.is_empty() && params.node_id.is_none() {
        filtered_nodes = std::mem::take(&mut nodes_raw);
    }

    // Geo-aware ordering: always exclude pure relay infrastructure nodes from
    // user subscriptions (is_relay == true nodes are transit-only infra).
    // Then for Russian IPs, put relay-chained nodes first so that sing-box /
    // Clash auto-select picks a working relay path when RKN whitelists are
    // active, while still including direct paths as an auto-fallback (if direct
    // works it will be faster and auto-test will prefer it).
    // Only apply when the user has not explicitly pinned a node.
    if params.node_id.is_none() && sub.node_id.is_none() {
        // Always remove pure relay infrastructure nodes – they are not
        // user-facing destinations, only inter-node transport hops.
        filtered_nodes.retain(|n| !n.is_relay);

        if filtered_nodes.len() > 1 {
            let is_russian = is_russian_ip(&client_ip);

            if is_russian {
                // For Russian users we want:
                //   1. Relay-chained destination nodes (relay_id.is_some()) → first.
                //   2. Direct destination nodes (relay_id.is_none())        → fallback.
                //
                // Both end up in the same sing-box config. The URLTest auto
                // group picks the fastest working path.  When whitelists block
                // direct foreign routes the relay paths win automatically.
                let (mut relay_nodes, direct_nodes): (Vec<_>, Vec<_>) = filtered_nodes
                    .iter()
                    .cloned()
                    .partition(|n| n.relay_id.is_some());

                if !relay_nodes.is_empty() {
                    relay_nodes.extend(direct_nodes);
                    filtered_nodes = relay_nodes;
                }
                // If no relay nodes are configured yet, keep all direct nodes.
            } else {
                // Non-Russian users: prefer direct nodes (lowest latency).
                // Still keep relay-chained ones in the list – they won't be
                // primary but a user can manually select them via the selector.
                let direct_nodes: Vec<_> = filtered_nodes
                    .iter()
                    .filter(|n| n.relay_id.is_none())
                    .cloned()
                    .collect();
                if !direct_nodes.is_empty() {
                    filtered_nodes = direct_nodes;
                }
            }
        }
    }

    if filtered_nodes.is_empty() {
        return (StatusCode::NOT_FOUND, "Requested server not found").into_response();
    }

    // Persist last explicitly selected node so UI/miniapp can show where the user last pulled config from.
    if let Some(selected_node_id) = params.node_id {
        if filtered_nodes.iter().any(|n| n.id == selected_node_id) {
            let _ = state
                .subscription_service
                .update_subscription_node(sub.id, Some(selected_node_id))
                .await;
        }
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
        let _filename = match client_type {
            "clash" => "config.yaml",
            "v2ray" => "config.txt",
            _ => "config.json",
        };
        let content_type = match client_type {
            "clash" => "text/yaml; charset=utf-8",
            "v2ray" => "text/plain; charset=utf-8",
            _ => "application/json; charset=utf-8",
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    user_info_header.as_str(),
                ),
                (
                    header::HeaderName::from_static("profile-title"),
                    plan_details.0.as_str(),
                ),
                (
                    header::HeaderName::from_static("profile-update-interval"),
                    "2",
                ),
            ],
            cached_config,
        )
            .into_response();
    }

    let (content, content_type, _filename): (String, &'static str, &'static str) = match client_type
    {
        "clash" => {
            match state
                .subscription_service
                .generate_clash(&sub, &node_infos, &user_keys)
            {
                Ok(c) => (c, "text/yaml; charset=utf-8", "config.yaml"),
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
                Ok(c) => (c, "text/plain; charset=utf-8", "config.txt"),
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
                Ok(c) => (c, "application/json; charset=utf-8", "config.json"),
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
                header::HeaderName::from_static("subscription-userinfo"),
                user_info_header.as_str(),
            ),
            (
                header::HeaderName::from_static("profile-title"),
                plan_details.0.as_str(),
            ),
            (
                header::HeaderName::from_static("profile-update-interval"),
                "2",
            ),
        ],
        content,
    )
        .into_response()
}
