use super::auth::get_auth_user;
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use caramba_db::models::node::Node;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Template, WebTemplate)]
#[template(path = "frontends.html")]
pub struct FrontendsTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
    pub miniapp_enabled: bool,
    pub subscription_domain: String,
    pub tma_domain: String,
    pub relay_auth_mode: String,
    pub installer_bot_command: String,
    pub installer_enrollment_key: String,
    pub installer_sub_token: String,
    pub installer_sub_token_ready: bool,
    pub relay_nodes: Vec<Node>,
    pub subscription_entry_relay_id: i64,
    pub tma_entry_relay_id: i64,
    pub single_host_nginx_config: String,
    pub single_host_caddy_config: String,
    pub subscription_entry_label: String,
    pub tma_entry_label: String,
}

#[derive(Deserialize)]
pub struct SaveFrontendSettingsForm {
    pub miniapp_enabled: Option<String>,
    pub subscription_domain: Option<String>,
    pub tma_domain: Option<String>,
    pub relay_auth_mode: Option<String>,
    pub subscription_entry_relay_id: Option<i64>,
    pub tma_entry_relay_id: Option<i64>,
}

fn normalize_domain_like(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

fn build_public_url(domain: &str, fallback: &str) -> String {
    let normalized = normalize_domain_like(if domain.trim().is_empty() {
        fallback
    } else {
        domain
    });

    if normalized.is_empty() {
        "https://YOUR_PANEL_DOMAIN".to_string()
    } else {
        format!("https://{}", normalized)
    }
}

fn relay_label(relay_nodes: &[Node], selected_id: i64) -> String {
    relay_nodes
        .iter()
        .find(|node| node.id == selected_id)
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "Panel host reverse proxy".to_string())
}

fn build_single_host_nginx_config(
    panel_origin: &str,
    subscription_domain: &str,
    tma_domain: &str,
) -> String {
    format!(
        r#"map $http_upgrade $connection_upgrade {{
    default upgrade;
    '' close;
}}

server {{
    listen 80;
    server_name {subscription_domain} {tma_domain};
    return 301 https://$host$request_uri;
}}

server {{
    listen 443 ssl http2;
    server_name {subscription_domain} {tma_domain};

    ssl_certificate     /etc/letsencrypt/live/{subscription_domain}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{subscription_domain}/privkey.pem;

    location /sub/ {{
        proxy_pass {panel_origin};
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}

    location /app/ {{
        proxy_pass {panel_origin};
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}

    location /api/webhooks/ {{
        proxy_pass {panel_origin};
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}

    location / {{
        proxy_pass {panel_origin};
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}
}}"#,
        panel_origin = panel_origin,
        subscription_domain = subscription_domain,
        tma_domain = tma_domain,
    )
}

fn build_single_host_caddy_config(
    panel_origin: &str,
    subscription_domain: &str,
    tma_domain: &str,
) -> String {
    format!(
        r#"{subscription_domain}, {tma_domain} {{
    encode zstd gzip

    handle /sub/* {{
        reverse_proxy {panel_origin}
    }}

    handle /app/* {{
        reverse_proxy {panel_origin}
    }}

    handle /api/webhooks/* {{
        reverse_proxy {panel_origin}
    }}

    handle {{
        reverse_proxy {panel_origin}
    }}
}}"#,
        panel_origin = panel_origin
            .trim_start_matches("https://")
            .trim_start_matches("http://"),
        subscription_domain = subscription_domain,
        tma_domain = tma_domain,
    )
}

pub async fn get_frontends(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or("Admin".to_string());
    let admin_path = state.admin_path.clone();

    let miniapp_enabled = state
        .settings
        .get_or_default("miniapp_enabled", "false")
        .await
        == "true";
    let subscription_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let tma_domain = state.settings.get_or_default("tma_domain", "").await;
    let relay_auth_mode = state
        .settings
        .get_or_default("relay_auth_mode", "dual")
        .await;
    let subscription_entry_relay_id = state
        .settings
        .get_or_default("subscription_entry_relay_id", "0")
        .await
        .parse::<i64>()
        .unwrap_or(0);
    let tma_entry_relay_id = state
        .settings
        .get_or_default("tma_entry_relay_id", "0")
        .await
        .parse::<i64>()
        .unwrap_or(0);

    let panel_url = state.settings.get_or_default("panel_url", "").await;
    let panel_url_display = build_public_url(&panel_url, "YOUR_PANEL_DOMAIN");
    let subscription_host = normalize_domain_like(&subscription_domain);
    let subscription_host = if subscription_host.is_empty() {
        normalize_domain_like(&panel_url_display)
    } else {
        subscription_host
    };
    let tma_host = normalize_domain_like(&tma_domain);
    let tma_host = if tma_host.is_empty() {
        subscription_host.clone()
    } else {
        tma_host
    };

    let installer_enrollment_key = state
        .settings
        .get_or_default("installer_enrollment_key", "<ENROLLMENT_KEY>")
        .await;

    let internal_api_token = std::env::var("INTERNAL_API_TOKEN").unwrap_or_else(|_| {
        futures::executor::block_on(state.settings.get_or_default("internal_api_token", ""))
    });

    let installer_sub_token_ready = !internal_api_token.is_empty();
    let installer_sub_token = if installer_sub_token_ready {
        internal_api_token
    } else {
        "<INTERNAL_API_TOKEN>".to_string()
    };

    let installer_bot_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role bot --panel {} --panel-token {}",
        panel_url_display, panel_url_display, installer_sub_token
    );

    let relay_nodes = state
        .infrastructure_service
        .get_active_nodes()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|node| node.is_relay)
        .collect::<Vec<_>>();

    let template = FrontendsTemplate {
        is_auth: true,
        username,
        admin_path,
        active_page: "frontends".to_string(),
        miniapp_enabled,
        subscription_domain,
        tma_domain,
        relay_auth_mode,
        installer_bot_command,
        installer_enrollment_key,
        installer_sub_token,
        installer_sub_token_ready,
        relay_nodes: relay_nodes.clone(),
        subscription_entry_relay_id,
        tma_entry_relay_id,
        single_host_nginx_config: build_single_host_nginx_config(
            &panel_url_display,
            &subscription_host,
            &tma_host,
        ),
        single_host_caddy_config: build_single_host_caddy_config(
            &panel_url_display,
            &subscription_host,
            &tma_host,
        ),
        subscription_entry_label: relay_label(&relay_nodes, subscription_entry_relay_id),
        tma_entry_label: relay_label(&relay_nodes, tma_entry_relay_id),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
    }
}

pub async fn save_frontend_settings(
    State(state): State<AppState>,
    axum::extract::Form(form): axum::extract::Form<SaveFrontendSettingsForm>,
) -> impl IntoResponse {
    let mut settings = HashMap::new();
    let subscription_domain_input = form.subscription_domain.clone();
    let tma_domain_input = form.tma_domain.clone();

    settings.insert(
        "miniapp_enabled".to_string(),
        if form
            .miniapp_enabled
            .map(|v| v == "true" || v == "on")
            .unwrap_or(false)
        {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    if let Some(v) = subscription_domain_input.clone() {
        settings.insert("subscription_domain".to_string(), normalize_domain_like(&v));
    }

    if let Some(v) = tma_domain_input.clone() {
        let normalized = normalize_domain_like(&v);
        settings.insert("tma_domain".to_string(), normalized.clone());
        if !normalized.is_empty() {
            settings.insert("mini_app_url".to_string(), format!("https://{}/app", normalized));
        }
    }

    if let Some(v) = form.relay_auth_mode {
        settings.insert("relay_auth_mode".to_string(), v);
    }

    settings.insert(
        "subscription_entry_relay_id".to_string(),
        form.subscription_entry_relay_id.unwrap_or(0).to_string(),
    );
    settings.insert(
        "tma_entry_relay_id".to_string(),
        form.tma_entry_relay_id.unwrap_or(0).to_string(),
    );
    settings.insert("frontend_mode".to_string(), "single-host".to_string());

    if !settings.contains_key("mini_app_url") {
        let fallback_tma = subscription_domain_input
            .as_deref()
            .map(normalize_domain_like)
            .unwrap_or_default();
        if !fallback_tma.is_empty() {
            settings.insert("mini_app_url".to_string(), format!("https://{}/app", fallback_tma));
        }
    }

    match state.settings.set_multiple(settings).await {
        Ok(_) => ([("HX-Refresh", "true")], "Settings Saved").into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save settings: {}", e),
        )
            .into_response(),
    }
}

pub async fn get_frontends_rows(State(state): State<AppState>) -> impl IntoResponse {
    let subscription_entry_relay_id = state
        .settings
        .get_or_default("subscription_entry_relay_id", "0")
        .await
        .parse::<i64>()
        .unwrap_or(0);
    let tma_entry_relay_id = state
        .settings
        .get_or_default("tma_entry_relay_id", "0")
        .await
        .parse::<i64>()
        .unwrap_or(0);

    let relay_nodes = state
        .infrastructure_service
        .get_active_nodes()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|node| node.is_relay)
        .collect::<Vec<_>>();

    if relay_nodes.is_empty() {
        return Html(
            r#"<tr><td colspan="4" class="px-6 py-12 text-center text-slate-600 dark:text-slate-400">
                <i data-lucide="radio-tower" class="w-8 h-8 mx-auto mb-3 text-slate-500"></i>
                <p>No relay nodes detected yet.</p>
                <p class="text-xs mt-1">Mark a node as relay to expose it as a subscription or TMA entry point.</p>
            </td></tr>"#
                .to_string(),
        )
        .into_response();
    }

    let mut html = String::new();
    for relay in relay_nodes {
        let subscription_badge = if relay.id == subscription_entry_relay_id {
            "<span class=\"badge badge-warning\">Subscription</span>"
        } else {
            "" 
        };
        let tma_badge = if relay.id == tma_entry_relay_id {
            "<span class=\"badge badge-proto\">TMA</span>"
        } else {
            ""
        };

        html.push_str(&format!(
            r#"<tr class="hover:bg-white/5 transition-colors group">
                <td class="px-6 py-4 font-medium text-slate-900 dark:text-white">{name}</td>
                <td class="px-6 py-4 font-mono text-xs text-slate-400">{ip}</td>
                <td class="px-6 py-4 flex items-center gap-2">{subscription_badge}{tma_badge}</td>
                <td class="px-6 py-4 text-right text-xs text-slate-500">relay</td>
            </tr>"#,
            name = relay.name,
            ip = relay.ip,
            subscription_badge = subscription_badge,
            tma_badge = tma_badge,
        ));
    }

    Html(html).into_response()
}
