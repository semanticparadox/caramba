// Plans Module
// Plan management and configuration

use axum::{
    extract::{State, Form, Path},
    http::HeaderMap,
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;

use tracing::{info, error};

use crate::AppState;
use crate::models::store::Plan;
use crate::models::node::Node;
use super::auth::get_auth_user;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "plans.html")]
pub struct PlansTemplate {
    pub plans: Vec<Plan>,
    pub nodes: Vec<Node>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "plan_edit_modal.html")]
struct PlanEditModalTemplate {
    plan: Plan,
    nodes: Vec<(Node, bool)>,
    admin_path: String,
}

// ============================================================================
// Route Handlers  
// ============================================================================

pub async fn get_plans(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let mut plans = match sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_trial = 0")
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

    let template = PlansTemplate { 
        plans, 
        nodes,
        is_auth: true, 
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(), 
        active_page: "plans".to_string() 
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn add_plan(
    State(state): State<AppState>,
    Form(raw_form): Form<Vec<(String, String)>>,
) -> impl IntoResponse {
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

    info!("Adding flexible plan: {}", name);
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

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
    
    let _ = crate::services::activity_service::ActivityService::log(&state.pool, "Plan", &format!("New plan created: {}", name)).await;

    let admin_path = state.admin_path.clone();
    
    let mut headers = HeaderMap::new();
    headers.insert("HX-Redirect", format!("{}/plans", admin_path).parse().unwrap());
    (axum::http::StatusCode::OK, headers, "Plan Created").into_response()
}

pub async fn delete_plan(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete plan: {}", id);
    
    let is_trial: bool = match sqlx::query_scalar("SELECT is_trial FROM plans WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await {
            Ok(v) => v,
            Err(_) => return (axum::http::StatusCode::NOT_FOUND, "Plan not found").into_response(),
        };

    if is_trial {
        return (axum::http::StatusCode::BAD_REQUEST, "Cannot delete system trial plan. Disable it instead.").into_response();
    }

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
    let plan = match sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE id = ?").bind(id).fetch_optional(&state.pool).await {
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

    let all_nodes = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE is_enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let linked_node_ids: Vec<i64> = sqlx::query_scalar("SELECT node_id FROM plan_nodes WHERE plan_id = ?")
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let admin_path = state.admin_path.clone();
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    let nodes_with_status: Vec<(Node, bool)> = all_nodes.into_iter().map(|n| {
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

    if let Err(e) = sqlx::query("DELETE FROM plan_durations WHERE plan_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await {
            error!("Failed to clear durations: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to clear durations").into_response();
        }

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

    let admin_path = state.admin_path.clone();
    ([(("HX-Redirect", format!("{}/plans", admin_path)))], "Redirecting...").into_response()
}
