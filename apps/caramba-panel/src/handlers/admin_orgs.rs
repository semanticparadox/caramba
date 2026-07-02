use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use caramba_db::models::orgs::Organization;
use serde::Deserialize;

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

/// Resolve current admin's tg_id from session username via the admins table,
/// then look up the corresponding internal user.id (creating a placeholder
/// users row is out of scope — fallback chain mirrors how admin/tickets.rs
/// derives admin tg_id).
async fn resolve_admin_user_id(state: &AppState, jar: &CookieJar) -> i64 {
    let session_username = crate::handlers::admin::auth::get_auth_user(state, jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());

    let admin_tg_id: Option<i64> =
        sqlx::query_scalar("SELECT tg_id FROM admins WHERE username = $1 LIMIT 1")
            .bind(&session_username)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    if let Some(tg_id) = admin_tg_id {
        let user_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM users WHERE tg_id = $1 LIMIT 1")
                .bind(tg_id)
                .fetch_optional(&state.pool)
                .await
                .unwrap_or(None);
        if let Some(uid) = user_id {
            return uid;
        }
    }

    // Fallback: env var for legacy single-admin installs, then 0.
    std::env::var("ADMIN_DEFAULT_USER_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub async fn get_organizations(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let username = crate::handlers::admin::auth::get_auth_user(&state, &jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());
    let admin_path = state.admin_path.clone();

    // Admin view shows ALL organizations, not the org membership of a hardcoded user.
    match state.org_service.get_all_organizations().await {
        Ok(orgs) => {
            let template = OrgsTemplate {
                orgs,
                admin_path,
                is_auth: true,
                username,
                active_page: "orgs".to_string(),
            };
            Html(template.render().unwrap_or_default()).into_response()
        }
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_organization(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(payload): Form<CreateOrgRequest>,
) -> impl IntoResponse {
    let owner_id = resolve_admin_user_id(&state, &jar).await;
    if owner_id == 0 {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Could not resolve admin user_id — set ADMIN_DEFAULT_USER_ID or seed admins.tg_id",
        )
            .into_response();
    }
    match state
        .org_service
        .create_organization(owner_id, &payload.name, payload.slug.as_deref())
        .await
    {
        Ok(_) => {
            axum::response::Redirect::to(&format!("{}/orgs", state.admin_path)).into_response()
        }
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn delete_organization(
    Path(org_id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if crate::handlers::admin::auth::get_auth_user(&state, &jar)
        .await
        .is_none()
    {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    match state.org_service.delete_organization(org_id).await {
        Ok(_) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                "HX-Redirect",
                format!("{}/orgs", state.admin_path).parse().unwrap(),
            );
            (axum::http::StatusCode::OK, headers, "OK").into_response()
        }
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
