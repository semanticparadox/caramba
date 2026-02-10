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
use crate::models::groups::{InboundTemplate, NodeGroup};
use tracing::{info, error};

#[derive(Template, WebTemplate)]
#[template(path = "admin_templates.html")]
pub struct AdminTemplatesTemplate {
    pub templates: Vec<TemplateWithGroup>,
    pub groups: Vec<NodeGroup>, // For dropdowns
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
    // Placeholders for template (literal strings)
    pub uuid: String,
    pub email: String,
    pub reality_private: String,
    pub sni: String,
}

pub struct TemplateWithGroup {
    pub tpl: InboundTemplate,
    pub group_name: Option<String>,
}

pub async fn get_templates_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let templates = sqlx::query_as::<_, InboundTemplate>("SELECT * FROM inbound_templates ORDER BY name ASC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        
    let groups = sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY name ASC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut tpls_with_group = Vec::new();
    for t in templates {
        let group_name = if let Some(gid) = t.target_group_id {
            groups.iter().find(|g| g.id == gid).map(|g| g.name.clone())
        } else {
            None
        };
        tpls_with_group.push(TemplateWithGroup { tpl: t, group_name });
    }

    let template = AdminTemplatesTemplate {
        templates: tpls_with_group,
        groups,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "templates".to_string(),
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        uuid: "{{uuid}}".to_string(),
        email: "{{email}}".to_string(),
        reality_private: "{{reality_private}}".to_string(),
        sni: "{{sni}}".to_string(),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

#[derive(Deserialize)]
pub struct CreateTemplateForm {
    pub name: String,
    pub protocol: String, // vless, vmess, etc.
    pub target_group_id: Option<i64>,
    pub settings_template: String,
    pub stream_settings_template: String,
    pub port_range_start: i64,
    pub port_range_end: i64,
}

pub async fn create_template(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CreateTemplateForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    // Basic validation of JSON & Inject Protocol if missing
    let mut settings_json: serde_json::Value = match serde_json::from_str(&form.settings_template) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("Invalid Settings JSON: {}", e)).into_response(),
    };

    if let Some(obj) = settings_json.as_object_mut() {
        if !obj.contains_key("protocol") {
            obj.insert("protocol".to_string(), serde_json::Value::String(form.protocol.clone()));
        }
    }
    let final_settings = settings_json.to_string();

    if let Err(e) = serde_json::from_str::<serde_json::Value>(&form.stream_settings_template) {
         return (StatusCode::BAD_REQUEST, format!("Invalid Stream Settings JSON: {}", e)).into_response();
    }

    let res = sqlx::query(
        r#"
        INSERT INTO inbound_templates 
        (name, protocol, target_group_id, settings_template, stream_settings_template, port_range_start, port_range_end) 
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#
    )
        .bind(&form.name)
        .bind(&form.protocol)
        .bind(form.target_group_id)
        .bind(&final_settings)
        .bind(&form.stream_settings_template)
        .bind(form.port_range_start)
        .bind(form.port_range_end)
        .execute(&state.pool)
        .await;

    match res {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            axum::response::Redirect::to(&format!("{}/templates", admin_path)).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create template: {}", e)).into_response(),
    }
}

pub async fn delete_template(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    let _ = sqlx::query("DELETE FROM inbound_templates WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    let admin_path = state.admin_path.clone();
    axum::response::Redirect::to(&format!("{}/templates", admin_path)).into_response()
}

pub async fn sync_template(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    // Get group ID from template
    let group_id: Option<i64> = sqlx::query_scalar("SELECT target_group_id FROM inbound_templates WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);
        
    if let Some(gid) = group_id {
        match state.generator_service.sync_group_inbounds(gid).await {
            Ok(_) => {
                info!("Synced inbounds for group {}", gid);
                let admin_path = state.admin_path.clone();
                // Redirect back with success message? For now just redirect.
                 ([("HX-Redirect", format!("{}/templates", admin_path))], "Synced").into_response()
            },
            Err(e) => {
                error!("Sync failed: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Sync failed: {}", e)).into_response()
            }
        }
    } else {
        (StatusCode::BAD_REQUEST, "Template has no target group").into_response()
    }
}
