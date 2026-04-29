use askama::Template;
use axum::{
    extract::{Form, State},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use super::auth::{get_auth_user, is_authenticated};
use crate::AppState;

#[derive(Template)]
#[template(path = "updates.html")]
pub struct UpdatesTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,

    // Panel/Hub
    pub panel_current_version: String,
    pub panel_latest_version: String,
    pub panel_update_available: bool,

    // Agent Rollout
    pub agent_latest_version: String,
    pub agent_update_url: String,
    pub active_nodes_count: usize,
    pub auto_update_agents: bool,

    // Worker Status
    pub worker_inventory: Vec<crate::handlers::admin::settings::WorkerInventoryView>,
}

#[derive(Deserialize)]
pub struct TriggerUpdateForm {
    pub target_id: Option<i64>, // Specific node ID or None for all
}

pub async fn get_updates_page(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return axum::response::Redirect::to(&format!("{}/login", state.admin_path))
            .into_response();
    }

    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or("Admin".to_string());

    // Panel Version Logic
    let panel_current = crate::utils::current_panel_version();
    // In a real scenario, you might fetch latest from GitHub or cache.
    // For now, we reuse the logic from settings check if available, or just display current.
    // We'll reuse the `resolve_latest_release_version` logic if we move it to a shared helper,
    // or just display what we know.
    // Let's assume we fetch it fresh or from settings cache.
    let panel_latest = state
        .settings
        .get_or_default("panel_latest_version_cache", &panel_current)
        .await;
    // Compare versions ignoring an optional `v` prefix on either side
    // ("v0.9.47" == "0.9.47"), so a GitHub-tagged release doesn't appear
    // perpetually different from the local crate version.
    let normalize = |v: &str| v.trim().trim_start_matches('v').to_string();
    let panel_update_available = normalize(&panel_latest) != normalize(&panel_current);

    // Agent Config
    let agent_latest_version = state
        .settings
        .get_or_default("agent_latest_version", "0.0.0")
        .await;
    let agent_update_url = state.settings.get_or_default("agent_update_url", "").await;
    let auto_update_agents = state
        .settings
        .get_bool_or_default("auto_update_agents", true)
        .await;

    let active_nodes = state
        .infrastructure_service
        .get_active_nodes()
        .await
        .unwrap_or_default();

    let worker_inventory =
        crate::handlers::admin::settings::fetch_worker_inventory(&state.pool).await;

    let template = UpdatesTemplate {
        is_auth: true,
        username,
        admin_path: state.admin_path.clone(),
        active_page: "updates".to_string(),
        panel_current_version: panel_current,
        panel_latest_version: panel_latest,
        panel_update_available,
        agent_latest_version,
        agent_update_url,
        active_nodes_count: active_nodes.len(),
        auto_update_agents,
        worker_inventory,
    };

    Html(template.render().unwrap_or_default()).into_response()
}

/// POST /admin/updates/recheck
/// Re-fetches latest panel release version from GitHub and updates the
/// `panel_latest_version_cache` setting. The Updates page renders a
/// stale cache by default — this lets the operator force a fresh check
/// without restarting the panel.
pub async fn recheck_panel_version(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    use super::auth::get_auth_user;
    if get_auth_user(&state, &jar).await.is_none() {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    match super::settings::resolve_latest_release_version_pub().await {
        Ok(version) => {
            let _ = state
                .settings
                .set("panel_latest_version_cache", &version)
                .await;
            (
                axum::http::StatusCode::OK,
                format!("Re-checked. Latest stable release: {}", version),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("recheck_panel_version failed: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch latest release: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn trigger_update(
    State(state): State<AppState>,
    Form(form): Form<TriggerUpdateForm>,
) -> impl IntoResponse {
    let target_version = state
        .settings
        .get_or_default("agent_latest_version", "")
        .await;
    if target_version.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "No target agent version configured",
        )
            .into_response();
    }

    if let Some(node_id) = form.target_id {
        // Update single node
        let _ = sqlx::query("UPDATE nodes SET target_version = $1 WHERE id = $2")
            .bind(&target_version)
            .bind(node_id)
            .execute(&state.pool)
            .await;
        let _ = state
            .orchestration_service
            .notify_node_update(node_id)
            .await;
        return (axum::http::StatusCode::OK, "Update signal sent to node").into_response();
    } else {
        // Rollout to all active
        let active_nodes = state
            .infrastructure_service
            .get_active_nodes()
            .await
            .unwrap_or_default();
        let mut count = 0;
        for node in active_nodes {
            let _ = sqlx::query("UPDATE nodes SET target_version = $1 WHERE id = $2")
                .bind(&target_version)
                .bind(node.id)
                .execute(&state.pool)
                .await;
            if state
                .orchestration_service
                .notify_node_update(node.id)
                .await
                .is_ok()
            {
                count += 1;
            }
        }
        return (
            axum::http::StatusCode::OK,
            format!("Rollout signal sent to {} nodes", count),
        )
            .into_response();
    }
}

