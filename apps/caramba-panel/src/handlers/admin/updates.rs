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
    let panel_update_available = panel_latest != panel_current; // Simplified semver check

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

    // Worker Inventory (Reuse logic from settings for now, ideally moved to a service)
    // We need to implement `fetch_worker_inventory` here or make it public in settings/mod.
    // For this refactor, let's duplicate the fetch logic or move it to `infrastructure_service`.
    // We'll assume we can call the SQL query directly here for speed.
    let worker_inventory = fetch_worker_inventory_shared(&state.pool).await;

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

    Html(template.render().unwrap()).into_response()
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

// Shared helper (duplicated from settings for now to avoid circular deps if settings isn't a lib)
// In a real refactor, this moves to `infrastructure_service`
async fn fetch_worker_inventory_shared(
    pool: &sqlx::PgPool,
) -> Vec<crate::handlers::admin::settings::WorkerInventoryView> {
    // Reuse struct from settings module if public, else duplicate definition locally
    use crate::handlers::admin::settings::WorkerInventoryView;

    // We need to fetch rows manually since we can't easily import private row structs
    #[derive(sqlx::FromRow)]
    struct WorkerRow {
        role: String,
        worker_id: String,
        current_version: Option<String>,
        target_version: Option<String>,
        last_state: String,
        last_message: Option<String>,
        last_seen: chrono::DateTime<chrono::Utc>,
    }

    let rows: Vec<WorkerRow> = sqlx::query_as(
        r#"
        SELECT role, worker_id, current_version, target_version, last_state, last_message, last_seen
        FROM worker_runtime_status
        ORDER BY role ASC, last_seen DESC
        LIMIT 200
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| {
            let is_online = chrono::Utc::now().signed_duration_since(row.last_seen)
                <= chrono::Duration::minutes(3);
            let current_v = row.current_version.unwrap_or_default();
            let target_v = row.target_version.unwrap_or_default();
            let update_available =
                crate::handlers::api::internal::should_offer_worker_update(&target_v, &current_v);
            WorkerInventoryView {
                role: row.role.to_ascii_uppercase(),
                worker_id: row.worker_id,
                current_version: if current_v.is_empty() { "-".into() } else { current_v },
                target_version: if target_v.is_empty() { "-".into() } else { target_v },
                last_state: row.last_state.to_ascii_uppercase(),
                last_message: row.last_message.unwrap_or_default(),
                is_online,
                online_label: if is_online {
                    "online".into()
                } else {
                    "offline".into()
                },
                last_seen: row.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                last_seen_ago: "just now".into(), // Simplified for this view
                update_available,
            }
        })
        .collect()
}
