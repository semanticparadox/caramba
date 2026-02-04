use axum::{
    extract::{State, Path, Form},
    response::{IntoResponse, Html},
    body::Bytes,
};
use axum_extra::extract::cookie::CookieJar;
use crate::handlers::admin::get_auth_user;
use askama::Template;
use serde::Deserialize;
use crate::AppState;
use crate::models::node::Node;
use crate::models::network::Inbound;
use crate::models::store::Plan;
use tracing::{info, error};
use crate::utils::format_bytes;


#[derive(Template)]
#[template(path = "node_inbounds.html")]
pub struct NodeInboundsTemplate {
    pub node: Node,
    pub inbounds: Vec<Inbound>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String, // NEW
}

pub async fn get_node_inbounds(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(node_id): Path<i64>,
) -> impl IntoResponse {
    let node_res = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(&state.pool)
        .await;

    match node_res {
        Ok(Some(node)) => {
            let inbounds = sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds WHERE node_id = ? ORDER BY listen_port ASC")
                .bind(node_id)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();

            let template = NodeInboundsTemplate {
                node,
                inbounds,
                is_auth: true,
                admin_path: {
                    let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                    if p.starts_with('/') { p } else { format!("/{}", p) }
                },
                active_page: "nodes".to_string(),
                username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
            };
            Html(template.render().unwrap_or_default()).into_response()
        },
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
        Err(e) => {
            error!("DB Error: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct AddInboundForm {
    pub tag: String,
    pub protocol: String,
    pub listen_port: i64,
    pub listen_ip: String,
    pub settings: String,
    pub stream_settings: String,
}

pub async fn add_inbound(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
    Form(form): Form<AddInboundForm>,
) -> impl IntoResponse {
    info!("Adding inbound {} ({}) to node {}", form.tag, form.protocol, node_id);

    // Validate JSON against Models
    // 1. Stream Settings
    if let Err(e) = serde_json::from_str::<crate::models::network::StreamSettings>(&form.stream_settings) {
         return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid Stream Settings: {}", e)).into_response();
    }

    // 2. Protocol Settings
    match form.protocol.as_str() {
        "vless" => {
            if let Err(e) = serde_json::from_str::<crate::models::network::VlessSettings>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid VLESS Settings: {}", e)).into_response();
            }
        },
        "hysteria2" => {
            if let Err(e) = serde_json::from_str::<crate::models::network::Hysteria2Settings>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid Hysteria2 Settings: {}", e)).into_response();
            }
        },
        "trojan" => {
            if let Err(e) = serde_json::from_str::<crate::models::network::TrojanSettings>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid Trojan Settings: {}", e)).into_response();
            }
        },
        _ => {
            // Unknown protocol, just check valid JSON
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
            }
        }
    }

    // Check if port is already in use
    let port_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbounds WHERE node_id = ? AND listen_port = ?")
        .bind(node_id)
        .bind(form.listen_port)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    if port_count > 0 {
         return (axum::http::StatusCode::BAD_REQUEST, format!("Port {} is already used by another inbound on this node.", form.listen_port)).into_response();
    }

    let res = sqlx::query("INSERT INTO inbounds (node_id, tag, protocol, listen_port, listen_ip, settings, stream_settings) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(node_id)
        .bind(&form.tag)
        .bind(&form.protocol)
        .bind(form.listen_port)
        .bind(&form.listen_ip)
        .bind(&form.settings)
        .bind(&form.stream_settings)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            // PubSub Notify
            let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "update").await;

            // Redirect back to the list
             let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
             let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
             ([("HX-Redirect", format!("{}/nodes/{}/inbounds", admin_path, node_id))], "Redirecting...").into_response()
        },
        Err(e) => {
            error!("Failed to add inbound: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to add inbound: {}", e)).into_response()
        }
    }
}

pub async fn delete_inbound(
    State(state): State<AppState>,
    Path((node_id, inbound_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    info!("Deleting inbound {} from node {}", inbound_id, node_id);
    
    match sqlx::query("DELETE FROM inbounds WHERE id = ? AND node_id = ?")
        .bind(inbound_id)
        .bind(node_id)
        .execute(&state.pool)
        .await 
    {
        Ok(_) => {
            // PubSub Notify
            let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "update").await;
            axum::http::StatusCode::OK.into_response()
        },
        Err(e) => {
             error!("Failed to delete inbound: {}", e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete").into_response()
        }
    }
}

// --- Plan Bindings Handlers ---

#[derive(Template)]
#[template(path = "plan_bindings.html")]
pub struct PlanBindingsTemplate {
    pub plan: Plan,
    pub bindings: Vec<NodeBindingGroup>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String, // NEW
}

pub struct NodeBindingGroup {
    pub node_name: String,
    pub node_ip: String,
    pub inbounds: Vec<InboundBindingItem>,
}

pub struct InboundBindingItem {
    pub inbound: Inbound,
    pub is_bound: bool,
}

pub async fn get_plan_bindings(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(plan_id): Path<i64>,
) -> impl IntoResponse {
    // 1. Fetch Plan
    let plan = match sqlx::query_as::<_, Plan>("SELECT * FROM plans WHERE id = ?").bind(plan_id).fetch_optional(&state.pool).await {
        Ok(Some(p)) => p,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Plan not found").into_response(),
        Err(_) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response(),
    };

    // 2. Fetch All Active Nodes & Inbounds
    let nodes = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE status = 'active'").fetch_all(&state.pool).await.unwrap_or_default();
    
    // 3. Fetch Existing Bindings
    let bound_ids: Vec<i64> = sqlx::query_scalar("SELECT inbound_id FROM plan_inbounds WHERE plan_id = ?")
        .bind(plan_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut bindings = Vec::new();

    for node in nodes {
        let inbounds = sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds WHERE node_id = ? AND enable = 1 ORDER BY listen_port ASC")
            .bind(node.id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

        let items = inbounds.into_iter().map(|i| {
            let is_bound = bound_ids.contains(&i.id);
            InboundBindingItem { inbound: i, is_bound }
        }).collect();

        bindings.push(NodeBindingGroup {
            node_name: node.name,
            node_ip: node.ip,
            inbounds: items,
        });
    }

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    Html(PlanBindingsTemplate { plan, bindings, is_auth: true, admin_path, active_page: "plans".to_string(), username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()) }.render().unwrap_or_default()).into_response()
}

pub async fn save_plan_bindings(
    State(state): State<AppState>,
    Path(plan_id): Path<i64>,
    // Using Bytes to manually parse repeated form keys (checkboxes)
    body: Bytes,
) -> impl IntoResponse {
    let body_str = String::from_utf8(body.to_vec()).unwrap_or_default();
    let mut inbound_ids = Vec::new();
    
    // Manual parsing of "inbound_ids=1&inbound_ids=2..."
    for pair in body_str.split('&') {
        let mut parts = pair.split('=');
        if let Some(key) = parts.next() {
            if let Some(value) = parts.next() {
                // Decode URL-encoded key/value if needed, but for simple numeric IDs it's fine.
                // Key should match "inbound_ids"
                if key == "inbound_ids" {
                     if let Ok(id) = value.parse::<i64>() {
                         inbound_ids.push(id);
                     }
                }
            }
        }
    }

    info!("Updating bindings for plan {}: {:?}", plan_id, inbound_ids);

    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to start transaction: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database Transaction Error").into_response();
        }
    };

    // 1. Clear existing
    if let Err(e) = sqlx::query("DELETE FROM plan_inbounds WHERE plan_id = ?")
        .bind(plan_id)
        .execute(&mut *tx)
        .await 
    {
         error!("Failed to delete existing bindings for plan {}: {}", plan_id, e);
         return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to clear existing bindings").into_response();
    }

    // 2. Insert new
    for inbound_id in inbound_ids {
        if let Err(e) = sqlx::query("INSERT INTO plan_inbounds (plan_id, inbound_id) VALUES (?, ?)")
            .bind(plan_id)
            .bind(inbound_id)
            .execute(&mut *tx)
            .await 
        {
            error!("Failed to insert binding plan={} inbound={}: {}", plan_id, inbound_id, e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to insert new bindings").into_response();
        }
    }

    if let Err(e) = tx.commit().await {
         error!("Failed to commit bindings transaction: {}", e);
         return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Transaction Commit Failed").into_response();
    }

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    axum::response::Redirect::to(&format!("{}/plans", admin_path)).into_response()
}

// --- Dynamic Config Preview ---

pub async fn preview_node_config(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> impl IntoResponse {
    info!("Generating config preview for node {}", node_id);

    // This is essentially the logic from OrchestrationService minus the SSH push
    // We can't easily call sync_node_config because it's hardcoded to push via SSH.
    // Let's implement a 'dry run' or copy the logic. 
    // Optimization: Refactor OrchestrationService better later.
    
    // 1. Fetch node details
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?").bind(node_id).fetch_one(&state.pool).await {
        Ok(n) => n,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
    };

    // 2. Fetch Inbounds
    let mut inbounds: Vec<Inbound> = sqlx::query_as("SELECT * FROM inbounds WHERE node_id = ?")
        .bind(node_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    // 3. Simple user injection (VLESS/UUID only for preview)
    for inbound in &mut inbounds {
        let linked_plans: Vec<i64> = sqlx::query_scalar("SELECT plan_id FROM plan_inbounds WHERE inbound_id = ?")
            .bind(inbound.id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

        if !linked_plans.is_empty() {
             let plan_ids_str = linked_plans.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
             let query = format!("SELECT * FROM subscriptions WHERE status = 'active' AND plan_id IN ({}) LIMIT 5", plan_ids_str);
             let active_subs: Vec<crate::models::store::Subscription> = sqlx::query_as(&query).fetch_all(&state.pool).await.unwrap_or_default();

             use crate::models::network::{InboundType, VlessClient};
             if let Ok(mut settings) = serde_json::from_str::<InboundType>(&inbound.settings) {
                 match &mut settings {
                     InboundType::Vless(vless) => {
                         for sub in &active_subs {
                             if let Some(uuid) = &sub.vless_uuid {
                                 vless.clients.push(VlessClient { id: uuid.clone(), email: format!("user_{}", sub.user_id), flow: "xtls-rprx-vision".to_string() });
                             }
                         }
                     },
                     _ => {}
                 }
                 inbound.settings = serde_json::to_string(&settings).unwrap_or(inbound.settings.clone());
             }
        }
    }

    // 4. Generate Config
    let config = crate::singbox::ConfigGenerator::generate_config(&node, inbounds);
    let json = serde_json::to_string_pretty(&config).unwrap_or_default();

    (axum::http::StatusCode::OK, json).into_response()
}

// --- Inbound Editing ---

#[derive(Template)]
#[template(path = "inbound_edit_modal.html")]
pub struct InboundEditModalTemplate {
    pub node_id: i64,
    pub inbound: Inbound,
    pub admin_path: String,
}

pub async fn get_edit_inbound(
    State(state): State<AppState>,
    Path((node_id, inbound_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let inbound = match sqlx::query_as::<_, Inbound>("SELECT * FROM inbounds WHERE id = ? AND node_id = ?")
        .bind(inbound_id).bind(node_id).fetch_one(&state.pool).await {
            Ok(i) => i,
            Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Inbound not found").into_response(),
        };

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = InboundEditModalTemplate { node_id, inbound, admin_path };
    Html(template.render().unwrap_or_default()).into_response()
}

pub async fn update_inbound(
    State(state): State<AppState>,
    Path((node_id, inbound_id)): Path<(i64, i64)>,
    Form(form): Form<AddInboundForm>,
) -> impl IntoResponse {
    info!("Updating inbound {} on node {}", inbound_id, node_id);
    
    // Validate JSON against Models
    // 1. Stream Settings
    if let Err(e) = serde_json::from_str::<crate::models::network::StreamSettings>(&form.stream_settings) {
         return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid Stream Settings: {}", e)).into_response();
    }

    // 2. Protocol Settings
    match form.protocol.as_str() {
        "vless" => {
            if let Err(e) = serde_json::from_str::<crate::models::network::VlessSettings>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid VLESS Settings: {}", e)).into_response();
            }
        },
        "hysteria2" => {
            if let Err(e) = serde_json::from_str::<crate::models::network::Hysteria2Settings>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid Hysteria2 Settings: {}", e)).into_response();
            }
        },
        "trojan" => {
            if let Err(e) = serde_json::from_str::<crate::models::network::TrojanSettings>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid Trojan Settings: {}", e)).into_response();
            }
        },
        _ => {
            // Unknown protocol, just check valid JSON
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&form.settings) {
                return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
            }
        }
    }

    // Check if port is already in use (excluding self)
    let port_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbounds WHERE node_id = ? AND listen_port = ? AND id != ?")
        .bind(node_id)
        .bind(form.listen_port)
        .bind(inbound_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    if port_count > 0 {
         return (axum::http::StatusCode::BAD_REQUEST, format!("Port {} is already used by another inbound on this node.", form.listen_port)).into_response();
    }

    let res = sqlx::query("UPDATE inbounds SET tag = ?, protocol = ?, listen_port = ?, listen_ip = ?, settings = ?, stream_settings = ? WHERE id = ? AND node_id = ?")
        .bind(&form.tag).bind(&form.protocol).bind(form.listen_port).bind(&form.listen_ip)
        .bind(&form.settings).bind(&form.stream_settings).bind(inbound_id).bind(node_id)
        .execute(&state.pool).await;

    match res {
        Ok(_) => {
            // PubSub Notify
            let _ = state.pubsub.publish(&format!("node_events:{}", node_id), "update").await;

            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
            ([("HX-Redirect", format!("{}/nodes/{}/inbounds", admin_path, node_id))], "Updated").into_response()
        },
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    }
}
