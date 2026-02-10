use axum::{
    extract::{State, Form, Path},
    response::{IntoResponse, Html, Redirect},
    http::HeaderMap,
};
use askama::Template;
use askama_web::WebTemplate;
use tracing::{info, error};
use uuid::Uuid;

use crate::AppState;
use crate::models::api_key::ApiKey;

#[derive(Template, WebTemplate)]
#[template(path = "api_keys.html")]
pub struct ApiKeysTemplate {
    pub keys: Vec<ApiKey>,
    pub admin_path: String,
    pub active_page: String,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let keys = state.store_service.get_api_keys().await.unwrap_or_default();
    
    let template = ApiKeysTemplate {
        keys,
        admin_path: state.admin_path.clone(),
        active_page: "api_keys".to_string(),
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn create_api_key(
    State(state): State<AppState>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let name = form.get("name").cloned().unwrap_or_default();
    let max_uses_str = form.get("max_uses").cloned().unwrap_or_default();
    
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Name is required").into_response();
    }

    let max_uses = max_uses_str.parse::<i64>().ok().filter(|&x| x > 0);
    
    // Generate a secure random key
    let key = format!("EXA-ENROLL-{}", Uuid::new_v4().to_string().to_uppercase());

    if let Err(e) = state.store_service.create_api_key(&name, &key, max_uses).await {
        error!("Failed to create API key: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to create API key").into_response();
    }

    let admin_path = state.admin_path.clone();
    Redirect::to(&format!("{}/api-keys", admin_path)).into_response()
}

pub async fn delete_api_key(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(e) = state.store_service.delete_api_key(id).await {
        error!("Failed to delete API key: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete API key").into_response();
    }

    let admin_path = state.admin_path.clone();
    Redirect::to(&format!("{}/api-keys", admin_path)).into_response()
}
