use super::auth::get_auth_user;
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Template, WebTemplate)]
#[template(path = "frontends.html")]
pub struct FrontendsTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
    pub frontend_mode: String,
    pub miniapp_enabled: bool,
    pub subscription_domain: String,
    pub panel_url: String,
    pub mini_app_url: String,
    pub relay_auth_mode: String,
    pub installer_node_command: String,
    pub installer_sub_command: String,
    pub installer_bot_command: String,
    pub installer_enrollment_key: String,
    pub installer_sub_token: String,
    pub installer_sub_token_ready: bool,
}

#[derive(Deserialize)]
pub struct SaveFrontendSettingsForm {
    pub frontend_mode: Option<String>,
    pub miniapp_enabled: Option<String>,
    pub subscription_domain: Option<String>,
    pub panel_url: Option<String>,
    pub mini_app_url: Option<String>,
    pub relay_auth_mode: Option<String>,
}

pub async fn get_frontends(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or("Admin".to_string());
    let admin_path = state.admin_path.clone();
    let frontend_mode = state
        .settings
        .get_or_default("frontend_mode", "local")
        .await;
    let miniapp_enabled = state
        .settings
        .get_or_default("miniapp_enabled", "false")
        .await
        == "true";
    let subscription_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let relay_auth_mode = state
        .settings
        .get_or_default("relay_auth_mode", "dual")
        .await;

    let panel_url = state.settings.get_or_default("panel_url", "").await;
    let panel_url_display = if panel_url.is_empty() {
        "https://YOUR_PANEL_DOMAIN".to_string()
    } else {
        panel_url.clone()
    };

    let mini_app_url = state.settings.get_or_default("mini_app_url", "").await;

    let installer_enrollment_key = state
        .settings
        .get_or_default("installer_enrollment_key", "<ENROLLMENT_KEY>")
        .await;
    
    // Attempt to get internal token from settings or env
    let internal_api_token = std::env::var("INTERNAL_API_TOKEN").unwrap_or_else(|_| {
        futures::executor::block_on(state.settings.get_or_default("internal_api_token", ""))
    });
    
    let installer_sub_token_ready = !internal_api_token.is_empty();
    let installer_sub_token = if installer_sub_token_ready {
        internal_api_token
    } else {
        "<INTERNAL_API_TOKEN>".to_string()
    };


    let installer_node_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role node --panel {} --token {}",
        panel_url_display, panel_url_display, installer_enrollment_key
    );
    let installer_sub_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role sub --panel {} --token {} --region global",
        panel_url_display, panel_url_display, installer_sub_token
    );
    let installer_bot_command = format!(
        "curl -fsSL {}/install.sh | sudo bash -s -- --role bot --panel {} --panel-token {}",
        panel_url_display, panel_url_display, installer_sub_token
    );

    let template = FrontendsTemplate {
        is_auth: true,
        username,
        admin_path,
        active_page: "frontends".to_string(),
        frontend_mode,
        miniapp_enabled,
        subscription_domain,
        panel_url,
        mini_app_url,
        relay_auth_mode,
        installer_node_command,
        installer_sub_command,
        installer_bot_command,
        installer_enrollment_key,
        installer_sub_token,
        installer_sub_token_ready,
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

    if let Some(v) = form.frontend_mode {
        settings.insert("frontend_mode".to_string(), v);
    }

    settings.insert(
        "miniapp_enabled".to_string(),
        if form.miniapp_enabled.map(|v| v == "true" || v == "on").unwrap_or(false) {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    if let Some(v) = form.subscription_domain {
        settings.insert("subscription_domain".to_string(), v);
    }

    if let Some(v) = form.panel_url {
        settings.insert("panel_url".to_string(), v);
    }

    if let Some(v) = form.mini_app_url {
        settings.insert("mini_app_url".to_string(), v);
    }

    if let Some(v) = form.relay_auth_mode {
        settings.insert("relay_auth_mode".to_string(), v);
    }

    match state.settings.set_multiple(settings).await {
        Ok(_) => ([(("HX-Refresh", "true"))], "Settings Saved").into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save settings: {}", e),
        )
            .into_response(),
    }
}

/// GET /admin/partials/frontends_rows - HTML partial for frontend servers table
pub async fn get_frontends_rows(
    State(state): State<AppState>,
) -> impl IntoResponse {
    use sqlx::Row;

    let rows = sqlx::query(
        "SELECT id, domain, region, ip_address, is_active, last_heartbeat, traffic_monthly FROM frontend_servers ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) if rows.is_empty() => Html(
            r#"<tr><td colspan="7" class="px-6 py-12 text-center text-slate-600 dark:text-slate-400">
                <i data-lucide="server-off" class="w-8 h-8 mx-auto mb-3 text-slate-500"></i>
                <p>No frontend servers configured yet.</p>
                <p class="text-xs mt-1">Click "Add Frontend" to register your first distributed node.</p>
            </td></tr>"#.to_string()
        ).into_response(),
        Ok(rows) => {
            let mut html = String::new();
            for row in &rows {
                let id: i64 = row.try_get("id").unwrap_or(0);
                let domain: String = row.try_get("domain").unwrap_or_default();
                let region: String = row.try_get("region").unwrap_or_default();
                let ip: String = row.try_get("ip_address").unwrap_or_default();
                let is_active: bool = row.try_get("is_active").unwrap_or(false);
                let traffic: i64 = row.try_get("traffic_monthly").unwrap_or(0);
                let last_hb: Option<chrono::DateTime<chrono::Utc>> = row.try_get("last_heartbeat").ok();

                let status_class = if is_active { "badge-online" } else { "badge-offline" };
                let status_text = if is_active { "Active" } else { "Inactive" };
                let traffic_mb = traffic / 1024 / 1024;
                let _last_seen_text = last_hb
                    .map(|t| {
                        let ago = chrono::Utc::now() - t;
                        if ago.num_minutes() < 2 { "just now".to_string() }
                        else if ago.num_hours() < 1 { format!("{}m ago", ago.num_minutes()) }
                        else if ago.num_days() < 1 { format!("{}h ago", ago.num_hours()) }
                        else { format!("{}d ago", ago.num_days()) }
                    })
                    .unwrap_or_else(|| "never".to_string());

                html.push_str(&format!(
                    r#"<tr class="hover:bg-white/5 transition-colors group">
                        <td class="px-6 py-4 font-mono text-xs text-brand">{domain}</td>
                        <td class="px-6 py-4">
                            <span class="text-[10px] font-bold uppercase tracking-widest text-slate-500 bg-slate-800/50 px-2 py-1 rounded border border-white/5">{region}</span>
                        </td>
                        <td class="px-6 py-4 font-mono text-xs text-slate-400">{ip}</td>
                        <td class="px-6 py-4"><span class="badge {status_class}">{status_text}</span></td>
                        <td class="px-6 py-4">
                            <span class="font-bold text-slate-300">{traffic_mb}</span>
                            <span class="text-[10px] text-slate-500 uppercase ml-1">MB</span>
                        </td>
                        <td class="px-6 py-4 text-right">
                            <button hx-delete="/api/admin/frontends/{id}" hx-swap="none" hx-confirm="Remove frontend '{domain}'?"
                                class="btn-icon text-rose-500 bg-rose-500/10 border-rose-500/20 hover:bg-rose-500/20" title="Remove Frontend">
                                <i data-lucide="trash-2" class="w-4 h-4"></i>
                            </button>
                        </td>
                    </tr>"#
                ));
            }
            Html(html).into_response()
        },
        Err(_) => Html(
            r#"<tr><td colspan="7" class="px-6 py-12 text-center text-slate-600 dark:text-slate-400">
                <p>Frontend servers table not available yet.</p>
            </td></tr>"#.to_string()
        ).into_response(),
    }
}
