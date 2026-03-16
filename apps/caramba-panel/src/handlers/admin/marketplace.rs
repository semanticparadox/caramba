use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use super::auth::get_auth_user;
use crate::AppState;

#[derive(Template, WebTemplate)]
#[template(path = "marketplace.html")]
pub struct MarketplaceTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,

    // Setting fields
    pub nowpayments_key: String,
    pub cryptobot_token: String,
    pub stars_enabled: bool,
    pub manual_enabled: bool,

    // Manual pending approvals
    pub pending_manual: Vec<caramba_db::models::store::PaymentSession>,
}

#[derive(Deserialize)]
pub struct MarketplaceSettingsForm {
    pub nowpayments_key: Option<String>,
    pub cryptobot_token: Option<String>,
    pub stars_enabled: Option<String>,
    pub manual_enabled: Option<String>,
}

pub async fn get_marketplace_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let nowpayments_key = state.settings.get_or_default("nowpayments_key", "").await;
    let cryptobot_token = state.settings.get_or_default("cryptobot_token", "").await;
    let stars_enabled = state
        .settings
        .get_or_default("stars_enabled", "false")
        .await
        == "true";
    let manual_enabled = state
        .settings
        .get_or_default("manual_enabled", "false")
        .await
        == "true";

    // Fetch pending manual sessions
    let pending_manual = state
        .marketplace_service
        .session_repo
        .get_pending_by_provider("manual")
        .await
        .unwrap_or_default();

    let template = MarketplaceTemplate {
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(),
        active_page: "marketplace".to_string(),
        nowpayments_key,
        cryptobot_token,
        stars_enabled,
        manual_enabled,
        pending_manual,
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

pub async fn save_marketplace_settings(
    State(state): State<AppState>,
    Form(form): Form<MarketplaceSettingsForm>,
) -> impl IntoResponse {
    let admin_path = state.admin_path.clone();

    let _ = state
        .settings
        .set("nowpayments_key", &form.nowpayments_key.unwrap_or_default())
        .await;
    let _ = state
        .settings
        .set("cryptobot_token", &form.cryptobot_token.unwrap_or_default())
        .await;

    let stars_val = if form.stars_enabled.is_some() {
        "true"
    } else {
        "false"
    };
    let _ = state.settings.set("stars_enabled", stars_val).await;

    let manual_val = if form.manual_enabled.is_some() {
        "true"
    } else {
        "false"
    };
    let _ = state.settings.set("manual_enabled", manual_val).await;

    // We use HX-Redirect for HTMX
    (
        axum::http::StatusCode::OK,
        [("HX-Redirect", format!("{}/marketplace", admin_path))],
        "Settings saved",
    )
        .into_response()
}

pub async fn approve_manual_payment(
    Path(id): Path<uuid::Uuid>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // 1. Mark as completed and execute product provisioning logic
    if let Err(e) = state.marketplace_service.fulfill_payment(id).await {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to approve and fulfill: {}", e),
        )
            .into_response();
    }

    (axum::http::StatusCode::OK, "Approved").into_response()
}

pub async fn reject_manual_payment(
    Path(id): Path<uuid::Uuid>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(e) = state
        .marketplace_service
        .session_repo
        .update_status(id, "failed")
        .await
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to reject: {}", e),
        )
            .into_response();
    }

    (axum::http::StatusCode::OK, "Rejected").into_response()
}
