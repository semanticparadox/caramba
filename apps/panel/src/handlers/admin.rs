use axum::{
    extract::{State, Form, Path},
    response::{IntoResponse, Html},
};
use askama::Template;
use serde::Deserialize;
use crate::AppState;
use crate::models::node::Node;
use crate::models::store::{Plan, User, Order};
use std::collections::HashMap;
use tracing::{info, error};
use axum_extra::extract::cookie::{Cookie, CookieJar};

#[derive(Template)]
#[template(path = "settings.html")]
pub struct SettingsTemplate {
    pub masked_bot_token: String,
    pub bot_status: String,
    pub masked_payment_api_key: String,
    pub payment_ipn_url: String,
    pub currency_rate: String,
    pub support_url: String,
    pub brand_name: String,
    pub terms_of_service: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}


#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub total_users: i64,
    pub active_subs: i64,
    pub total_revenue: f64,
    pub active_nodes: i64,
    // Add traffic stats
    // Add traffic stats
    pub total_traffic: String,
    pub bot_status: String,
    pub is_auth: bool,
    pub activities: Vec<crate::models::activity::Activity>,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "partials/bot_status.html")]
pub struct BotStatusPartial {
    pub bot_status: String,
    pub admin_path: String,
}

#[derive(Template)]
#[template(path = "nodes.html")]
pub struct NodesTemplate {
    pub nodes: Vec<Node>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}




#[derive(Template)]
#[template(path = "users.html")]
pub struct UsersTemplate {
    pub users: Vec<crate::models::store::User>,
    pub search: String,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "bot_logs.html")]
pub struct BotLogsTemplate {
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template)]
#[template(path = "transactions.html")]
pub struct TransactionsTemplate {
    pub orders: Vec<OrderWithUser>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

pub struct OrderWithUser {
    pub id: i64,
    pub username: String,
    pub total_amount: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}



#[derive(Deserialize)]
pub struct SaveSettingsForm {
    pub bot_token: Option<String>,
    pub payment_api_key: Option<String>,
    pub payment_ipn_url: Option<String>,
    pub currency_rate: Option<String>,
    pub support_url: Option<String>,
    pub brand_name: Option<String>,
    pub terms_of_service: Option<String>,
}

pub async fn get_login() -> impl IntoResponse {
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    let template = LoginTemplate { 
        is_auth: false,
        admin_path,
        active_page: "login".to_string(),
    };
    Html(template.render().unwrap_or_default())
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let admin_res = sqlx::query("SELECT password_hash FROM admins WHERE username = ?")
        .bind(&form.username)
        .fetch_optional(&state.pool)
        .await;

    match admin_res {
        Ok(Some(row)) => {
            use sqlx::Row;
            let hash: String = row.get(0);
            if bcrypt::verify(&form.password, &hash).unwrap_or(false) {
                let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                let cookie = Cookie::build(("admin_session", state.session_secret.clone()))
                    .path("/")
                    .http_only(true)
                    .build();
                
                // For HTMX requests, we use HX-Redirect header
                // For HTMX requests, we use HX-Redirect header.
                // We return 200 OK to prevent HTMX from just swapping the redirect response body into the current page.
                // The HX-Redirect header forces a full page navigation.
                let mut headers = axum::http::HeaderMap::new();
                headers.insert("HX-Redirect", format!("{}/dashboard", admin_path).parse().unwrap());

                return (
                    axum::http::StatusCode::OK,
                    jar.add(cookie),
                    headers,
                ).into_response();
            }
        }
        _ => {}
    }

    (axum::http::StatusCode::UNAUTHORIZED, "Invalid username or password").into_response()
}

pub async fn activate_node(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Forcing activation for node ID: {}", id);

    let res = sqlx::query("UPDATE nodes SET status = 'active' WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Node", &format!("Node {} activated", id)).await;
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            ([("HX-Redirect", &format!("{}/nodes", admin_path))], "Activated").into_response()
        },
        Err(e) => {
            error!("Failed to activate node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to activate node").into_response()
        }
    }
}

pub async fn logout(jar: CookieJar) -> impl IntoResponse {
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let cookie = Cookie::build(("admin_session", ""))
        .path("/")
        // expire immediately
        .build();
    
    (jar.add(cookie), axum::response::Redirect::to(&format!("{}/login", admin_path))).into_response()
}

pub async fn get_dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let active_nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let active_subs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let total_revenue: f64 = sqlx::query_scalar("SELECT SUM(amount) FROM payments WHERE status = 'completed'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0.0);



    let total_traffic_bytes: i64 = sqlx::query_scalar("SELECT SUM(used_traffic) FROM subscriptions")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let _total_traffic = format!("{:.2} GB", total_traffic_bytes as f64 / (1024.0 * 1024.0 * 1024.0));

    let bot_status = state.settings.get_or_default("bot_status", "stopped").await;

    let activities = crate::services::activity_service::ActivityService::get_latest(&state.pool, 10)
        .await
        .unwrap_or_default();

    let template = DashboardTemplate {
        total_users,
        active_subs,
        total_revenue,
        active_nodes,
        total_traffic: format!("{:.2} GB", total_traffic_bytes as f64 / 1024.0 / 1024.0 / 1024.0),
        bot_status,
        is_auth: true,
        activities,
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "dashboard".to_string(),
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

#[derive(Deserialize)]
pub struct InstallNodeForm {
    pub name: String,
    pub ip: Option<String>,
    pub vpn_port: i64,
    pub auto_configure: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateNodeForm {
    pub name: String,
    pub ip: String,
}

#[derive(Template)]
#[template(path = "node_edit_modal.html")]
pub struct NodeEditModalTemplate {
    pub node: Node,
    pub admin_path: String,
}

fn mask_key(key: &str) -> String {
    let len = key.len();
    if len < 8 {
        return "*".repeat(len);
    }
    let start_len = (len as f64 * 0.1).ceil() as usize;
    let mask_len = (len as f64 * 0.7).floor() as usize;
    let end_len = len.saturating_sub(start_len + mask_len);
    
    let start = &key[0..start_len];
    let end = &key[len - end_len..];
    format!("{}{}{}", start, "*".repeat(mask_len), end)
}

pub async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    let bot_status = state.settings.get_or_default("bot_status", "stopped").await;
    let payment_api_key = state.settings.get_or_default("payment_api_key", "").await;
    let payment_ipn_url = state.settings.get_or_default("payment_ipn_url", "").await;
    let currency_rate = state.settings.get_or_default("currency_rate", "1.0").await;
    let support_url = state.settings.get_or_default("support_url", "").await;
    let brand_name = state.settings.get_or_default("brand_name", "EXA ROBOT").await;
    let terms_of_service = state.settings.get_or_default("terms_of_service", "Welcome to EXA ROBOT.").await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| {
        tracing::warn!("ADMIN_PATH env var not found in get_settings handler! Defaulting to /admin");
        "/admin".to_string()
    });
    tracing::info!("get_settings handler seeing ADMIN_PATH: {}", admin_path);

    let masked_bot_token = if !bot_token.is_empty() { mask_key(&bot_token) } else { "".to_string() };
    let masked_payment_api_key = if !payment_api_key.is_empty() { mask_key(&payment_api_key) } else { "".to_string() };

    let template = SettingsTemplate {
        masked_bot_token,
        bot_status,
        masked_payment_api_key,
        payment_ipn_url,
        currency_rate,
        support_url,
        brand_name,
        terms_of_service,
        is_auth: true,
        admin_path: std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string()),
        active_page: "settings".to_string(),
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn save_settings(
    State(state): State<AppState>,
    Form(form): Form<SaveSettingsForm>,
) -> impl IntoResponse {
    info!("Saving system settings");
    
    let mut settings = HashMap::new();
    let is_running = state.bot_manager.is_running().await;

    // Logic for single-field masked update:
    // If input == masked_value(current_db_value), then do NOT update (user didn't touch it)
    // Else, update.
    
    let current_bot_token = state.settings.get_or_default("bot_token", "").await;
    let masked_bot_token = if !current_bot_token.is_empty() { mask_key(&current_bot_token) } else { "".to_string() };
    
    if let Some(v) = form.bot_token {
        if !v.is_empty() && v != masked_bot_token {
            if is_running {
                // Return error if trying to update token while running
                 return (
                    axum::http::StatusCode::BAD_REQUEST, 
                    "Cannot update Bot Token while bot is running. Please stop the bot first."
                ).into_response();
            }
            settings.insert("bot_token".to_string(), v);
        }
    }

    let current_payment_key = state.settings.get_or_default("payment_api_key", "").await;
    let masked_payment_key = if !current_payment_key.is_empty() { mask_key(&current_payment_key) } else { "".to_string() };

    if let Some(v) = form.payment_api_key {
        if !v.is_empty() && v != masked_payment_key {
            settings.insert("payment_api_key".to_string(), v);
        }
    }

    // For other fields, update if provided (allow empty to clear)
    if let Some(v) = form.payment_ipn_url { settings.insert("payment_ipn_url".to_string(), v); }
    if let Some(v) = form.currency_rate { settings.insert("currency_rate".to_string(), v); }
    if let Some(v) = form.support_url { settings.insert("support_url".to_string(), v); }
    if let Some(v) = form.brand_name { settings.insert("brand_name".to_string(), v); }
    if let Some(v) = form.terms_of_service { settings.insert("terms_of_service".to_string(), v); }

    match state.settings.set_multiple(settings).await {
        Ok(_) => {
             // Basic toast notification via HX-Trigger could be added here
             ([("HX-Refresh", "true")], "Settings Saved").into_response()
        },
        Err(e) => {
            error!("Failed to save settings: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to save settings").into_response()
        }
    }
}

pub async fn toggle_bot(State(state): State<AppState>) -> impl IntoResponse {
    let is_running = state.bot_manager.is_running().await;
    let new_status;

    if is_running {
        info!("Stopping bot via toggle");
        state.bot_manager.stop_bot().await;
        new_status = "stopped".to_string();
    } else {
        info!("Starting bot via toggle");
        let token = state.settings.get_or_default("bot_token", "").await;
        if token.is_empty() {
             return (axum::http::StatusCode::BAD_REQUEST, "Bot token is empty").into_response();
        }
        state.bot_manager.start_bot(token, state.clone()).await;
        new_status = "running".to_string();
    }

    let _ = state.settings.set("bot_status", &new_status).await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = BotStatusPartial {
        bot_status: new_status,
        admin_path,
    };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// Nodes Handlers
pub async fn get_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.orchestration_service.get_all_nodes().await.unwrap_or_default();
    
    let template = NodesTemplate { 
        nodes, 
        is_auth: true, 
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "nodes".to_string(),
    };
    Html(template.render().unwrap())
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

    // Generate Token if IP is missing or if auto-config requested
    // Actually, we always generate a token for "Smart Setup" possibility
    let token = uuid::Uuid::new_v4().to_string();
    let auto_configure = form.auto_configure.unwrap_or(false);

    // If IP is empty, we set it to 'pending' placeholder or allow NULL? Schema says TEXT NOT NULL UNIQUE.
    // We should probably allow placeholder IP (e.g. "pending-<token>") or make IP nullable.
    // Migration didn't make IP nullable. So let's use a unique placeholder.
    let ip = if let Some(ref i) = form.ip {
        if i.is_empty() { format!("pending-{}", &token[0..8]) } else { i.clone() }
    } else {
        format!("pending-{}", &token[0..8])
    };

    let res = sqlx::query("INSERT INTO nodes (name, ip, vpn_port, status, join_token, auto_configure) VALUES (?, ?, ?, 'installing', ?, ?) RETURNING id")
        .bind(&form.name)
        .bind(&ip)
        .bind(form.vpn_port)
        .bind(&token)
        .bind(auto_configure)
        .fetch_one(&state.pool)
        .await;

    match res {
        Ok(row) => {
            use sqlx::Row;
            let id: i64 = row.get(0);
            
            // Just register in node manager (sets status to 'new' explicitly)
            state.node_manager.add_node(id).await;
            
            // Initialize default inbounds (Reality Keys, etc.)
            // We spawn this to not block the redirect, or await it? 
            // Awaiting is safer to ensure keys exist when user connects.
            if let Err(e) = state.orchestration_service.init_default_inbounds(id).await {
                error!("Failed to initialize inbounds for node {}: {}", id, e);
                // We don't fail the request, but log it. Admin might need to "reset" node later.
            }
            
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
            
            let mut headers = axum::http::HeaderMap::new();
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
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await {
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

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    // Ensure leading slash
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let template = NodeEditModalTemplate { node, admin_path };
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
    info!("Updating node ID: {}", id);
    
    // If password is empty, don't update it, keep old one? But form sends it. 
    // Usually admin puts new password or we fetch old one if empty.
    // Let's assume for simplicity we update everything. If password field is empty, it might clear it.
    // Better logic: if password is NOT empty, update it.
    
    let query = sqlx::query("UPDATE nodes SET name = ?, ip = ? WHERE id = ?")
        .bind(&form.name)
        .bind(&form.ip)
        .bind(id);

    match query.execute(&state.pool).await {
        Ok(_) => {
             let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
             let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
             
             let mut headers = axum::http::HeaderMap::new();
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
    tokio::spawn(async move {
        // Delete existing inbounds to force regeneration with fresh keys
        if let Err(e) = sqlx::query("DELETE FROM inbounds WHERE node_id = ?")
            .bind(id)
            .execute(&orch.pool)
            .await 
        {
            error!("Failed to delete old inbounds: {}", e);
        } else {
            info!("Deleted old inbounds for node {}", id);
        }
        
        // Recreate default inbounds with fresh keys
        if let Err(e) = orch.init_default_inbounds(id).await {
            error!("Failed to recreate inbounds for node {}: {}", id, e);
        } else {
            info!("Successfully regenerated inbounds with fresh keys for node {}", id);
        }
    });

    axum::http::StatusCode::ACCEPTED
}

// Node Scripts
pub async fn get_node_install_script(
    Path(_id): Path<i64>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // In the future, we can inject unique tokens or specific config here based on ID.
    // Use embedded script
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


// Plans Handlers
pub async fn get_plans(State(state): State<AppState>) -> impl IntoResponse {
    let mut plans = match sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb FROM plans")
        .fetch_all(&state.pool)
        .await {
            Ok(p) => {
                info!("Successfully fetched {} plans from DB", p.len());
                p
            },
            Err(e) => {
                error!("Failed to fetch plans from DB (Admin): {}", e);
                Vec::new()
            }
        };

    for plan in &mut plans {
        let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC"
        )
        .bind(plan.id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        plan.durations = durations;
    }

    let nodes = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE is_enabled = 1").fetch_all(&state.pool).await.unwrap_or_default();

    #[derive(Template)]
    #[template(path = "plans.html")]
    pub struct PlansTemplate {
        pub plans: Vec<Plan>,
        pub nodes: Vec<Node>,
        pub is_auth: bool,
        pub admin_path: String,
        pub active_page: String,
    }

    let template = PlansTemplate { 
        plans, 
        nodes,
        is_auth: true, 
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        }, 
        active_page: "plans".to_string() 
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

// Helper for handling single or multiple values in form
#[allow(dead_code)]
fn deserialize_vec_or_single<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    struct VecOrSingleVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> serde::de::Visitor<'de> for VecOrSingleVisitor<T>
    where
        T: Deserialize<'de> + std::str::FromStr,
        <T as std::str::FromStr>::Err: std::fmt::Display,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a sequence or a single value")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = seq.next_element()? {
                vec.push(elem);
            }
            Ok(vec)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            match T::from_str(value) {
                Ok(val) => Ok(vec![val]),
                Err(e) => Err(serde::de::Error::custom(format!("Parse error: {}", e))),
            }
        }
    }

    deserializer.deserialize_any(VecOrSingleVisitor(std::marker::PhantomData))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CreatePlanForm {
    pub name: String,
    pub description: String,
    #[serde(deserialize_with = "deserialize_vec_or_single")]
    pub price: Vec<i64>,
    #[serde(deserialize_with = "deserialize_vec_or_single")]
    pub duration_days: Vec<i32>,
    #[serde(deserialize_with = "deserialize_vec_or_single")]
    pub traffic_gb: Vec<i32>,
}

pub async fn add_plan(
    State(state): State<AppState>,
    Form(raw_form): Form<Vec<(String, String)>>,
) -> impl IntoResponse {
    let mut name = String::new();
    let mut description = String::new();
    let mut device_limit: i32 = 3; // Default value
    let mut duration_days: Vec<i32> = Vec::new();
    let mut price: Vec<i64> = Vec::new();
    let mut traffic_limit_gb: i32 = 0;

    let mut node_ids: Vec<i64> = Vec::new();


    for (key, value) in raw_form {
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            "device_limit" => {
                if let Ok(v) = value.parse() {
                    device_limit = v;
                }
            },
            "duration_days" => {
                if let Ok(v) = value.parse() {
                    duration_days.push(v);
                }
            },
            "price" => {
                if let Ok(v) = value.parse() {
                    price.push(v);
                }
            },
            "traffic_limit_gb" => {
                if let Ok(v) = value.parse() {
                    traffic_limit_gb = v;
                }
            },
            "node_ids" => {
                if let Ok(v) = value.parse() {
                    node_ids.push(v);
                }
            },
            _ => {}
        }
    }

    info!("Adding flexible plan: {}", name);
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    // 1. Insert Plan
    // Using traffic_limit_gb for the plan
    let plan_id: i64 = match sqlx::query("INSERT INTO plans (name, description, is_active, price, traffic_limit_gb, device_limit) VALUES (?, ?, 1, 0, ?, ?) RETURNING id")
        .bind(&name)
        .bind(&description)
        .bind(traffic_limit_gb)
        .bind(device_limit)
        .fetch_one(&mut *tx)
        .await {
            Ok(row) => {
                use sqlx::Row;
                row.get(0)
            },
            Err(e) => {
                error!("Failed to insert plan: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to add plan").into_response();
            }
        };

    // 2. Insert Durations
    let count = duration_days.len().min(price.len());
    for i in 0..count {
        let days = duration_days[i];
        let p = price[i];

        if let Err(e) = sqlx::query("INSERT INTO plan_durations (plan_id, duration_days, price) VALUES (?, ?, ?)")
            .bind(plan_id)
            .bind(days)
            .bind(p)
            .execute(&mut *tx)
            .await {
                error!("Failed to insert plan duration {}: {}", i, e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to add plan durations").into_response();
            }
    }

    // 3. Link to Nodes
    for node_id in node_ids {
        if let Err(e) = sqlx::query("INSERT INTO plan_nodes (plan_id, node_id) VALUES (?, ?)")
            .bind(plan_id)
            .bind(node_id)
            .execute(&mut *tx)
            .await {
                error!("Failed to link new plan to node: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to link plan to node").into_response();
            }
    }

    if let Err(e) = tx.commit().await {
         error!("Failed to commit plan transaction: {}", e);
         return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to create plan").into_response();
    }
    
    // Log activity
    let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Plan", &format!("New plan created: {}", name)).await;

    // Redirect to plans page to show new plan
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("HX-Redirect", format!("{}/plans", admin_path).parse().unwrap());
    (axum::http::StatusCode::OK, headers, "Plan Created").into_response()
}

pub async fn delete_plan(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete plan: {}", id);
    
    // 1. Use Store Service to delete plan + refund active users
    match state.store_service.delete_plan_and_refund(id).await {
        Ok((refunded_users, total_refunded_cents)) => {
            info!("Plan {} deleted. Refunded {} users (Total: ${:.2})", id, refunded_users, total_refunded_cents as f64 / 100.0);
            (axum::http::StatusCode::OK, "").into_response()
        },
        Err(e) => {
            error!("Failed to delete plan {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete plan: {}", e)).into_response()
        }
    }
}

pub async fn get_plan_edit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let plan = match sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb FROM plans WHERE id = ?").bind(id).fetch_optional(&state.pool).await {
        Ok(Some(mut p)) => {
            let durations = sqlx::query_as::<_, crate::models::store::PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = ? ORDER BY duration_days ASC"
            )
            .bind(p.id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
            p.durations = durations;
            p
        },
        _ => return (axum::http::StatusCode::NOT_FOUND, "Plan not found").into_response(),
    };

    let all_nodes = sqlx::query_as::<_, crate::models::node::Node>("SELECT * FROM nodes WHERE is_enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let linked_node_ids: Vec<i64> = sqlx::query_scalar("SELECT node_id FROM plan_nodes WHERE plan_id = ?")
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    #[derive(Template)]
    #[template(path = "plan_edit_modal.html")]
    struct PlanEditModalTemplate {
        plan: Plan,
        nodes: Vec<(crate::models::node::Node, bool)>,
        admin_path: String,
    }

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let nodes_with_status: Vec<(crate::models::node::Node, bool)> = all_nodes.into_iter().map(|n| {
        let is_linked = linked_node_ids.contains(&n.id);
        (n, is_linked)
    }).collect();

    Html(PlanEditModalTemplate { plan, nodes: nodes_with_status, admin_path }.render().unwrap_or_default()).into_response()
}

pub async fn update_plan(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(raw_form): Form<Vec<(String, String)>>,
) -> impl IntoResponse {
    info!("Updating flexible plan (raw): {}", id);

    let mut name = String::new();
    let mut description = String::new();
    let mut device_limit: i32 = 3; 
    let mut duration_days: Vec<i32> = Vec::new();
    let mut price: Vec<i64> = Vec::new();
    let mut traffic_limit_gb: i32 = 0;

    let mut node_ids: Vec<i64> = Vec::new();

    for (key, value) in raw_form {
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            "device_limit" => {
                if let Ok(v) = value.parse() {
                    device_limit = v;
                }
            },
            "duration_days" => {
                if let Ok(v) = value.parse() {
                    duration_days.push(v);
                }
            },
            "price" => {
                if let Ok(v) = value.parse() {
                    price.push(v);
                }
            },
            "traffic_limit_gb" => {
                if let Ok(v) = value.parse() {
                    traffic_limit_gb = v;
                }
            },
            "node_ids" => {
                if let Ok(v) = value.parse() {
                    node_ids.push(v);
                }
            },
            _ => {}
        }
    }

    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    // 1. Update Plan
    if let Err(e) = sqlx::query("UPDATE plans SET name = ?, description = ?, device_limit = ?, traffic_limit_gb = ? WHERE id = ?")
        .bind(&name)
        .bind(&description)
        .bind(device_limit)
        .bind(traffic_limit_gb)
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to update plan: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update plan").into_response();
        }

    // 2. Delete existing durations
    if let Err(e) = sqlx::query("DELETE FROM plan_durations WHERE plan_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to clear durations: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to clear durations").into_response();
        }

    // 3. Insert new durations
    let count = duration_days.len().min(price.len());

    for i in 0..count {
        let days = duration_days[i];
        let p = price[i];

        if let Err(e) = sqlx::query("INSERT INTO plan_durations (plan_id, duration_days, price) VALUES (?, ?, ?)")
            .bind(id)
            .bind(days)
            .bind(p)
            .execute(&mut *tx)
            .await {
                error!("Failed to insert duration: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to insert duration: {}", e)).into_response();
            }
    }

    // 4. Update Node Bindings (Modernized approach)
    if let Err(e) = sqlx::query("DELETE FROM plan_nodes WHERE plan_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to clear plan_nodes: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to clear plan bindings").into_response();
        }

    for node_id in node_ids {
        if let Err(e) = sqlx::query("INSERT INTO plan_nodes (plan_id, node_id) VALUES (?, ?)")
            .bind(id)
            .bind(node_id)
            .execute(&mut *tx)
            .await {
                error!("Failed to link plan to node: {}", e);
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to link plan to node").into_response();
            }
    }

    if let Err(e) = tx.commit().await {
        error!("Failed to commit update transaction: {}", e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Transaction failed").into_response();
    }

    let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Plan", &format!("Plan {} updated: {}", id, name)).await;

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    ([("HX-Redirect", format!("{}/plans", admin_path))], "Redirecting...").into_response()
}
// Users Handlers
pub async fn get_users(
    State(state): State<AppState>,
    query: axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let search = query.get("search").cloned().unwrap_or_default();
    let users = if search.is_empty() {
        sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    } else {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username LIKE ? OR full_name LIKE ? ORDER BY created_at DESC")
            .bind(format!("%{}%", search))
            .bind(format!("%{}%", search))
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    };

    let template = UsersTemplate { users, search, is_auth: true, admin_path: {
        let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
        if p.starts_with('/') { p } else { format!("/{}", p) }
    }, active_page: "users".to_string() };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

#[derive(Template)]
#[template(path = "user_details.html")]
pub struct UserDetailsTemplate {
    pub user: User,
    pub subscriptions: Vec<SubscriptionWithPlan>,
    pub orders: Vec<UserOrderDisplay>,
    pub referrals: Vec<crate::services::store_service::DetailedReferral>,
    pub total_referral_earnings: i64,
    pub available_plans: Vec<Plan>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserOrderDisplay {
    pub id: i64,
    pub total_amount: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct AdminGiftForm {
    pub duration_id: i64,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SubscriptionWithPlan {
    pub id: i64,
    pub plan_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub price: i64,
    pub active_devices: i64,
    pub device_limit: i64,
}

pub async fn admin_gift_subscription(
    Path(user_id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<AdminGiftForm>,
) -> impl IntoResponse {
    // 1. Fetch Duration details to get plan_id and days
    let duration = match sqlx::query_as::<_, crate::models::store::PlanDuration>("SELECT * FROM plan_durations WHERE id = ?")
        .bind(form.duration_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return (axum::http::StatusCode::BAD_REQUEST, "Invalid duration ID").into_response(),
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    match state.store_service.admin_gift_subscription(user_id, duration.plan_id, duration.duration_days).await {
        Ok(sub) => {
            // Find User TG ID for notification
            if let Ok(Some(user)) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?").bind(user_id).fetch_optional(&state.pool).await {
                 let msg = format!("🎁 *Gift Received\\!*\n\nYou have received a new subscription\\.\nExpires: {}", sub.expires_at.format("%Y-%m-%d"));
                 let _ = state.bot_manager.send_notification(
                     user.tg_id,
                     &msg
                 ).await;
            }

            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            return axum::response::Redirect::to(&format!("{}/users/{}", admin_path, user_id)).into_response();
        },
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR, 
                format!("Failed to gift subscription: {}", e)
            ).into_response();
        }
    }
}

pub async fn get_user_details(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // 1. Fetch User
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let user = match user {
        Some(u) => u,
        None => return (axum::http::StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    // 2. Fetch Active Subscriptions with Plan Name, Price, and Device Info
    // We infer the price by matching plan_id and the duration (expires_at - created_at)
    // against plan_durations table.
    // Device count is calculated from subscription_ip_tracking (last 15 minutes)
    let subscriptions = match sqlx::query_as::<_, SubscriptionWithPlan>(
        r#"
        SELECT 
            s.id, 
            p.name as plan_name, 
            s.expires_at, 
            s.created_at,
            s.status,
            0 as price, 
            COALESCE(
                (SELECT COUNT(DISTINCT client_ip) 
                 FROM subscription_ip_tracking 
                 WHERE subscription_id = s.id 
                 AND datetime(last_seen_at) > datetime('now', '-15 minutes')),
                0
            ) as active_devices,
            p.device_limit as device_limit
        FROM subscriptions s
        JOIN plans p ON s.plan_id = p.id
        WHERE s.user_id = ?
        "#
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await {
        Ok(subs) => subs,
        Err(e) => {
            error!("Failed to fetch user subscriptions: {}", e);
            // Return error to UI instead of empty list
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to fetch subs: {}", e)).into_response();
        }
    };

    // 3. Fetch Order History
    let db_orders = sqlx::query_as::<_, Order>(
        "SELECT id, user_id, total_amount, status, created_at, paid_at FROM orders WHERE user_id = ? ORDER BY created_at DESC"
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
         error!("Failed to fetch user orders: {}", e);
         e
    })
    .unwrap_or_default();

    let orders = db_orders.into_iter().map(|o| UserOrderDisplay {
        id: o.id,
        total_amount: format!("{:.2}", o.total_amount as f64 / 100.0),
        status: o.status,
        created_at: o.created_at.format("%Y-%m-%d").to_string(),
    }).collect();

    // 4. Fetch Referrals & Earnings
    let referrals = state.store_service.get_user_referrals(id).await.unwrap_or_default();
    let earnings_cents = state.store_service.get_user_referral_earnings(id).await.unwrap_or(0);
    let _total_referral_earnings = format!("{:.2}", earnings_cents as f64 / 100.0);

    // 5. Fetch Available Plans for Gifting
    let available_plans = state.store_service.get_active_plans().await.unwrap_or_default();

    let template = UserDetailsTemplate {
        user,
        subscriptions,
        orders,
        referrals,
        total_referral_earnings: earnings_cents,
        available_plans,
        is_auth: true,
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "users".to_string(),
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





#[derive(Deserialize)]
pub struct UpdateUserForm {
    pub balance: i64,
    pub is_banned: bool,
    pub referral_code: Option<String>,
}

pub async fn update_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateUserForm>,
) -> impl IntoResponse {
    // Fetch previous state
    let old_user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let res = sqlx::query("UPDATE users SET balance = ?, is_banned = ?, referral_code = ? WHERE id = ?")
        .bind(form.balance)
        .bind(form.is_banned)
        .bind(form.referral_code.as_deref().map(|s| s.trim()))
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let _ = crate::services::activity_service::ActivityService::log(&state.pool, "User", &format!("User {} updated: Balance={}, Banned={}", id, form.balance, form.is_banned)).await;
            
            if let Some(u) = old_user {
                // Notify on ban status change
                if u.is_banned != form.is_banned {
                    let msg = if form.is_banned {
                        "🚫 *Account Banned*\n\nYour account has been suspended by an administrator\\."
                    } else {
                        "✅ *Account Unbanned*\n\nYour account has been reactivated\\. Welcome back\\!"
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, msg).await;
                }

                // Notify on balance change (deposit/deduction by admin)
                if u.balance != form.balance {
                    let diff = form.balance - u.balance;
                    let amount = format!("{:.2}", diff.abs() as f64 / 100.0);
                    let msg = if diff > 0 {
                        format!("💰 *Balance Updated*\n\nAdministrator added *${}* to your account\\.", amount)
                    } else {
                        format!("📉 *Balance Updated*\n\nAdministrator deducted *${}* from your account\\.", amount)
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, &msg).await;
                }
            }

            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            ([("HX-Redirect", format!("{}/users/{}", admin_path, id))], "Updated").into_response()
        },
        Err(e) => {
            error!("Failed to update user {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update user").into_response()
        }
    }
}

pub async fn update_user_balance(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>, // Accept generic form for the modal which sends 'balance'
) -> impl IntoResponse {
    let balance_str = form.get("balance").unwrap_or(&"0".to_string()).clone();
    let balance: i64 = balance_str.parse().unwrap_or(0);

    let res = sqlx::query("UPDATE users SET balance = ? WHERE id = ?")
        .bind(balance)
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            ([("HX-Redirect", format!("{}/users", admin_path))], "Updated").into_response()
        },
        Err(e) => {
            error!("Failed to update balance for user {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update balance").into_response()
        }
    }
}

pub async fn delete_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete subscription ID: {}", id);
    match state.store_service.admin_delete_subscription(id).await {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(),
        Err(e) => {
             error!("Failed to delete subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct RefundForm {
    pub amount: i64, 
}

pub async fn refund_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<RefundForm>,
) -> impl IntoResponse {
    info!("Request to refund subscription ID: {} with amount {}", id, form.amount);
    match state.store_service.admin_refund_subscription(id, form.amount).await {
        Ok(_) => ([("HX-Refresh", "true")], "Refunded").into_response(),
        Err(e) => {
             error!("Failed to refund subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to refund: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ExtendForm {
    pub days: i32,
}

pub async fn extend_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<ExtendForm>,
) -> impl IntoResponse {
    info!("Request to extend subscription ID: {} by {} days", id, form.days);
    match state.store_service.admin_extend_subscription(id, form.days).await {
        Ok(_) => ([("HX-Refresh", "true")], "Extended").into_response(),
        Err(e) => {
             error!("Failed to extend subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to extend: {}", e)).into_response()
        }
    }
}


pub async fn handle_payment(
    State(state): State<AppState>,
    body: String,
) -> impl IntoResponse {
    info!("Received payment webhook");
    if let Err(e) = state.pay_service.handle_webhook(&body).await {
        error!("Failed to process payment webhook: {}", e);
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR;
    }
    axum::http::StatusCode::OK
}

pub async fn bot_logs_page(jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return axum::response::Redirect::to("/admin/login").into_response();
    }
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    Html(BotLogsTemplate { is_auth: true, admin_path, active_page: "settings".to_string() }.render().unwrap()).into_response()
}


pub async fn bot_logs_history(jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return "Unauthorized".to_string();
    }
    
    match std::fs::read_to_string("server.log") {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > 100 { lines.len() - 100 } else { 0 };
            lines[start..].join("\n")
        }
        Err(_) => "Error reading log file".to_string()
    }
}

static mut LAST_LOG_POS: u64 = 0;

pub async fn bot_logs_tail(jar: CookieJar) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return String::new();
    }
    
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    
    let current_pos = unsafe { LAST_LOG_POS };
    
    match File::open("server.log") {
        Ok(mut file) => {
            let metadata = file.metadata().unwrap();
            let file_len = metadata.len();
            
            if file_len < current_pos {
                unsafe { LAST_LOG_POS = 0; }
                file.seek(SeekFrom::Start(0)).ok();
            } else {
                file.seek(SeekFrom::Start(current_pos)).ok();
            }
            
            let reader = BufReader::new(file);
            let mut new_lines = Vec::new();
            
            for line in reader.lines() {
                if let Ok(line) = line {
                    new_lines.push(line);
                }
            }
            
            unsafe { LAST_LOG_POS = file_len; }
            
            new_lines.join("\n")
        }
        Err(_) => String::new()
    }
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    info!("Request to delete node ID: {}", id);

    // 2. Delete the node (Cascades to inbounds -> plan_inbounds)
    // Subscriptions are linked to plans, not nodes directly, so no need to touch them.
    // If we had direct node-user allocation, we would need to handle it.
    // But currently: Subscription -> Plan -> PlanInbounds -> Inbound -> Node.
    // Deleting Node deletes Inbounds (Cascade).
    // Deleting Inbounds should delete PlanInbounds (if cascade set? Otherwise might need manual cleanup).
    // Let's assume schema handles Inbounds ON DELETE CASCADE (it does).
    // PlanInbounds? Schema not fully visible but likely.
    
    // Proceed to delete node directly.

    // 3. Delete the node
    let res = sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            info!("Node {} deleted successfully", id);
            // Return explicit empty body for HTMX to remove the element
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
    
    // Fetch current status
    // Use unchecked query to avoid build failure if migration not applied
    let enabled_res: Result<bool, sqlx::Error> = sqlx::query_scalar("SELECT is_enabled FROM nodes WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await;

    let enabled = match enabled_res {
        Ok(e) => e,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
    };

    let new_status = !enabled;
    
    let res = sqlx::query("UPDATE nodes SET is_enabled = ? WHERE id = ?")
        .bind(new_status)
        .bind(id)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
            // Refresh the row
            ([("HX-Redirect", format!("{}/nodes", admin_path))], "Toggled").into_response()
        }
        Err(e) => {
            error!("Failed to toggle node {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to toggle node").into_response()
        }
    }
}

pub async fn get_transactions(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return axum::response::Redirect::to("/admin/login").into_response();
    }

    struct OrderQueryRow {
        id: i64,
        username: String,
        total_amount: i64,
        status: String,
        created_at: Option<chrono::NaiveDateTime>,
    }

    let orders = sqlx::query_as!(
        OrderQueryRow,
        r#"
        SELECT 
            o.id, 
            COALESCE(u.username, u.full_name, 'Unknown') as "username!", 
            o.total_amount, 
            o.status, 
            o.created_at
        FROM orders o
        JOIN users u ON o.user_id = u.id
        ORDER BY o.created_at DESC
        "#
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|row| OrderWithUser {
        id: row.id,
        username: row.username,
        total_amount: format!("{:.2}", (row.total_amount as f64) / 100.0),
        status: row.status,
        created_at: row.created_at.unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc()).and_utc().format("%Y-%m-%d %H:%M").to_string(),
    })
    .collect();

    let template = TransactionsTemplate {
        orders,
        is_auth: true,
        admin_path: {
            let p = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
            if p.starts_with('/') { p } else { format!("/{}", p) }
        },
        active_page: "transactions".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}


pub async fn get_subscription_devices(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&jar) {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let ips = match state.store_service.get_subscription_active_ips(sub_id).await {
        Ok(ips) => ips,
        Err(e) => {
            error!("Failed to fetch IPs for sub {}: {}", sub_id, e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch devices").into_response();
        }
    };

    if ips.is_empty() {
        return (axum::http::StatusCode::OK, "<p class='secondary'>No active devices found in the last 15 minutes.</p>").into_response();
    }

    let mut html = String::from("<table class='striped'><thead><tr><th>IP Address</th><th>Last Seen</th></tr></thead><tbody>");
    for ip_record in ips {
        let time_ago = format_duration(chrono::Utc::now() - ip_record.last_seen_at);
        html.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{} ago</td></tr>",
            ip_record.client_ip, time_ago
        ));
    }
    html.push_str("</tbody></table>");

    Html(html).into_response()
}

fn format_duration(dur: chrono::Duration) -> String {
    let secs = dur.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn is_authenticated(jar: &CookieJar) -> bool {
    jar.get("admin_session").is_some()
}
