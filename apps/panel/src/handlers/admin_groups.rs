use axum::{
    extract::{State, Path, Form},
    response::{IntoResponse, Html},
};
use axum_extra::extract::cookie::CookieJar;
use crate::handlers::admin::{get_auth_user, is_authenticated};
use askama::Template;
use askama_web::WebTemplate;
use serde::Deserialize;
use crate::AppState;
use crate::models::groups::{NodeGroup, InboundTemplate};
use crate::models::node::Node;
use tracing::error;

#[derive(Template, WebTemplate)]
#[template(path = "admin_groups.html")]
pub struct AdminGroupsTemplate {
    pub groups: Vec<GroupWithCount>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
}

pub struct GroupWithCount {
    pub group: NodeGroup,
    pub node_count: i64,
}

pub async fn get_groups_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let groups = sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY name ASC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut groups_with_count = Vec::new();
    for g in groups {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM node_group_members WHERE group_id = ?")
            .bind(g.id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
        groups_with_count.push(GroupWithCount { group: g, node_count: count });
    }

    let template = AdminGroupsTemplate {
        groups: groups_with_count,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "groups".to_string(),
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

#[derive(Deserialize)]
pub struct CreateGroupForm {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
}

pub async fn create_group(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CreateGroupForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let res = sqlx::query("INSERT INTO node_groups (name, slug, description) VALUES (?, ?, ?)")
        .bind(&form.name)
        .bind(&form.slug)
        .bind(&form.description)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            axum::response::Redirect::to(&format!("{}/groups", admin_path)).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create group: {}", e)).into_response(),
    }
}

pub async fn delete_group(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    // Safety: Don't delete Default group (id=1)
    if id == 1 {
         return (StatusCode::BAD_REQUEST, "Cannot delete default group").into_response();
    }

    let _ = sqlx::query("DELETE FROM node_groups WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    let admin_path = state.admin_path.clone();
    axum::response::Redirect::to(&format!("{}/groups", admin_path)).into_response()
}

// --- Edit Group ---

#[derive(Template, WebTemplate)]
#[template(path = "admin_group_edit.html")]
pub struct AdminGroupEditTemplate {
    pub group: NodeGroup,
    pub members: Vec<Node>,
    pub available_nodes: Vec<Node>,
    pub inbounds: Vec<InboundTemplate>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
}

pub async fn get_group_edit(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let group = match sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await {
            Ok(Some(g)) => g,
            Ok(None) => return (StatusCode::NOT_FOUND, "Group not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response(),
        };

    // Get current members
    let members = sqlx::query_as::<_, Node>(
        "SELECT n.* FROM nodes n JOIN node_group_members ngm ON n.id = ngm.node_id WHERE ngm.group_id = ?"
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    // Get available nodes (not in this group)
    let available_nodes = sqlx::query_as::<_, Node>(
        "SELECT * FROM nodes WHERE id NOT IN (SELECT node_id FROM node_group_members WHERE group_id = ?)"
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await;

    let available_nodes = match available_nodes {
        Ok(nodes) => nodes,
        Err(e) => {
            error!("Failed to fetch available nodes for group {}: {}", id, e);
            Vec::new()
        }
    };

    // Get inbounds for this group
    let inbounds = sqlx::query_as::<_, InboundTemplate>("SELECT * FROM inbound_templates WHERE target_group_id = ?")
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let template = AdminGroupEditTemplate {
        group,
        members,
        available_nodes,
        inbounds,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "groups".to_string(),
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

pub async fn add_group_member(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(group_id): Path<i64>,
    Form(form): Form<MemberForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    let _ = sqlx::query("INSERT INTO node_group_members (group_id, node_id) VALUES (?, ?)")
        .bind(group_id)
        .bind(form.node_id)
        .execute(&state.pool)
        .await;

    // Trigger sync to ensure new member gets templates
    if let Err(e) = state.generator_service.sync_group_inbounds(group_id).await {
        error!("Failed to sync group inbounds: {}", e);
    }
        
    let admin_path = state.admin_path.clone();
    axum::response::Redirect::to(&format!("{}/groups/{}", admin_path, group_id)).into_response()
}

pub async fn remove_group_member(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((group_id, node_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    let _ = sqlx::query("DELETE FROM node_group_members WHERE group_id = ? AND node_id = ?")
        .bind(group_id)
        .bind(node_id)
        .execute(&state.pool)
        .await;

    let admin_path = state.admin_path.clone();
    axum::response::Redirect::to(&format!("{}/groups/{}", admin_path, group_id)).into_response()
}

#[derive(Deserialize)]
pub struct MemberForm {
    pub node_id: i64,
}

pub async fn rotate_group_inbounds(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(group_id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    match state.generator_service.rotate_group_inbounds(group_id).await {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            axum::response::Redirect::to(&format!("{}/groups/{}", admin_path, group_id)).into_response()
        },
        Err(e) => {
            error!("Failed to rotate inbounds: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to rotate inbounds: {}", e)).into_response()
        }
    }
}
