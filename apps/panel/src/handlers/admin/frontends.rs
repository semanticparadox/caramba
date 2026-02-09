use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use crate::AppState;
use super::auth::get_auth_user;

#[derive(Template, WebTemplate)]
#[template(path = "frontends.html")]
pub struct FrontendsTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
    pub frontend_mode: String,
}

pub async fn get_frontends(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let username = get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string());
    let admin_path = state.admin_path.clone();
    let frontend_mode = state.settings.get_or_default("frontend_mode", "local").await;

    let template = FrontendsTemplate {
        is_auth: true,
        username,
        admin_path,
        active_page: "frontends".to_string(),
        frontend_mode,
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}
