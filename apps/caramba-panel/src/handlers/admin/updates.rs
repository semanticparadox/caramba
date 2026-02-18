use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse},
    http::StatusCode,
};
use crate::AppState;
use tracing::{info, error};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct UpdateNodeRequest {
    // Empty form simply triggers the action
}

pub async fn trigger_update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // 1. Get Latest Version
    let latest_version = state.settings.get_or_default("agent_latest_version", "0.0.0").await;

    // 2. Update Node Target Version
    let res = sqlx::query("UPDATE nodes SET target_version = $1 WHERE id = $2")
        .bind(&latest_version)
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            info!("🚀 Manual update triggered for Node {} to version {}", id, latest_version);
            
            // 3. Return Updated Row (HTMX Partial)
            // We need to re-render the row. For now, we can just return a success toast 
            // or trigger a client-side reload of the row.
            // Ideally, we'd render `partials/nodes_rows.html` for this single node.
            // But since that template loops, we might need a specific `partials/node_row.html`.
            // For simplicity in this phase, we'll return an OOB swap or just a success message.
            
            // Let's rely on HTMX to swap the button with "Update Pending..." text
            Html(format!(
                r#"<button class="px-3 py-1 bg-gray-500 text-white rounded cursor-not-allowed opacity-70" disabled>
                    Pending Update...
                </button>"#
            )).into_response()
        },
        Err(e) => {
            error!("Failed to trigger update for node {}: {}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to trigger update").into_response()
        }
    }
}
