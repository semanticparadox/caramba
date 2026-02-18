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
use caramba_db::models::node::Node;
use super::auth::get_auth_user;
use chrono::Utc;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template)]
#[template(path = "nodes.html")]
pub struct NodesTemplate {
    pub nodes: Vec<Node>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
    // Phase 67
    pub agent_latest_version: String,
    pub auto_update_agents: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/nodes_rows.html")]
pub struct NodesRowsPartial {
    pub nodes: Vec<Node>,
    pub admin_path: String,
    // Phase 67
    pub agent_latest_version: String,
    pub auto_update_agents: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "node_edit_modal.html")]
pub struct NodeEditModalTemplate {
    pub node: Node,
    pub all_nodes: Vec<Node>, // Added for relay selection
    pub admin_path: String,
}

#[derive(askama::Template)]
#[template(path = "node_manual_install.html")]
pub struct NodeManualInstallTemplate {
    pub node_id: i64,
    pub node_name: String,
    pub join_token: String,
    pub admin_path: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "node_manage.html")]
pub struct NodeManageTemplate {
    pub node: Node,
    pub all_nodes: Vec<Node>, // For relay selection
    pub admin_path: String,
    pub username: String,
    pub active_page: String,
    pub is_auth: bool,
    pub inbounds: Vec<caramba_db::models::network::Inbound>,
    pub discovered_snis: Vec<NodeSniDisplay>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct NodeSniDisplay {
    pub id: i64,
    pub domain: String,
    pub is_pinned: bool,
    pub health_score: i32,
    pub is_premium: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "node_rescue_modal.html")]
pub struct NodeRescueModalTemplate {
    pub node: Node,
    pub admin_path: String,
}

// No custom filters needed, using Node methods instead.

#[derive(Deserialize)]
pub struct InstallNodeForm {
    pub name: String,
    pub ip: Option<String>,
    pub vpn_port: Option<i32>,
    pub auto_configure: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateNodeForm {
    pub name: String,
    pub ip: String,
    pub relay_id: Option<i64>,
    pub is_relay: Option<String>, // "on" or None from checkbox
    pub config_block_torrent: Option<String>,
    pub config_block_ads: Option<String>,
    pub config_block_porn: Option<String>,
    pub config_qos_enabled: Option<String>,
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

    // Fetch Update Settings (Phase 67)
    let agent_latest_version = state.settings.get_or_default("agent_latest_version", "0.0.0").await;
    let auto_update_agents: bool = state.settings.get_or_default("auto_update_agents", "true").await.parse().unwrap_or(true);

    if headers.contains_key("hx-request") {
        let template = NodesRowsPartial {
            nodes,
            admin_path,
            agent_latest_version,
            auto_update_agents,
        };
        return Html(template.render().unwrap()).into_response();
    }
    
    let template = NodesTemplate { 
        nodes, 
        is_auth: true, 
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "nodes".to_string(),
        agent_latest_version,
        auto_update_agents,
    };
    Html(template.render().unwrap()).into_response()
}

pub async fn install_node(
    State(state): State<AppState>,
    Form(form): Form<InstallNodeForm>,
) -> impl IntoResponse {
    let check_ip = form.ip.unwrap_or_default().trim().to_string();
    let vpn_port = form.vpn_port.unwrap_or(443).clamp(1, 65535);
    if !check_ip.is_empty() {
        info!("Adding node: {} @ {}", form.name, check_ip);
    } else {
        info!("Adding pending node: {}", form.name);
    }
    
    match state.infrastructure_service.create_node(&form.name, &check_ip, vpn_port, form.auto_configure.unwrap_or(false)).await {
        Ok(id) => {
            // Trigger default inbounds via orchestration
            if let Err(e) = state.orchestration_service.init_default_inbounds(id).await {
                error!("Failed to initialize inbounds for new node {}: {}", id, e);
            }
            
            // Always return installation modal with command+token, same as legacy flow.
            let join_token = sqlx::query_scalar::<_, String>(
                "SELECT COALESCE(join_token, '') FROM nodes WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

            let admin_path = state.admin_path.clone();
            let template = NodeManualInstallTemplate {
                node_id: id,
                node_name: form.name.clone(),
                join_token,
                admin_path,
            };

            let mut headers = HeaderMap::new();
            headers.insert("HX-Trigger", "refresh_nodes".parse().unwrap());

            let mut html = template.render().unwrap();
            html.push_str("<script>document.getElementById('add-node-modal').close(); document.getElementById('manual-install-modal').showModal();</script>");

            (headers, Html(html)).into_response()
        }
        Err(e) => {
            error!("Failed to insert node: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to add node: {}", e),
            )
                .into_response()
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
    let is_relay = form.is_relay.is_some();
    info!("Updating node ID: {} (Relay: {})", id, is_relay);
    
    // 1. Update core fields
    if let Err(e) = state.infrastructure_service.update_node(id, &form.name, &form.ip, form.relay_id, is_relay).await {
        error!("Failed to update node core: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update node").into_response();
    }

    // 2. Update security policies (Partial updates supported by HTMX)
    let b_torrent = form.config_block_torrent.is_some();
    let b_ads = form.config_block_ads.is_some();
    let b_porn = form.config_block_porn.is_some();
    let qos = form.config_qos_enabled.is_some();

    // If any policy is present in the form, it's likely a targeted update or a full form submission.
    // We update policies based on checkbox presence.
    let _ = sqlx::query("UPDATE nodes SET config_block_torrent = $1, config_block_ads = $2, config_block_porn = $3, config_qos_enabled = $4 WHERE id = $5")
        .bind(b_torrent)
        .bind(b_ads)
        .bind(b_porn)
        .bind(qos)
        .bind(id)
        .execute(&state.pool)
        .await;

    // 3. Trigger sync if policies changed (Policy changes need config regent)
    let _ = state.orchestration_service.reset_inbounds(id).await;
    let _ = state.pubsub.publish(&format!("node_events:{}", id), "update").await;

    let admin_path = state.admin_path.clone();
    let mut headers = HeaderMap::new();
    
    // Check if HTMX request
    if let Some(_) = form.config_block_torrent.as_ref().or(form.config_block_ads.as_ref()).or(form.config_block_porn.as_ref()).or(form.config_qos_enabled.as_ref()) {
        // Targeted update from managing page switches
        return (axum::http::StatusCode::OK, "Saved").into_response();
    }

    headers.insert("HX-Redirect", format!("{}/nodes", admin_path).parse().unwrap());
    (axum::http::StatusCode::OK, headers, "Updated").into_response()
}

pub async fn sync_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Manual sync triggered for node: {}", id);
    
    // Update trigger tracking
    let _ = sqlx::query("UPDATE nodes SET last_sync_trigger = 'Manual Update' WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

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

pub async fn test_node_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node = match state.infrastructure_service.get_node_by_id(id).await {
        Ok(n) => n,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
    };

    let is_online = node.last_seen.map(|ls| (Utc::now() - ls).num_seconds() < 60).unwrap_or(false);

    if is_online {
        (axum::http::StatusCode::OK, format!("✅ Node '{}' is ONLINE (Last seen: {}s ago)", node.name, (Utc::now() - node.last_seen.unwrap()).num_seconds())).into_response()
    } else {
        (axum::http::StatusCode::OK, format!("❌ Node '{}' is OFFLINE or UNREACHABLE", node.name)).into_response()
    }
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    info!("Request to delete node ID: {}", id);

    match state.infrastructure_service.delete_node(id).await {
        Ok(_) => {
            info!("Node {} deleted successfully", id);
            (
                axum::http::StatusCode::OK,
                [
                    ("HX-Trigger", "refresh_nodes"),
                ],
                "",
            ).into_response()
        }
        Err(e) => {
            error!("Failed to delete node {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                [
                    ("HX-Reswap", "none"),
                ],
                format!("Failed to delete node: {}", e),
            ).into_response()
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
    match state.infrastructure_service.activate_node(id, &state.security_service).await {
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
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let join_token = match sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(join_token, '') FROM nodes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(token)) => token,
        Ok(None) => {
            return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response();
        }
        Err(e) => {
            error!("Failed to load join token for node {}: {}", id, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load node token",
            )
                .into_response();
        }
    };

    if join_token.trim().is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Node token is empty").into_response();
    }

    let panel_url = std::env::var("PANEL_URL").ok().and_then(|v| {
        let trimmed = v.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() { None } else { Some(trimmed) }
    }).or_else(|| {
        headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|h| h.to_str().ok())
            .map(|host| {
                let proto = headers
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("https");
                format!("{}://{}", proto, host.trim_end_matches('/'))
            })
    }).unwrap_or_else(|| "https://YOUR_PANEL_DOMAIN".to_string());

    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
curl -fsSL '{panel_url}/install.sh' | sudo bash -s -- --role node --panel '{panel_url}' --token '{token}'
"#,
        panel_url = panel_url,
        token = join_token,
    );

    (
        [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
        script
    ).into_response()
}

pub async fn get_install_sh() -> impl IntoResponse {
    match crate::scripts::Scripts::get_universal_install_script() {
        Some(content) => (
            [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
            content
        ).into_response(),
        None => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Script not found").into_response(),
    }
}

pub async fn get_node_raw_install_script(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    get_node_install_script(Path(id), State(state), headers).await
}

pub async fn get_node_logs(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // 1. Check if logs are already in Redis
    if let Ok(Some(logs_json)) = state.redis.get(&format!("node_logs:{}", id)).await {
        return Html(format!(r###"
            <div class="space-y-4">
                <div class="flex justify-end">
                    <button hx-get="{}/nodes/{}/logs" hx-target="#logs-modal-content" class="text-xs text-indigo-400 hover:text-indigo-300">Refresh Logs</button>
                </div>
                <div class="custom-scrollbar overflow-auto max-h-[60vh] space-y-4 font-mono text-[11px]">
                    {}
                </div>
            </div>
        "###, state.admin_path, id, format_logs_html(&logs_json))).into_response();
    }

    // 2. If not, trigger collection and return "Waiting"
    // Optimization: Only set pending flag if it's not already set to prevent spamming the agent
    let pending: bool = sqlx::query_scalar("SELECT pending_log_collection FROM nodes WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);

    if !pending {
        let _ = sqlx::query("UPDATE nodes SET pending_log_collection = TRUE WHERE id = $1")
            .bind(id)
            .execute(&state.pool)
            .await;
    }

    Html(format!(r###"
        <div class="p-8 text-center" hx-get="{}/nodes/{}/logs" hx-trigger="every 3s" hx-target="this">
            <div class="animate-spin w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full mx-auto mb-4"></div>
            <p class="text-slate-400">Requesting real-time logs from agent...</p>
            <p class="text-[10px] text-slate-500 mt-2 italic">Ensure the node is online. This may take up to 30 seconds.</p>
        </div>
    "###, state.admin_path, id)).into_response()
}

fn format_logs_html(json_str: &str) -> String {
    let logs: std::collections::HashMap<String, String> = serde_json::from_str(json_str).unwrap_or_default();
    let mut html = String::new();

    for (service, content) in logs {
        html.push_str(&format!(r###"
            <div class="bg-slate-950/50 rounded-lg border border-white/5 overflow-hidden">
                <div class="bg-white/5 px-3 py-1.5 flex justify-between items-center text-[10px] uppercase font-bold text-slate-400">
                    <span>{}</span>
                </div>
                <pre class="p-3 text-emerald-400 overflow-x-auto">{}</pre>
            </div>
        "###, service, content));
    }

    html
}

pub async fn get_node_manage(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let node = match state.infrastructure_service.get_node_by_id(id).await {
        Ok(n) => n,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
    };

    let all_nodes = state.infrastructure_service.get_active_nodes().await.unwrap_or_default();
    let inbounds = state.infrastructure_service.get_node_inbounds(id).await.unwrap_or_default();
    
    // Fetch discovered SNIs with pinning status
    let discovered_snis = sqlx::query_as::<_, NodeSniDisplay>(
        r#"
        SELECT 
            s.id, 
            s.domain, 
            s.health_score, 
            s.is_premium,
            EXISTS(SELECT 1 FROM node_pinned_snis WHERE node_id = $1 AND sni_id = s.id) as is_pinned
        FROM sni_pool s
        WHERE s.discovered_by_node_id = $2 OR s.is_premium = TRUE
        ORDER BY is_pinned DESC, s.health_score DESC
        "#
    )
    .bind(id)
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    
    let admin_path = state.admin_path.clone();
    let username = get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string());

    let template = NodeManageTemplate {
        node,
        all_nodes,
        admin_path,
        username,
        active_page: "nodes".to_string(),
        is_auth: true,
        inbounds,
        discovered_snis,
    };

    Html(template.render().unwrap()).into_response()
}

pub async fn get_node_rescue(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node = match state.infrastructure_service.get_node_by_id(id).await {
        Ok(n) => n,
        Err(e) => return (axum::http::StatusCode::NOT_FOUND, format!("Node not found: {}", e)).into_response(),
    };

    let template = NodeRescueModalTemplate {
        node,
        admin_path: state.admin_path.clone(),
    };
    Html(template.render().unwrap()).into_response()
}

pub async fn trigger_scan(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Manual Neighbor Sniper scan triggered for node: {}", id);
    
    // Notify Agent via PubSub
    if let Err(e) = state.pubsub.publish(&format!("node_events:{}", id), "scan").await {
        error!("Failed to publish scan event: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Signal failed").into_response();
    }

    axum::http::StatusCode::ACCEPTED.into_response()
}
pub async fn pin_sni(
    Path((node_id, sni_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Pinning SNI {} for node {}", sni_id, node_id);
    
    let _ = sqlx::query("INSERT INTO node_pinned_snis (node_id, sni_id) VALUES ($1, $2) ON CONFLICT (node_id, sni_id) DO NOTHING")
        .bind(node_id)
        .bind(sni_id)
        .execute(&state.pool)
        .await;

    // Trigger sync to apply the pinned SNI
    let _ = state.orchestration_service.reset_inbounds(node_id).await;
    let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "update").await;

    let mut headers = HeaderMap::new();
    headers.insert("HX-Refresh", "true".parse().unwrap());
    (axum::http::StatusCode::OK, headers, "").into_response()
}

pub async fn unpin_sni(
    Path((node_id, sni_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Unpinning SNI {} for node {}", sni_id, node_id);
    
    let _ = sqlx::query("DELETE FROM node_pinned_snis WHERE node_id = $1 AND sni_id = $2")
        .bind(node_id)
        .bind(sni_id)
        .execute(&state.pool)
        .await;

    // Trigger sync
    let _ = state.orchestration_service.reset_inbounds(node_id).await;
    let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "update").await;

    let mut headers = HeaderMap::new();
    headers.insert("HX-Refresh", "true".parse().unwrap());
    (axum::http::StatusCode::OK, headers, "").into_response()
}

pub async fn block_sni(
    Path((node_id, sni_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Blocking SNI {} (requested from node {})", sni_id, node_id);
    
    // 1. Get Domain
    let domain: Option<String> = sqlx::query_scalar("SELECT domain FROM sni_pool WHERE id = $1")
        .bind(sni_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    if let Some(domain) = domain {
        // 2. Add to Blacklist
        let _ = sqlx::query("INSERT INTO sni_blacklist (domain, reason) VALUES ($1, $2) ON CONFLICT (domain) DO NOTHING")
            .bind(&domain)
            .bind(format!("Rejected by admin on node {}", node_id))
            .execute(&state.pool)
            .await;

        // 3. Delete from Pool
        let _ = sqlx::query("DELETE FROM sni_pool WHERE id = $1")
            .bind(sni_id)
            .execute(&state.pool)
            .await;
    }

    let mut headers = HeaderMap::new();
    headers.insert("HX-Refresh", "true".parse().unwrap());
    (axum::http::StatusCode::OK, headers, "").into_response()
}

pub async fn restart_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Manual restart triggered for node: {}", id);
    
    // Publish restart event
    if let Err(e) = state.pubsub.publish(&format!("node_events:{}", id), "restart").await {
        error!("Failed to publish restart event: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Signal failed").into_response();
    }

    // Optional: Update last_sync_trigger to denote manual intervention
    let _ = sqlx::query("UPDATE nodes SET last_sync_trigger = 'Manual Restart' WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

    (axum::http::StatusCode::OK, "Restart Signal Sent").into_response()
}

pub async fn rotate_node_inbounds(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Manual inbound rotation requested for node {}", node_id);

    let inbound_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM inbounds WHERE node_id = $1 AND tag LIKE 'tpl_%' AND enable = TRUE"
    )
    .bind(node_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    if inbound_ids.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "No templated inbounds found").into_response();
    }

    let mut rotated = 0usize;
    let mut failed = 0usize;
    for inbound_id in inbound_ids {
        match state.generator_service.rotate_inbound(inbound_id).await {
            Ok(_) => rotated += 1,
            Err(e) => {
                failed += 1;
                error!("Failed to rotate inbound {} on node {}: {}", inbound_id, node_id, e);
            }
        }
    }

    if rotated > 0 {
        let _ = sqlx::query("UPDATE nodes SET last_sync_trigger = 'Manual Rotation' WHERE id = $1")
            .bind(node_id)
            .execute(&state.pool)
            .await;
        let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "update").await;
    }

    (axum::http::StatusCode::OK, format!("Rotated {} inbound(s), {} failed", rotated, failed)).into_response()
}

pub async fn get_node_config_preview(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.orchestration_service.generate_node_config_json(id).await {
        Ok((_, config)) => {
            let json_str = serde_json::to_string_pretty(&config).unwrap_or_default();
            Html(format!(r###"
                <div class="p-6">
                    <div class="flex justify-between items-center mb-4">
                        <h3 class="text-lg font-bold text-white">Config Preview</h3>
                        <button onclick="document.getElementById('config-preview-modal').close()" class="text-slate-400 hover:text-white">
                            <i data-lucide="x" class="w-5 h-5"></i>
                        </button>
                    </div>
                    <div class="bg-slate-950 rounded-lg border border-white/10 p-4 max-h-[70vh] overflow-auto custom-scrollbar">
                        <pre class="text-xs font-mono text-emerald-400">{}</pre>
                    </div>
                    <div class="mt-4 flex justify-end">
                        <button onclick="copyConfig()" class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm font-medium">
                            Copy JSON
                        </button>
                    </div>
                    <script>
                        function copyConfig() {{
                            const content = document.querySelector('pre').innerText;
                            navigator.clipboard.writeText(content).then(() => {{
                                showToast('Config copied to clipboard');
                            }});
                        }}
                    </script>
                </div>
            "###, json_str)).into_response()
        },
        Err(e) => {
            error!("Failed to generate config for preview: {}", e);
             Html(format!(r###"
                <div class="p-6 text-center text-red-400">
                    <p class="font-bold">Failed to generate config</p>
                    <p class="text-sm mt-2">{}</p>
                    <button onclick="document.getElementById('config-preview-modal').close()" class="mt-4 px-4 py-2 bg-slate-800 text-white rounded-lg">Close</button>
                </div>
            "###, e)).into_response()
        }
    }
}
