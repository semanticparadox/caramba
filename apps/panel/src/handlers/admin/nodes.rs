// Nodes Module
// Node management, installation, configuration

use axum::{
    extract::{State, Form, Path},
    http::HeaderMap,
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use tracing::{info, error};

use crate::AppState;
use crate::models::node::Node;
use super::auth::get_auth_user;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "nodes.html")]
pub struct NodesTemplate {
    pub nodes: Vec<Node>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/nodes_rows.html")]
pub struct NodesRowsPartial {
    pub nodes: Vec<Node>,
    pub admin_path: String,
}

#[derive(askama::Template)]
#[template(path = "node_edit_modal.html")]
pub struct NodeEditModalTemplate {
    pub node: Node,
    pub all_nodes: Vec<Node>, // Added for relay selection
    pub admin_path: String,
}

#[derive(askama::Template)]
#[template(path = "node_manual_install.html")]
pub struct NodeManualInstallTemplate {
    pub node: Node,
    pub script: String,
    pub admin_path: String,
}

// Add empty filters module to satisfy Askama's resolution if it looks for it locally
pub mod filters {}

#[derive(Deserialize)]
pub struct InstallNodeForm {
    pub name: String,
    pub ip: Option<String>,
    pub vpn_port: i32,
    pub auto_configure: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateNodeForm {
    pub name: String,
    pub ip: String,
    pub relay_id: Option<i64>, // Added Phase 8
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_nodes(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> impl IntoResponse {
    let nodes = state.infrastructure_service.get_all_nodes().await.unwrap_or_default();
    
    let admin_path = state.admin_path.clone();

    if headers.contains_key("hx-request") {
        let template = NodesRowsPartial {
            nodes,
            admin_path,
        };
        return Html(template.render().unwrap()).into_response();
    }
    
    let template = NodesTemplate { 
        nodes, 
        is_auth: true, 
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "nodes".to_string(),
    };
    Html(template.render().unwrap()).into_response()
}

pub async fn install_node(
    State(state): State<AppState>,
    Form(form): Form<InstallNodeForm>,
) -> impl IntoResponse {
    let check_ip = form.ip.clone().unwrap_or_default();
    if !check_ip.is_empty() {
        info!("Adding node: {} @ {}", form.name, check_ip);
    } else {
        info!("Adding pending node: {}", form.name);
    }
    
    match state.infrastructure_service.create_node(&form.name, &check_ip, form.vpn_port, form.auto_configure.unwrap_or(false)).await {
        Ok(id) => {
            // Trigger default inbounds via orchestration
            if let Err(e) = state.orchestration_service.init_default_inbounds(id).await {
                error!("Failed to initialize inbounds for new node {}: {}", id, e);
            }
            
            // IF Smart Setup (No IP provided) -> Return the Setup Modal directly
            if check_ip.is_empty() {
                 let node = state.infrastructure_service.get_node_by_id(id).await.ok();
                 
                 if let Some(node) = node {
                     let script = crate::scripts::Scripts::get_setup_node_script().unwrap_or_default();

                     let admin_path = state.admin_path.clone();
                     let template = NodeManualInstallTemplate { node, script, admin_path };
                     
                     let mut headers = HeaderMap::new();
                     headers.insert("HX-Trigger", "refresh_nodes".parse().unwrap());
                     
                     // We still need the script to open the modal because hx-target is the modal content
                     let mut html = template.render().unwrap();
                     html.push_str("<script>document.getElementById('add-node-modal').close(); document.getElementById('manual-install-modal').showModal();</script>");
                     
                     return (headers, Html(html)).into_response();
                 }
            }

            let admin_path = state.admin_path.clone();
            let mut headers = HeaderMap::new();
            headers.insert("HX-Redirect", format!("{}/nodes", admin_path).parse().unwrap());
            (axum::http::StatusCode::OK, headers, "Redirecting...").into_response()
        }
        Err(e) => {
            error!("Failed to insert node: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to add node").into_response()
        }
    }
}

pub async fn get_node_edit(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node = match state.infrastructure_service.get_node_by_id(id).await {
            Ok(n) => n,
            Err(e) => {
                error!("Failed to fetch node for edit: {}", e);
                return Html(format!(r###"
                    <header>
                        <a href="#close" aria-label="Close" class="close" onclick="document.getElementById('edit-node-modal').close()"></a>
                        Error
                    </header>
                    <div style="padding: 1rem; color: #ff6b6b;">
                        <strong>Failed to load node:</strong><br>
                        {}<br><br>
                        <em>Please run database migrations.</em>
                    </div>
                    <footer><button onclick="document.getElementById('edit-node-modal').close()">Close</button></footer>
                "###, e)).into_response();
            }
        };

    let admin_path = state.admin_path.clone();
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let all_nodes = state.infrastructure_service.get_active_nodes().await.unwrap_or_default();

    let template = NodeEditModalTemplate { node, all_nodes, admin_path };
     match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn update_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateNodeForm>,
) -> impl IntoResponse {
    info!("Updating node ID: {} (Relay: {:?})", id, form.relay_id);
    
    match state.infrastructure_service.update_node(id, &form.name, &form.ip, form.relay_id).await {
        Ok(_) => {
             let admin_path = state.admin_path.clone();
             
             let mut headers = HeaderMap::new();
             headers.insert("HX-Redirect", format!("{}/nodes", admin_path).parse().unwrap());
             (axum::http::StatusCode::OK, headers, "Updated").into_response()
        },
        Err(e) => {
             error!("Failed to update node: {}", e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update node").into_response()
        }
    }
}

pub async fn sync_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Manual sync triggered for node: {}", id);
    
    let orch = state.orchestration_service.clone();
    let pubsub = state.pubsub.clone();

    tokio::spawn(async move {
        // Reset inbounds using orchestration service
        if let Err(e) = orch.reset_inbounds(id).await {
            error!("Failed to reset inbounds for node {}: {}", id, e);
        } else {
            info!("Successfully regenerated inbounds with fresh keys for node {}", id);
            
            // Notify Agent
            if let Err(e) = pubsub.publish(&format!("node_events:{}", id), "update").await {
                error!("Failed to publish update event: {}", e);
            }
        }
    });

    axum::http::StatusCode::ACCEPTED
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    info!("Request to delete node ID: {}", id);

    // Delete the node (now handled by service)
    match state.infrastructure_service.delete_node(id).await {
        Ok(_) => {
            info!("Node {} deleted successfully", id);
            (axum::http::StatusCode::OK, "").into_response()
        }
        Err(e) => {
            error!("Failed to delete node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete node: {}", e)).into_response()
        }
    }
}

pub async fn toggle_node_enable(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    info!("Request to toggle enable status for node ID: {}", id);
    
    // Toggle enable status
    match state.infrastructure_service.toggle_node_enable(id).await {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            (
                axum::http::StatusCode::OK,
                [("HX-Redirect", format!("{}/nodes", admin_path))],
                "Toggled"
            ).into_response()
        }
        Err(e) => {
            error!("Failed to toggle node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to toggle node").into_response()
        }
    }
}

pub async fn activate_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.infrastructure_service.activate_node(id).await {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            (
                axum::http::StatusCode::OK,
                [("HX-Redirect", format!("{}/nodes", admin_path))],
                "Activated"
            ).into_response()
        },
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to activate: {}", e)).into_response(),
    }
}

pub async fn get_node_install_script(
    Path(_id): Path<i64>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    match crate::scripts::Scripts::get_setup_node_script() {
        Some(content) => (
            [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
            content
        ).into_response(),
        None => {
            error!("Setup script not found in embedded assets");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Script not found").into_response()
        }
    }
}

pub async fn get_node_raw_install_script(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    get_node_install_script(Path(id), State(state)).await
}

pub async fn get_node_logs(
    Path(id): Path<i64>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // Stub for node logs
    Html(format!("<div class='p-4 text-slate-400'>Logs for node {} unavailable (Not implemented)</div>", id)).into_response()
}



