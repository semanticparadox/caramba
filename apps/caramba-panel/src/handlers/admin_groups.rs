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
use caramba_db::models::groups::{NodeGroup, InboundTemplate};
use caramba_db::models::node::Node;
use caramba_db::repositories::node_repo::NodeRepository;
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

    let groups: Vec<NodeGroup> = sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY name ASC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut groups_with_count = Vec::new();
    for g in groups {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM node_group_members WHERE group_id = $1")
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

    let res = sqlx::query("INSERT INTO node_groups (name, slug, description) VALUES ($1, $2, $3)")
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

    let _ = sqlx::query("DELETE FROM node_groups WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

    axum::http::StatusCode::OK.into_response()
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

    let group = match sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await {
            Ok(Some(g)) => g,
            Ok(None) => return (StatusCode::NOT_FOUND, "Group not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response(),
        };

    let node_repo = NodeRepository::new(state.pool.clone());

    // Get current members via repository parser (handles INT4/INT8 drift).
    let member_ids = node_repo.get_group_nodes(id).await.unwrap_or_default();
    let mut members: Vec<Node> = Vec::new();
    for node_id in &member_ids {
        if let Ok(Some(node)) = node_repo.get_node_by_id(*node_id).await {
            members.push(node);
        }
    }

    // Get available nodes (not in this group).
    let available_nodes = match node_repo.get_all_nodes().await {
        Ok(nodes) => {
            let member_set: std::collections::HashSet<i64> = member_ids.into_iter().collect();
            nodes
                .into_iter()
                .filter(|n| !member_set.contains(&n.id))
                .collect()
        }
        Err(e) => {
            error!("Failed to fetch available nodes for group {}: {}", id, e);
            Vec::new()
        }
    };

    // Get templates for this group with explicit casts for compatibility.
    let inbounds: Vec<InboundTemplate> = sqlx::query_as::<_, InboundTemplate>(
        r#"
        SELECT
            id,
            name,
            protocol,
            settings_template,
            stream_settings_template,
            target_group_id,
            COALESCE(port_range_start, 10000)::BIGINT AS port_range_start,
            COALESCE(port_range_end, 60000)::BIGINT AS port_range_end,
            0::BIGINT AS renew_interval_hours,
            COALESCE(renew_interval_mins, 0)::BIGINT AS renew_interval_mins,
            COALESCE(is_active, TRUE) AS is_active,
            created_at
        FROM inbound_templates
        WHERE target_group_id = $1
        ORDER BY name ASC
        "#,
    )
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
    
    let _ = sqlx::query("INSERT INTO node_group_members (group_id, node_id) VALUES ($1, $2) ON CONFLICT (group_id, node_id) DO NOTHING")
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
    
    let _ = sqlx::query("DELETE FROM node_group_members WHERE group_id = $1 AND node_id = $2")
        .bind(group_id)
        .bind(node_id)
        .execute(&state.pool)
        .await;

    axum::http::StatusCode::OK.into_response()
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
