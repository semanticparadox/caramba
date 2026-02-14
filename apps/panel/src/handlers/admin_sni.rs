use axum::{
    extract::{State, Path, Form},
    response::{IntoResponse, Html},
};
use axum_extra::extract::cookie::CookieJar;
use crate::handlers::admin::{is_authenticated};
use askama::Template;
use askama_web::WebTemplate;
use serde::Deserialize;
use crate::AppState;
use crate::models::sni::SniPoolItem;
use crate::models::sni_log::SniRotationLog;
use tracing::{info, error};

#[derive(Template, WebTemplate)]
#[template(path = "admin_sni.html")]
pub struct AdminSniTemplate {
    pub snis: Vec<SniPoolItem>,
    pub logs: Vec<SniRotationLog>,
    pub active_sni_count: usize,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
    pub nodes: Vec<crate::models::node::Node>,
    pub filter_node_id: Option<i64>,
}

pub async fn get_sni_page(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let node_id = query.get("node_id").and_then(|id| id.parse::<i64>().ok());

    let snis = if let Some(id) = node_id {
        state.sni_repo.get_snis_by_node(id).await.unwrap_or_default()
    } else {
        state.sni_repo.get_all_snis().await.unwrap_or_default()
    };

    let logs = state.sni_repo.get_recent_logs(10).await.unwrap_or_default();
    let nodes = state.infrastructure_service.get_all_nodes().await.unwrap_or_default();
    let active_sni_count = snis.iter().filter(|s| s.is_active).count();
    
    let username = state.settings.get_or_default("admin_username", "admin").await;

    let template = AdminSniTemplate {
        snis,
        logs,
        active_sni_count,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "sni".to_string(),
        username,
        nodes,
        filter_node_id: node_id,
    };

    Html(template.render().unwrap()).into_response()
}

#[derive(Deserialize)]
pub struct AddSniForm {
    pub domain: String,
    pub tier: i32,
    pub notes: Option<String>,
}

pub async fn add_sni(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AddSniForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    match state.sni_repo.add_sni(&form.domain, form.tier, form.notes.as_deref()).await {
        Ok(_) => {
            info!("Added SNI {} to pool", form.domain);
            axum::response::Redirect::to(&format!("{}/sni", state.admin_path)).into_response()
        }
        Err(e) => {
            error!("Failed to add SNI: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to add SNI").into_response()
        }
    }
}

pub async fn delete_sni(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    match state.sni_repo.delete_sni(id).await {
        Ok(_) => {
            info!("Deleted SNI ID {} from pool", id);
            StatusCode::OK.into_response()
        }
        Err(e) => {
            error!("Failed to delete SNI: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete SNI").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct BulkSniForm {
    pub domains: String,
    pub tier: i32,
    pub notes: Option<String>,
}

pub async fn bulk_add_sni(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<BulkSniForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let domains: Vec<&str> = form.domains.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let mut count = 0;
    for domain in domains {
        if let Ok(_) = state.sni_repo.add_sni(domain, form.tier, form.notes.as_deref()).await {
            count += 1;
        }
    }

    info!("Bulk added {} SNIs to pool", count);
    axum::response::Redirect::to(&format!("{}/sni", state.admin_path)).into_response()
}

pub async fn toggle_sni(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    // Get current state to toggle
    let all = state.sni_repo.get_all_snis().await.unwrap_or_default();
    if let Some(item) = all.iter().find(|i| i.id == id) {
        match state.sni_repo.toggle_sni_active(id, !item.is_active).await {
            Ok(_) => StatusCode::OK.into_response(),
            Err(e) => {
                error!("Failed to toggle SNI: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to toggle SNI").into_response()
            }
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}
