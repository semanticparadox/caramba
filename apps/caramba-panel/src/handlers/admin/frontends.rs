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
        panel_url
    };

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
