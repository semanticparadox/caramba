// Nodes Module
// Node management, installation, configuration

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use tracing::{error, info, warn};

use std::collections::HashMap;

use super::auth::get_auth_user;
use crate::AppState;
use caramba_db::models::node::Node;
use chrono::Utc;
use uuid::Uuid;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template)]
#[template(path = "nodes_scope.html")]
pub struct NodesTemplate {
    pub nodes: Vec<Node>,
    pub relay_nodes: Vec<Node>,
    pub rows_html: String,
    pub scope: String,
    pub scope_title: String,
    pub total_nodes_count: usize,
    pub exit_nodes_count: usize,
    pub relay_nodes_count: usize,
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
    // Метрики: активные подписки и онлайн-устройства по узлам
    pub subs_map: HashMap<i64, i64>,
    pub online_map: HashMap<i64, i64>,
}

/// Запрашивает счётчики активных подписок и онлайн-устройств (активность за 15 минут)
/// для всех узлов. Возвращает (subs_map, online_map) node_id -> count.
async fn fetch_node_metrics(
    pool: &sqlx::PgPool,
) -> (HashMap<i64, i64>, HashMap<i64, i64>) {
    // Активные подписки на каждом узле
    let subs_rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT node_id, COUNT(*)::bigint AS count
         FROM subscriptions
         WHERE status = 'active' AND node_id IS NOT NULL
         GROUP BY node_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let subs_map: HashMap<i64, i64> = subs_rows.into_iter().collect();

    // Уникальные устройства, активные за последние 15 минут
    let online_rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT last_node_id AS node_id, COUNT(DISTINCT subscription_id)::bigint AS count
         FROM subscription_device_leases
         WHERE last_seen_at > NOW() - INTERVAL '15 minutes'
           AND last_node_id IS NOT NULL
         GROUP BY last_node_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let online_map: HashMap<i64, i64> = online_rows.into_iter().collect();

    (subs_map, online_map)
}

/// Общие данные для рендера страницы / HTMX-строк узлов.
/// Загружается один раз, используется и полной страницей, и partial-рендером.
struct NodeListData {
    nodes_for_scope: Vec<Node>,
    relay_nodes: Vec<Node>,
    exit_nodes: Vec<Node>,
    all_nodes_count: usize,
    admin_path: String,
    agent_latest_version: String,
    auto_update_agents: bool,
    subs_map: HashMap<i64, i64>,
    online_map: HashMap<i64, i64>,
}

async fn fetch_node_list_data(state: &AppState, scope: NodesScope) -> NodeListData {
    let all_nodes = state
        .infrastructure_service
        .get_all_nodes()
        .await
        .unwrap_or_else(|e| {
            error!("Failed to load nodes list: {}", e);
            Vec::new()
        });

    let relay_nodes: Vec<Node> = all_nodes.iter().filter(|n| n.is_relay_node()).cloned().collect();
    let exit_nodes: Vec<Node> = all_nodes.iter().filter(|n| n.is_exit_node()).cloned().collect();
    let nodes_for_scope: Vec<Node> = all_nodes.iter().filter(|n| scope.matches_node(n)).cloned().collect();
    let all_nodes_count = all_nodes.len();

    let admin_path = normalized_admin_path(&state.admin_path);
    let agent_latest_version = state.settings.get_or_default("agent_latest_version", "0.0.0").await;
    let auto_update_agents: bool = state
        .settings
        .get_or_default("auto_update_agents", "true")
        .await
        .parse()
        .unwrap_or(true);

    let (subs_map, online_map) = fetch_node_metrics(&state.pool).await;

    NodeListData {
        nodes_for_scope,
        relay_nodes,
        exit_nodes,
        all_nodes_count,
        admin_path,
        agent_latest_version,
        auto_update_agents,
        subs_map,
        online_map,
    }
}

fn render_nodes_rows_html(
    nodes: Vec<Node>,
    admin_path: &str,
    agent_latest_version: &str,
    auto_update_agents: bool,
    subs_map: HashMap<i64, i64>,
    online_map: HashMap<i64, i64>,
) -> String {
    let partial = NodesRowsPartial {
        nodes,
        admin_path: admin_path.to_string(),
        agent_latest_version: agent_latest_version.to_string(),
        auto_update_agents,
        subs_map,
        online_map,
    };

    partial.render().unwrap_or_else(|e| {
        format!(
            r#"<tr><td colspan="6" class="px-6 py-8 text-center text-rose-400 text-xs uppercase tracking-widest">Failed to render rows: {}</td></tr>"#,
            e
        )
    })
}

#[derive(Clone, Copy)]
enum NodesScope {
    Exit,
    Relay,
}

impl NodesScope {
    fn as_str(self) -> &'static str {
        match self {
            NodesScope::Exit => "exit",
            NodesScope::Relay => "relay",
        }
    }

    fn title(self) -> &'static str {
        match self {
            NodesScope::Exit => "Exit Nodes",
            NodesScope::Relay => "Relay Nodes",
        }
    }

    fn active_page(self) -> &'static str {
        match self {
            NodesScope::Exit => "nodes_exit",
            NodesScope::Relay => "nodes_relay",
        }
    }

    fn matches_node(self, node: &Node) -> bool {
        match self {
            NodesScope::Exit => node.is_exit_node(),
            NodesScope::Relay => node.is_relay_node(),
        }
    }
}

fn normalized_admin_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "/admin".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "node_edit_modal.html")]
pub struct NodeEditModalTemplate {
    pub node: Node,
    pub relay_nodes: Vec<Node>, // Relay-only selection for exit nodes
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
    pub nginx_proxy_config: String,
    pub caddy_proxy_config: String,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct NodeSniDisplay {
    pub id: i64,
    pub domain: String,
    pub is_pinned: bool,
    pub health_score: i32,
    pub latency_ms: Option<i32>,
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
    pub country: Option<String>,
    #[serde(default)]
    pub node_type: Option<String>,
    #[serde(default)]
    pub is_relay: Option<String>,
    #[serde(default)]
    pub relay_id: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct NodeLogsQuery {
    pub lines: Option<usize>,
    pub page: Option<usize>,
    pub force: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateNodeForm {
    pub name: String,
    pub ip: String,
    #[serde(default)]
    pub relay_id: Option<String>,
    #[serde(default)]
    pub node_type: Option<String>,
    pub is_relay: Option<String>, // "on" or None from checkbox
    pub config_block_torrent: Option<String>,
    pub config_block_ads: Option<String>,
    pub config_block_porn: Option<String>,
    pub config_qos_enabled: Option<String>,
    pub country: Option<String>,
}

fn normalize_node_type(node_type: Option<&str>, legacy_is_relay: bool) -> &'static str {
    if legacy_is_relay {
        return "relay";
    }

    match node_type
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("relay") => "relay",
        _ => "exit",
    }
}

async fn ensure_node_join_token(pool: &sqlx::PgPool, node_id: i64) -> anyhow::Result<String> {
    let existing: Option<Option<String>> =
        sqlx::query_scalar("SELECT join_token FROM nodes WHERE id = $1")
            .bind(node_id)
            .fetch_optional(pool)
            .await?;

    let current = match existing {
        Some(v) => v.unwrap_or_default(),
        None => return Err(anyhow::anyhow!("Node not found")),
    };

    if !current.trim().is_empty() {
        return Ok(current);
    }

    let token = Uuid::new_v4().to_string();
    let rows = sqlx::query("UPDATE nodes SET join_token = $1 WHERE id = $2")
        .bind(&token)
        .bind(node_id)
        .execute(pool)
        .await?
        .rows_affected();

    if rows == 0 {
        return Err(anyhow::anyhow!("Node not found"));
    }

    Ok(token)
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_nodes(
    State(state): State<AppState>,
    _headers: HeaderMap,
    _jar: CookieJar,
) -> impl IntoResponse {
    let admin_path = normalized_admin_path(&state.admin_path);
    Redirect::to(&format!("{}/nodes/exit", admin_path.trim_end_matches('/')))
}

async fn render_nodes_scope_page(
    state: &AppState,
    headers: &HeaderMap,
    jar: &CookieJar,
    scope: NodesScope,
) -> Response {
    let data = fetch_node_list_data(state, scope).await;

    // HTMX partial refresh — вернуть только строки таблицы
    if headers.contains_key("HX-Request") || headers.contains_key("hx-request") {
        let template = NodesRowsPartial {
            nodes: data.nodes_for_scope,
            admin_path: data.admin_path,
            agent_latest_version: data.agent_latest_version,
            auto_update_agents: data.auto_update_agents,
            subs_map: data.subs_map,
            online_map: data.online_map,
        };
        return Html(template.render().unwrap_or_default()).into_response();
    }

    let rows_html = render_nodes_rows_html(
        data.nodes_for_scope.clone(),
        &data.admin_path,
        &data.agent_latest_version,
        data.auto_update_agents,
        data.subs_map.clone(),
        data.online_map.clone(),
    );

    let template = NodesTemplate {
        nodes: data.nodes_for_scope,
        relay_nodes: data.relay_nodes.clone(),
        rows_html,
        scope: scope.as_str().to_string(),
        scope_title: scope.title().to_string(),
        total_nodes_count: data.all_nodes_count,
        exit_nodes_count: data.exit_nodes.len(),
        relay_nodes_count: data.relay_nodes.len(),
        is_auth: true,
        username: get_auth_user(state, jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path: data.admin_path,
        active_page: scope.active_page().to_string(),
        agent_latest_version: data.agent_latest_version,
        auto_update_agents: data.auto_update_agents,
    };
    Html(template.render().unwrap_or_default()).into_response()
}

pub async fn get_exit_nodes_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> impl IntoResponse {
    render_nodes_scope_page(&state, &headers, &jar, NodesScope::Exit).await
}

pub async fn get_relay_nodes_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> impl IntoResponse {
    render_nodes_scope_page(&state, &headers, &jar, NodesScope::Relay).await
}

/// Возвращает HTML-строки таблицы узлов для HTMX-запросов (refresh по scope).
/// Переиспользует fetch_node_list_data для единообразия с полной страницей.
async fn render_nodes_rows_for_scope(state: &AppState, scope: NodesScope) -> Response {
    let data = fetch_node_list_data(state, scope).await;
    let template = NodesRowsPartial {
        nodes: data.nodes_for_scope,
        admin_path: data.admin_path,
        agent_latest_version: data.agent_latest_version,
        auto_update_agents: data.auto_update_agents,
        subs_map: data.subs_map,
        online_map: data.online_map,
    };
    Html(template.render().unwrap_or_default()).into_response()
}

pub async fn get_exit_nodes_rows(State(state): State<AppState>) -> impl IntoResponse {
    render_nodes_rows_for_scope(&state, NodesScope::Exit).await
}

pub async fn get_relay_nodes_rows(State(state): State<AppState>) -> impl IntoResponse {
    render_nodes_rows_for_scope(&state, NodesScope::Relay).await
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

    match state
        .infrastructure_service
        .create_node(
            &form.name,
            &check_ip,
            vpn_port,
            form.auto_configure.unwrap_or(false),
        )
        .await
    {
        Ok(id) => {
            // Set country and relay fields if provided
            let country = form.country.as_deref().unwrap_or("").trim();
            let node_type = normalize_node_type(form.node_type.as_deref(), form.is_relay.is_some());
            let is_relay = node_type == "relay";
            let relay_id: Option<i64> = if is_relay {
                None
            } else {
                form.relay_id.as_deref().and_then(|s| s.trim().parse().ok())
            };
            if !country.is_empty() || is_relay || relay_id.is_some() || form.node_type.is_some() {
                let country_opt = if country.is_empty() {
                    None
                } else {
                    Some(country)
                };
                let primary = sqlx::query(
                    "UPDATE nodes SET country = $1, node_type = $2, is_relay = $3, relay_id = $4 WHERE id = $5",
                )
                .bind(country_opt)
                .bind(node_type)
                .bind(is_relay)
                .bind(relay_id)
                .bind(id)
                .execute(&state.pool)
                .await;

                if let Err(e) = primary {
                    let msg = e.to_string();
                    if msg.contains("node_type") && msg.contains("does not exist") {
                        let _ = sqlx::query(
                            "UPDATE nodes SET country = $1, is_relay = $2, relay_id = $3 WHERE id = $4",
                        )
                        .bind(country_opt)
                        .bind(is_relay)
                        .bind(relay_id)
                        .bind(id)
                        .execute(&state.pool)
                        .await;
                    } else {
                        error!("Failed to persist node role data for node {}: {}", id, e);
                    }
                }
            }

            // Trigger default inbounds via orchestration
            if let Err(e) = state.orchestration_service.init_default_inbounds(id).await {
                error!("Failed to initialize inbounds for new node {}: {}", id, e);
            }

            // Always return installation modal with command+token, same as legacy flow.
            let join_token = match ensure_node_join_token(&state.pool, id).await {
                Ok(token) => token,
                Err(e) => {
                    error!("Failed to resolve node token for {}: {}", id, e);
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to generate node token",
                    )
                        .into_response();
                }
            };

            let admin_path = state.admin_path.clone();
            let template = NodeManualInstallTemplate {
                node_id: id,
                node_name: form.name.clone(),
                join_token,
                admin_path,
            };

            let mut headers = HeaderMap::new();
            headers.insert("HX-Trigger", "refresh_nodes".parse().unwrap());

            let mut html = template.render().unwrap_or_default();
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
    let admin_path = if admin_path.starts_with('/') {
        admin_path
    } else {
        format!("/{}", admin_path)
    };

    let all_nodes = state
        .infrastructure_service
        .get_active_relay_nodes()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|relay| relay.id != node.id)
        .collect();

    let template = NodeEditModalTemplate {
        node,
        relay_nodes: all_nodes,
        admin_path,
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
    }
}

pub async fn update_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateNodeForm>,
) -> impl IntoResponse {
    let node_type = normalize_node_type(form.node_type.as_deref(), form.is_relay.is_some());
    let is_relay = node_type == "relay";
    let country = form.country.as_deref().unwrap_or("").trim().to_string();
    info!(
        "Updating node ID: {} (Type: {}, Country: {})",
        id, node_type, country
    );

    let relay_id: Option<i64> = if is_relay {
        None
    } else {
        form.relay_id.as_deref().and_then(|s| s.trim().parse().ok())
    };

    // 1. Update core fields
    if let Err(e) = state
        .infrastructure_service
        .update_node(id, &form.name, &form.ip, relay_id, node_type, &country)
        .await
    {
        error!("Failed to update node core: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update node",
        )
            .into_response();
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
    let _ = state
        .pubsub
        .publish(&format!("node_events:{}", id), "update")
        .await;

    let admin_path = state.admin_path.clone();
    let mut headers = HeaderMap::new();

    // Check if HTMX request
    if let Some(_) = form
        .config_block_torrent
        .as_ref()
        .or(form.config_block_ads.as_ref())
        .or(form.config_block_porn.as_ref())
        .or(form.config_qos_enabled.as_ref())
    {
        // Targeted update from managing page switches
        return (axum::http::StatusCode::OK, "Saved").into_response();
    }

    if let Ok(val) = format!("{}/nodes", admin_path).parse() {
        headers.insert("HX-Redirect", val);
    }
    (axum::http::StatusCode::OK, headers, "Updated").into_response()
}

pub async fn sync_node(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    info!("Manual sync triggered for node: {}", id);

    // Update trigger tracking
    let _ = sqlx::query("UPDATE nodes SET last_sync_trigger = 'Manual Update' WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

    let orch = state.orchestration_service.clone();
    let pubsub = state.pubsub.clone();

    tokio::spawn(async move {
        // Sync should be non-destructive: keep chosen/manual inbounds and only regenerate effective config.
        if let Err(e) = orch.generate_node_config_json(id).await {
            error!(
                "Failed to generate config for node {} during manual sync: {}",
                id, e
            );
            return;
        }

        if let Err(e) = pubsub
            .publish(&format!("node_events:{}", id), "update")
            .await
        {
            error!("Failed to publish update event: {}", e);
        } else {
            info!("Successfully requested config pull for node {}", id);
        }
    });

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("HX-Refresh", "true".parse().unwrap());
    (axum::http::StatusCode::ACCEPTED, headers, "").into_response()
}

pub async fn test_node_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node = match state.infrastructure_service.get_node_by_id(id).await {
        Ok(n) => n,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
    };

    let is_online = node
        .last_seen
        .map(|ls| (Utc::now() - ls).num_seconds() < 60)
        .unwrap_or(false);

    if is_online {
        (
            axum::http::StatusCode::OK,
            format!(
                "✅ Node '{}' is ONLINE (Last seen: {}s ago)",
                node.name,
                (Utc::now() - node.last_seen.unwrap()).num_seconds()
            ),
        )
            .into_response()
    } else {
        (
            axum::http::StatusCode::OK,
            format!("❌ Node '{}' is OFFLINE or UNREACHABLE", node.name),
        )
            .into_response()
    }
}

pub async fn delete_node(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    info!("Request to delete node ID: {}", id);

    // Collect affected users before deletion clears node_id from subscriptions
    let affected_tg_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT u.tg_id FROM subscriptions s JOIN users u ON s.user_id = u.id WHERE s.node_id = $1 AND s.status = 'active' AND u.tg_id > 0"
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    match state.infrastructure_service.delete_node(id).await {
        Ok(_) => {
            info!("Node {} deleted successfully", id);

            // Notify affected users about server removal
            for tg_id in affected_tg_ids {
                let msg = "⚠️ Сервер был удалён\\. Выберите новый сервер в Mini App → Серверы\\.";
                let _ = state.bot_manager.send_notification(tg_id, msg).await;
            }

            (
                axum::http::StatusCode::OK,
                [("HX-Trigger", "refresh_nodes")],
                "",
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to delete node {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                [("HX-Reswap", "none")],
                format!("Failed to delete node: {}", e),
            )
                .into_response()
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
                "Toggled",
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to toggle node {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to toggle node",
            )
                .into_response()
        }
    }
}

/// Admin override: принудительная активация ноды через панель.
/// В нормальном flow это происходит автоматически при первом heartbeat (Task 3).
/// Используется только при ручном вмешательстве администратора.
pub async fn activate_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state
        .infrastructure_service
        .activate_node(id, &state.security_service)
        .await
    {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            (
                axum::http::StatusCode::OK,
                [("HX-Redirect", format!("{}/nodes", admin_path))],
                "Activated",
            )
                .into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to activate: {}", e),
        )
            .into_response(),
    }
}

pub async fn get_node_install_script(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let node_type = state
        .infrastructure_service
        .get_node_by_id(id)
        .await
        .map(|node| node.normalized_node_type().to_string())
        .unwrap_or_else(|_| "exit".to_string());

    let join_token = match ensure_node_join_token(&state.pool, id).await {
        Ok(token) => token,
        Err(e) => {
            error!("Failed to load/generate join token for node {}: {}", id, e);
            if e.to_string().contains("Node not found") {
                return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response();
            }
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load node token",
            )
                .into_response();
        }
    };

    let panel_url = {
        let configured = state.settings.get_or_default("panel_url", "").await;
        let from_settings = configured.trim().trim_end_matches('/').to_string();
        if !from_settings.is_empty() {
            Some(from_settings)
        } else {
            None
        }
    }
    .or_else(|| {
        std::env::var("PANEL_URL").ok().and_then(|v| {
            let trimmed = v.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
    })
    .or_else(|| {
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
    })
    .unwrap_or_else(|| "https://YOUR_PANEL_DOMAIN".to_string());

    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
curl -fsSL '{panel_url}/install.sh' | sudo bash -s -- --role node --panel '{panel_url}' --token '{token}' --node-type '{node_type}'
"#,
        panel_url = panel_url,
        token = join_token,
        node_type = node_type,
    );

    (
        [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
        script,
    )
        .into_response()
}

pub async fn get_install_sh() -> impl IntoResponse {
    match crate::scripts::Scripts::get_universal_install_script() {
        Some(content) => (
            [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
            content,
        )
            .into_response(),
        None => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Script not found",
        )
            .into_response(),
    }
}

pub async fn get_node_raw_install_script(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let node_type = state
        .infrastructure_service
        .get_node_by_id(id)
        .await
        .map(|node| node.normalized_node_type().to_string())
        .unwrap_or_else(|_| "exit".to_string());

    let join_token = match ensure_node_join_token(&state.pool, id).await {
        Ok(token) => token,
        Err(e) => {
            error!("Failed to load/generate join token for node {}: {}", id, e);
            if e.to_string().contains("Node not found") {
                return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response();
            }
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load node token",
            )
                .into_response();
        }
    };

    let panel_url = {
        let configured = state.settings.get_or_default("panel_url", "").await;
        let from_settings = configured.trim().trim_end_matches('/').to_string();
        if !from_settings.is_empty() {
            Some(from_settings)
        } else {
            None
        }
    }
    .or_else(|| {
        std::env::var("PANEL_URL").ok().and_then(|v| {
            let trimmed = v.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
    })
    .or_else(|| {
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
    })
    .unwrap_or_else(|| "https://YOUR_PANEL_DOMAIN".to_string());

    let command = format!(
        "curl -fsSL '{panel_url}/install.sh' | sudo bash -s -- --role node --panel '{panel_url}' --token '{token}' --node-type '{node_type}'",
        panel_url = panel_url,
        token = join_token,
        node_type = node_type,
    );

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        command,
    )
        .into_response()
}

pub async fn get_node_logs(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Query(query): Query<NodeLogsQuery>,
) -> impl IntoResponse {
    let lines = query.lines.unwrap_or(120).clamp(20, 1000);
    let page = query.page.unwrap_or(0);
    let base_url = format!("{}/nodes/{}/logs", state.admin_path, id);
    let cache_key = format!("node_logs:{}", id);
    let force_refresh = query.force.unwrap_or(false);

    if force_refresh {
        let _ = state.redis.del(&cache_key).await;
        let _ = sqlx::query("UPDATE nodes SET pending_log_collection = TRUE WHERE id = $1")
            .bind(id)
            .execute(&state.pool)
            .await;

        return Html(format!(
            r###"
        <div class="p-8 text-center" hx-get="{base_url}?lines={lines}&page=0" hx-trigger="every 3s" hx-target="this">
            <div class="animate-spin w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full mx-auto mb-4"></div>
            <p class="text-slate-400">Refreshing logs from agent...</p>
            <p class="text-[10px] text-slate-500 mt-2 italic">Please wait a few seconds.</p>
        </div>
    "###,
            base_url = base_url,
            lines = lines
        ))
        .into_response();
    }

    // 1. Check if logs are already in Redis
    if let Ok(Some(logs_json)) = state.redis.get(&cache_key).await {
        return Html(format!(r###"
            <div class="space-y-4">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <div class="inline-flex items-center gap-2 text-xs text-slate-400">
                        <span>Lines:</span>
                        <button hx-get="{base_url}?lines=60&page=0" hx-target="#logs-modal-content" class="px-2 py-1 rounded border border-white/10 hover:bg-white/5">60</button>
                        <button hx-get="{base_url}?lines=120&page=0" hx-target="#logs-modal-content" class="px-2 py-1 rounded border border-white/10 hover:bg-white/5">120</button>
                        <button hx-get="{base_url}?lines=300&page=0" hx-target="#logs-modal-content" class="px-2 py-1 rounded border border-white/10 hover:bg-white/5">300</button>
                    </div>
                    <button hx-get="{base_url}?lines={lines}&page={page}&force=true" hx-target="#logs-modal-content" class="text-xs text-indigo-400 hover:text-indigo-300">Refresh Logs</button>
                </div>
                <div class="custom-scrollbar overflow-auto max-h-[60vh] space-y-4 font-mono text-[11px]">
                    {}
                </div>
            </div>
        "###, format_logs_html(&logs_json, &base_url, lines, page))).into_response();
    }

    // 2. If not, trigger collection and return "Waiting"
    // Optimization: Only set pending flag if it's not already set to prevent spamming the agent
    let pending: bool =
        sqlx::query_scalar("SELECT pending_log_collection FROM nodes WHERE id = $1")
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
        <div class="p-8 text-center" hx-get="{}?lines={}&page=0" hx-trigger="every 3s" hx-target="this">
            <div class="animate-spin w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full mx-auto mb-4"></div>
            <p class="text-slate-400">Requesting real-time logs from agent...</p>
            <p class="text-[10px] text-slate-500 mt-2 italic">Ensure the node is online. This may take up to 30 seconds.</p>
        </div>
    "###, base_url, lines)).into_response()
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn format_logs_html(json_str: &str, base_url: &str, lines: usize, page: usize) -> String {
    let logs: std::collections::HashMap<String, String> =
        serde_json::from_str(json_str).unwrap_or_default();
    let mut html = String::new();
    let mut entries: Vec<(String, String)> = logs.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (service, content) in entries {
        let all_lines: Vec<&str> = content.lines().collect();
        let total = all_lines.len();
        let page_size = lines.max(1);
        let offset = page.saturating_mul(page_size);
        let end = total.saturating_sub(offset);
        let start = end.saturating_sub(page_size);
        let slice = if start < end {
            &all_lines[start..end]
        } else {
            &[]
        };
        let visible = escape_html(&slice.join("\n"));
        let prev_page = page.saturating_sub(1);
        let next_page = page.saturating_add(1);
        let can_next = start > 0;
        let range_label = if total == 0 {
            "no lines".to_string()
        } else {
            format!("lines {}-{} of {}", start + 1, end, total)
        };

        html.push_str(&format!(r###"
            <div class="bg-slate-950/50 rounded-lg border border-white/5 overflow-hidden">
                <div class="bg-white/5 px-3 py-1.5 flex justify-between items-center text-[10px] uppercase font-bold text-slate-400">
                    <span>{}</span>
                    <div class="flex items-center gap-2 normal-case text-[10px] text-slate-500 font-medium">
                        <span>{}</span>
                        <button hx-get="{}?lines={}&page={}" hx-target="#logs-modal-content" class="px-1.5 py-0.5 rounded border border-white/10 hover:bg-white/5 {}">Prev</button>
                        <button hx-get="{}?lines={}&page={}" hx-target="#logs-modal-content" class="px-1.5 py-0.5 rounded border border-white/10 hover:bg-white/5 {}">Next</button>
                    </div>
                </div>
                <pre class="p-3 text-emerald-400 overflow-x-auto">{}</pre>
            </div>
        "###, service, range_label, base_url, lines, prev_page, if page == 0 { "opacity-40 pointer-events-none" } else { "" }, base_url, lines, next_page, if can_next { "" } else { "opacity-40 pointer-events-none" }, visible));
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

    let all_nodes = state
        .infrastructure_service
        .get_active_relay_nodes()
        .await
        .unwrap_or_default();
    let inbounds = state
        .infrastructure_service
        .get_node_inbounds(id)
        .await
        .unwrap_or_default();

    // Fetch discovered SNIs with pinning status, with compatibility fallback for legacy schemas.
    let discovered_snis = {
        use sqlx::Row;
        let primary = sqlx::query(
            r#"
            SELECT 
                s.id,
                s.domain,
                COALESCE(s.health_score, 100) AS health_score,
                s.latency_ms,
                COALESCE(s.is_premium, FALSE) AS is_premium,
                EXISTS(SELECT 1 FROM node_pinned_snis p WHERE p.node_id = $1 AND p.sni_id = s.id) AS is_pinned
            FROM sni_pool s
            WHERE (s.discovered_by_node_id = $1 OR COALESCE(s.is_premium, FALSE) = TRUE)
              AND (COALESCE(s.health_score, 100) >= 50 OR EXISTS(SELECT 1 FROM node_pinned_snis p WHERE p.node_id = $1 AND p.sni_id = s.id))
            ORDER BY is_pinned DESC, is_premium DESC, health_score DESC
            LIMIT 200
            "#,
        )
        .bind(id)
        .fetch_all(&state.pool)
        .await;

        match primary {
            Ok(rows) => rows
                .into_iter()
                .map(|row| NodeSniDisplay {
                    id: row.try_get::<i64, _>("id").unwrap_or_default(),
                    domain: row.try_get::<String, _>("domain").unwrap_or_default(),
                    is_pinned: row.try_get::<bool, _>("is_pinned").unwrap_or(false),
                    health_score: row.try_get::<i32, _>("health_score").unwrap_or(100),
                    latency_ms: row.try_get::<Option<i32>, _>("latency_ms").unwrap_or(None),
                    is_premium: row.try_get::<bool, _>("is_premium").unwrap_or(false),
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                error!(
                    "Primary discovered_snis query failed for node {}: {}. Using compatibility fallback.",
                    id, e
                );
                let fallback = sqlx::query(
                    "SELECT id, domain, COALESCE(health_score, 100) AS health_score FROM sni_pool ORDER BY health_score DESC LIMIT 200",
                )
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();
                fallback
                    .into_iter()
                    .map(|row| NodeSniDisplay {
                        id: row.try_get::<i64, _>("id").unwrap_or_default(),
                        domain: row.try_get::<String, _>("domain").unwrap_or_default(),
                        is_pinned: false,
                        health_score: row.try_get::<i32, _>("health_score").unwrap_or(100),
                        latency_ms: None,
                        is_premium: false,
                    })
                    .collect::<Vec<_>>()
            }
        }
    };

    let admin_path = state.admin_path.clone();
    let username = get_auth_user(&state, &jar)
        .await
        .unwrap_or("Admin".to_string());

    // Generate nginx reverse proxy config for relay nodes
    let nginx_proxy_config = if node.is_relay_node() {
        let panel_url = state.settings.get_or_default("panel_url", "").await;
        let subscription_domain = state
            .settings
            .get_or_default("subscription_domain", "")
            .await;
        let tma_domain = state.settings.get_or_default("tma_domain", "").await;
        let subscription_entry_relay_id = state
            .settings
            .get_or_default("subscription_entry_relay_id", "0")
            .await
            .parse::<i64>()
            .unwrap_or(0);
        let tma_entry_relay_id = state
            .settings
            .get_or_default("tma_entry_relay_id", "0")
            .await
            .parse::<i64>()
            .unwrap_or(0);
        let upstream = if !panel_url.is_empty() {
            panel_url.trim_end_matches('/').to_string()
        } else {
            std::env::var("PANEL_URL").unwrap_or_else(|_| "https://YOUR_HUB_IP".to_string())
        };
        let upstream_host = upstream
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("YOUR_HUB_IP");
        let relay_domain = format!("{}.example.com", node.name.to_lowercase().replace(' ', "-"));
        let subscription_host =
            if subscription_entry_relay_id == node.id && !subscription_domain.trim().is_empty() {
                subscription_domain.trim().to_string()
            } else {
                relay_domain.clone()
            };
        let tma_host = if tma_entry_relay_id == node.id && !tma_domain.trim().is_empty() {
            tma_domain.trim().to_string()
        } else {
            subscription_host.clone()
        };

        format!(
            r#"# Nginx Reverse Proxy for Relay: {node_name}
# Install: sudo apt install nginx certbot python3-certbot-nginx
# Then: sudo certbot --nginx -d YOUR_RELAY_DOMAIN

upstream caramba_hub {{
    server {upstream_host}:443;
}}

server {{
    listen 80;
    server_name {subscription_host} {tma_host};
    return 301 https://$host$request_uri;
}}

server {{
    listen 443 ssl http2;
    server_name {subscription_host} {tma_host};

    # SSL — replace with your actual certificate paths
    ssl_certificate     /etc/letsencrypt/live/{subscription_host}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{subscription_host}/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    # Subscription endpoint
    location /sub/ {{
        proxy_pass https://caramba_hub;
        proxy_ssl_server_name on;
        proxy_ssl_name {upstream_host};
        proxy_set_header Host {upstream_host};
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_ssl_verify off;
    }}

    # Mini-app / WebApp
    location /app/ {{
        proxy_pass https://caramba_hub;
        proxy_ssl_server_name on;
        proxy_ssl_name {upstream_host};
        proxy_set_header Host {upstream_host};
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_ssl_verify off;

        # WebSocket support
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }}

    # Telegram Bot Webhook (if needed)
    location /bot/ {{
        proxy_pass https://caramba_hub;
        proxy_ssl_server_name on;
        proxy_ssl_name {upstream_host};
        proxy_set_header Host {upstream_host};
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_ssl_verify off;
    }}

    # Block everything else
    location / {{
        return 444;
    }}
}}"#,
            node_name = node.name,
            upstream_host = upstream_host,
            subscription_host = subscription_host,
            tma_host = tma_host,
        )
    } else {
        String::new()
    };

    let caddy_proxy_config = if node.is_relay_node() && !nginx_proxy_config.is_empty() {
        let subscription_domain = state
            .settings
            .get_or_default("subscription_domain", "")
            .await;
        let tma_domain = state.settings.get_or_default("tma_domain", "").await;
        let subscription_entry_relay_id = state
            .settings
            .get_or_default("subscription_entry_relay_id", "0")
            .await
            .parse::<i64>()
            .unwrap_or(0);
        let tma_entry_relay_id = state
            .settings
            .get_or_default("tma_entry_relay_id", "0")
            .await
            .parse::<i64>()
            .unwrap_or(0);
        let subscription_host =
            if subscription_entry_relay_id == node.id && !subscription_domain.trim().is_empty() {
                subscription_domain.trim().to_string()
            } else {
                format!("{}.example.com", node.name.to_lowercase().replace(' ', "-"))
            };
        let tma_host = if tma_entry_relay_id == node.id && !tma_domain.trim().is_empty() {
            tma_domain.trim().to_string()
        } else {
            subscription_host.clone()
        };
        let upstream = state.settings.get_or_default("panel_url", "").await;
        let upstream = if upstream.is_empty() {
            "https://YOUR_HUB_IP".to_string()
        } else {
            upstream
        };
        format!(
            "{subscription_host}, {tma_host} {{\n    encode zstd gzip\n\n    handle /sub/* {{\n        reverse_proxy {upstream}\n    }}\n\n    handle /app/* {{\n        reverse_proxy {upstream}\n    }}\n\n    handle /bot/* {{\n        reverse_proxy {upstream}\n    }}\n\n    handle {{\n        respond \"relay-only host\" 403\n    }}\n}}",
            subscription_host = subscription_host,
            tma_host = tma_host,
            upstream = upstream
                .trim_start_matches("https://")
                .trim_start_matches("http://"),
        )
    } else {
        String::new()
    };

    let active_page = if node.is_relay_node() {
        "nodes_relay".to_string()
    } else {
        "nodes_exit".to_string()
    };

    let template = NodeManageTemplate {
        node,
        all_nodes,
        admin_path,
        username,
        active_page,
        is_auth: true,
        inbounds,
        discovered_snis,
        nginx_proxy_config,
        caddy_proxy_config,
    };

    Html(template.render().unwrap_or_default()).into_response()
}

/// Возвращает модальное окно с командами для ручного восстановления агента.
/// Функционален: показывает systemd команды для диагностики на сервере.
pub async fn get_node_rescue(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node = match state.infrastructure_service.get_node_by_id(id).await {
        Ok(n) => n,
        Err(e) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                format!("Node not found: {}", e),
            )
                .into_response();
        }
    };

    let template = NodeRescueModalTemplate {
        node,
        admin_path: state.admin_path.clone(),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

pub async fn trigger_scan(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    info!("Manual Neighbor Sniper scan triggered for node: {}", id);

    // Notify Agent via PubSub
    if let Err(e) = state
        .pubsub
        .publish(&format!("node_events:{}", id), "scan")
        .await
    {
        error!("Failed to publish scan event: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Signal failed",
        )
            .into_response();
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
    let _ = state
        .pubsub
        .publish(&format!("node_events:{}", node_id), "update")
        .await;

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
    let _ = state
        .pubsub
        .publish(&format!("node_events:{}", node_id), "update")
        .await;

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

pub async fn restart_node(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    info!("Manual restart triggered for node: {}", id);

    // Publish restart event
    if let Err(e) = state
        .pubsub
        .publish(&format!("node_events:{}", id), "restart")
        .await
    {
        error!("Failed to publish restart event: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Signal failed",
        )
            .into_response();
    }

    // Optional: Update last_sync_trigger to denote manual intervention
    let _ = sqlx::query("UPDATE nodes SET last_sync_trigger = 'Manual Restart' WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("HX-Refresh", "true".parse().unwrap());
    (axum::http::StatusCode::OK, headers, "Restart Signal Sent").into_response()
}

pub async fn rotate_node_inbounds(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Manual inbound rotation requested for node {}", node_id);

    let inbound_ids: Vec<i32> = sqlx::query_scalar(
        "SELECT id FROM inbounds WHERE node_id = $1 AND tag LIKE 'tpl_%' AND enable = TRUE",
    )
    .bind(node_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    if inbound_ids.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "No templated inbounds found",
        )
            .into_response();
    }

    let mut rotated = 0usize;
    let mut failed = 0usize;
    for inbound_id in inbound_ids {
        match state
            .generator_service
            .rotate_inbound(inbound_id as i64)
            .await
        {
            Ok(_) => rotated += 1,
            Err(e) => {
                failed += 1;
                error!(
                    "Failed to rotate inbound {} on node {}: {}",
                    inbound_id, node_id, e
                );
            }
        }
    }

    if rotated > 0 {
        let _ = sqlx::query("UPDATE nodes SET last_sync_trigger = 'Manual Rotation' WHERE id = $1")
            .bind(node_id)
            .execute(&state.pool)
            .await;
        let _ = state
            .pubsub
            .publish(&format!("node_events:{}", node_id), "update")
            .await;
    }

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("HX-Refresh", "true".parse().unwrap());
    (
        axum::http::StatusCode::OK,
        headers,
        format!("Rotated {} inbound(s), {} failed", rotated, failed),
    )
        .into_response()
}

pub async fn get_node_config_preview(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state
        .orchestration_service
        .generate_node_config_json(id)
        .await
    {
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
        }
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

/// POST /admin/nodes/{id}/rotate-sni
/// Ручная ротация Reality SNI для конкретной ноды из панели администратора.
/// Доступна независимо от cooldown агента — это явное действие оператора.
pub async fn admin_rotate_node_sni(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if get_auth_user(&state, &jar).await.is_none() {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    info!("Admin manual SNI rotation requested for node {}", node_id);

    match state
        .security_service
        .rotate_node_sni(node_id, "Manual Admin Rotation")
        .await
    {
        Ok((old_sni, new_sni, _rotation_id)) => {
            info!(
                "✅ Admin SNI rotation: Node {} {} → {}",
                node_id, old_sni, new_sni
            );
            // Regenerate config with new SNI and notify node
            let _ = state
                .orchestration_service
                .generate_node_config_json(node_id)
                .await;
            let _ = state
                .orchestration_service
                .notify_node_update(node_id)
                .await;

            let mut resp_headers = axum::http::HeaderMap::new();
            resp_headers.insert(
                "HX-Trigger",
                "refresh_nodes".parse().unwrap(),
            );
            (
                axum::http::StatusCode::OK,
                resp_headers,
                format!("SNI ротирован: {} → {}", old_sni, new_sni),
            )
                .into_response()
        }
        Err(e) if e.to_string().contains("No other SNI") => {
            warn!("SNI pool exhausted for node {}: {}", node_id, e);
            (
                axum::http::StatusCode::CONFLICT,
                "Пул SNI исчерпан — нет альтернативных записей",
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to rotate SNI for node {}: {}", node_id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка ротации: {}", e),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct SniIntervalForm {
    /// None/empty = глобальная настройка, "0" = никогда, "24"/"168"/"720" = часы
    pub sni_renew_interval_hours: Option<String>,
}

/// POST /admin/nodes/{id}/sni-interval
/// Обновляет per-node интервал автоматической ротации SNI.
pub async fn update_sni_interval(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<SniIntervalForm>,
) -> impl IntoResponse {
    if get_auth_user(&state, &jar).await.is_none() {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    // Парсим значение: пустая строка → NULL (глобальная настройка)
    let interval: Option<i32> = match form.sni_renew_interval_hours.as_deref() {
        None | Some("") | Some("global") => None,
        Some(v) => v.parse::<i32>().ok(),
    };

    if let Err(e) = sqlx::query(
        "UPDATE nodes SET sni_renew_interval_hours = $1 WHERE id = $2",
    )
    .bind(interval)
    .bind(node_id)
    .execute(&state.pool)
    .await
    {
        error!(
            "Failed to update sni_renew_interval_hours for node {}: {}",
            node_id, e
        );
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Ошибка обновления интервала",
        )
            .into_response();
    }

    info!(
        "Updated sni_renew_interval_hours for node {} to {:?}",
        node_id, interval
    );

    let mut resp_headers = axum::http::HeaderMap::new();
    resp_headers.insert("HX-Trigger", "refresh_nodes".parse().unwrap());
    (axum::http::StatusCode::OK, resp_headers, "OK").into_response()
}
