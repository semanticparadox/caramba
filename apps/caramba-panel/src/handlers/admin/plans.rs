// Plans Module
// Plan management and configuration

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;

use tracing::{error, info};

use crate::AppState;
use caramba_db::models::store::Plan;
// use caramba_db::models::node::Node; // Removed

use super::auth::get_auth_user;
use caramba_db::models::groups::NodeGroup;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "plans.html")]
pub struct PlansTemplate {
    pub plans: Vec<Plan>,
    pub groups: Vec<NodeGroup>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "plan_edit_modal.html")]
struct PlanEditModalTemplate {
    plan: Plan,
    groups: Vec<(NodeGroup, bool)>,
    admin_path: String,
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_plans(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let plans = match state.catalog_service.get_plans_admin().await {
        Ok(p) => {
            info!("Successfully fetched {} plans from DB", p.len());
            p
        }
        Err(e) => {
            error!("Failed to fetch plans from DB (Admin): {}", e);
            Vec::new()
        }
    };

    // Durations are already fetched by get_plans_admin
    // for plan in &mut plans {
    //     let durations = ...
    // }

    // let nodes = state.orchestration_service.get_all_nodes().await.unwrap_or_default();
    let groups = state
        .orchestration_service
        .node_repo
        .get_all_groups()
        .await
        .unwrap_or_default();

    let template = PlansTemplate {
        plans,
        groups,
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(),
        active_page: "plans".to_string(),
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
    let mut group_ids: Vec<i64> = Vec::new();

    for (key, value) in raw_form {
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            "device_limit" => {
                if let Ok(v) = value.parse() {
                    device_limit = v;
                }
            }
            "duration_days" | "duration_days[]" => {
                if let Ok(v) = value.parse() {
                    duration_days.push(v);
                }
            }
            "price" | "price[]" => {
                if let Ok(v) = value.parse() {
                    price.push(v);
                }
            }
            "traffic_limit_gb" => {
                if let Ok(v) = value.parse() {
                    traffic_limit_gb = v;
                }
            }
            "group_ids" => {
                if let Ok(v) = value.parse() {
                    group_ids.push(v);
                }
            }
            _ => {}
        }
    }

    info!("Adding flexible plan: {}", name);
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }
    if duration_days.is_empty() || price.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "At least one duration/price pair is required",
        )
            .into_response();
    }

    match state
        .catalog_service
        .create_plan(
            &name,
            &description,
            device_limit,
            traffic_limit_gb,
            duration_days,
            price,
            group_ids,
        )
        .await
    {
        Ok(id) => info!("Created plan with ID: {}", id),
        Err(e) => {
            error!("Failed to create plan: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create plan",
            )
                .into_response();
        }
    }

    let _ = crate::services::activity_service::ActivityService::log(
        &state.pool,
        "Plan",
        &format!("New plan created: {}", name),
    )
    .await;

    let admin_path = state.admin_path.clone();

    let mut headers = HeaderMap::new();
    headers.insert(
        "HX-Redirect",
        format!("{}/plans", admin_path).parse().unwrap(),
    );
    (axum::http::StatusCode::OK, headers, "Plan Created").into_response()
}

pub async fn delete_plan(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    info!("Request to delete plan: {}", id);

    let is_trial = state
        .catalog_service
        .is_trial_plan(id)
        .await
        .unwrap_or(false);

    if is_trial {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Cannot delete system trial plan. Disable it instead.",
        )
            .into_response();
    }

    match state.catalog_service.delete_plan_and_refund(id).await {
        Ok((refunded_users, total_refunded_cents)) => {
            info!(
                "Plan {} deleted. Refunded {} users (Total: ${:.2})",
                id,
                refunded_users,
                total_refunded_cents as f64 / 100.0
            );
            (axum::http::StatusCode::OK, "").into_response()
        }
        Err(e) => {
            error!("Failed to delete plan {}: {}", id, e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete plan: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn get_plan_edit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let plan = match state.catalog_service.get_plan_by_id(id).await {
        Ok(Some(p)) => p,
        _ => return (axum::http::StatusCode::NOT_FOUND, "Plan not found").into_response(),
    };

    let all_groups = state
        .orchestration_service
        .node_repo
        .get_all_groups()
        .await
        .unwrap_or_default();

    let linked_group_ids = state
        .catalog_service
        .get_plan_group_ids(id)
        .await
        .unwrap_or_default();

    let admin_path = state.admin_path.clone();
    let admin_path = if admin_path.starts_with('/') {
        admin_path
    } else {
        format!("/{}", admin_path)
    };

    let groups_with_status: Vec<(NodeGroup, bool)> = all_groups
        .into_iter()
        .map(|g| {
            let is_linked = linked_group_ids.contains(&g.id);
            (g, is_linked)
        })
        .collect();

    Html(
        PlanEditModalTemplate {
            plan,
            groups: groups_with_status,
            admin_path,
        }
        .render()
        .unwrap_or_default(),
    )
    .into_response()
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
    let mut group_ids: Vec<i64> = Vec::new();

    for (key, value) in raw_form {
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            "device_limit" => {
                if let Ok(v) = value.parse() {
                    device_limit = v;
                }
            }
            "duration_days" | "duration_days[]" => {
                if let Ok(v) = value.parse() {
                    duration_days.push(v);
                }
            }
            "price" | "price[]" => {
                if let Ok(v) = value.parse() {
                    price.push(v);
                }
            }
            "traffic_limit_gb" => {
                if let Ok(v) = value.parse() {
                    traffic_limit_gb = v;
                }
            }
            "group_ids" => {
                if let Ok(v) = value.parse() {
                    group_ids.push(v);
                }
            }
            _ => {}
        }
    }

    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Plan name is required").into_response();
    }
    if duration_days.is_empty() || price.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "At least one duration/price pair is required",
        )
            .into_response();
    }

    if let Err(e) = state
        .catalog_service
        .update_plan(
            id,
            &name,
            &description,
            device_limit,
            traffic_limit_gb,
            duration_days,
            price,
            group_ids,
        )
        .await
    {
        error!("Failed to update plan: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update plan",
        )
            .into_response();
    }

    let _ = crate::services::activity_service::ActivityService::log(
        &state.pool,
        "Plan",
        &format!("Plan {} updated: {}", id, name),
    )
    .await;

    let admin_path = state.admin_path.clone();
    (
        [(("HX-Redirect", format!("{}/plans", admin_path)))],
        "Redirecting...",
    )
        .into_response()
}
