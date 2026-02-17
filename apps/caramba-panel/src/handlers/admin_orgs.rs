use axum::{
    extract::{State, Form},
    response::{IntoResponse, Html},
};
use serde::{Deserialize};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use crate::AppState;
use caramba_db::models::orgs::Organization;

#[derive(Template, WebTemplate)]
#[template(path = "admin_orgs.html")]
pub struct OrgsTemplate {
    pub orgs: Vec<Organization>,
    pub admin_path: String,
    pub is_auth: bool,
    pub username: String,
    pub active_page: String,
}

#[derive(Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
    pub slug: Option<String>,
}

pub async fn get_organizations(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let username = crate::handlers::admin::auth::get_auth_user(&state, &jar).await
        .unwrap_or_else(|| "Admin".to_string());
    let admin_path = state.admin_path.clone();

    // Fetch all for admin view for now
    match state.org_service.get_user_organizations(1).await { // Mock user_id 1
        Ok(orgs) => {
            let template = OrgsTemplate {
                orgs,
                admin_path,
                is_auth: true,
                username,
                active_page: "orgs".to_string(),
            };
            Html(template.render().unwrap_or_default()).into_response()
        },
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_organization(
    State(state): State<AppState>,
    Form(payload): Form<CreateOrgRequest>,
) -> impl IntoResponse {
    match state.org_service.create_organization(1, &payload.name, payload.slug.as_deref()).await { // Mock user_id 1
        Ok(_) => axum::response::Redirect::to(&format!("{}/orgs", state.admin_path)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

