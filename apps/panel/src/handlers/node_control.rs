use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use tracing::{info, error};
use crate::AppState;
use crate::models::node::Node;

/// Test node SSH connection
pub async fn test_node_connection(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Testing connection to node {}", node_id);
    
    // Fetch node
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
        Err(e) => {
            error!("DB error: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    // Test SSH connection
    let test_cmd = "echo 'ping'";
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    
    match crate::ssh::execute_remote_script(
        &node.ip,
        &node.ssh_user,
        node.ssh_port,
        &node.ssh_password,
        test_cmd,
        tx
    ).await {
        Ok(_) => {
            // Collect output
            let mut output = String::new();
            while let Some(line) = rx.recv().await {
                output.push_str(&line);
            }
            
            // Update last_seen
            let _ = sqlx::query("UPDATE nodes SET last_seen = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(node_id)
                .execute(&state.pool)
                .await;
            
            info!("✅ Node {} connection test successful", node_id);
            (axum::http::StatusCode::OK, "Connection successful ✅").into_response()
        },
        Err(e) => {
            error!("❌ Node {} connection failed: {}", node_id, e);
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, 
             format!("Connection failed: {}", e)).into_response()
        }
    }
}

/// Restart node service
pub async fn restart_node_service(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Restarting service on node {}", node_id);
    
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
        Err(e) => {
            error!("DB error: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    let restart_cmd = "systemctl restart exarobotnode && sleep 2 && systemctl is-active exarobotnode";
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    
    match crate::ssh::execute_remote_script(
        &node.ip,
        &node.ssh_user,
        node.ssh_port,
        &node.ssh_password,
        restart_cmd,
        tx
    ).await {
        Ok(_) => {
            let mut output = String::new();
            while let Some(line) = rx.recv().await {
                output.push_str(&line);
            }
            
            let _ = crate::services::activity_service::ActivityService::log(
                &state.pool,
                "NodeControl",
                &format!("Service restarted on node {} ({})", node_id, node.name)
            ).await;
            
            info!("✅ Node {} service restarted", node_id);
            (axum::http::StatusCode::OK, format!("Service restarted. Status: {}", output.trim())).into_response()
        },
        Err(e) => {
            error!("❌ Failed to restart node {}: {}", node_id, e);
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, 
             format!("Restart failed: {}", e)).into_response()
        }
    }
}

/// Pull logs from node
pub async fn pull_node_logs(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Pulling logs from node {}", node_id);
    
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Node not found").into_response(),
        Err(e) => {
            error!("DB error: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    // Try new path first, fallback to journalctl
    let logs_cmd = r#"
if [ -f /opt/exarobotnode/logs/sing-box.log ]; then
    tail -100 /opt/exarobotnode/logs/sing-box.log
else
    journalctl -u sing-box -n 100 --no-pager
fi
"#;
    
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    
    match crate::ssh::execute_remote_script(
        &node.ip,
        &node.ssh_user,
        node.ssh_port,
        &node.ssh_password,
        logs_cmd,
        tx
    ).await {
        Ok(_) => {
            let mut logs = String::new();
            while let Some(line) = rx.recv().await {
                logs.push_str(&line);
                logs.push('\n');
            }
            
            info!("✅ Pulled logs from node {} ({} bytes)", node_id, logs.len());
            
            // Return as plain text
            ([(axum::http::header::CONTENT_TYPE, "text/plain")], logs).into_response()
        },
        Err(e) => {
            error!("❌ Failed to pull logs from node {}: {}", node_id, e);
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, 
             format!("Failed to pull logs: {}", e)).into_response()
        }
    }
}

/// Get node health status
#[derive(Serialize)]
pub struct NodeHealthStatus {
    pub node_id: i64,
    pub node_name: String,
    pub ssh_reachable: bool,
    pub service_running: bool,
    pub last_check: String,
}

pub async fn get_node_health(
    Path(node_id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let node: Node = match sqlx::query_as("SELECT * FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Node not found"
        }))).into_response(),
        Err(e) => {
            error!("DB error: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "Database error"
            }))).into_response();
        }
    };

    // Test SSH + service status
    let health_cmd = r#"
if systemctl is-active --quiet exarobotnode; then
    echo "service_running:true"
else
    echo "service_running:false"
fi
"#;
    
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let ssh_reachable = crate::ssh::execute_remote_script(
        &node.ip,
        &node.ssh_user,
        node.ssh_port,
        &node.ssh_password,
        health_cmd,
        tx
    ).await.is_ok();
    
    let mut service_running = false;
    if ssh_reachable {
        while let Some(line) = rx.recv().await {
            if line.contains("service_running:true") {
                service_running = true;
            }
        }
    }
    
    let health = NodeHealthStatus {
        node_id,
        node_name: node.name.clone(),
        ssh_reachable,
        service_running,
        last_check: chrono::Utc::now().to_rfc3339(),
    };
    
    (axum::http::StatusCode::OK, Json(health)).into_response()
}
