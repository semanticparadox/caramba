// Config Builder — single landing page for two related admin sections that
// were previously separate top-level entries in the Servers sub-nav:
//   1. Config Profiles (DNS, log level, RU-direct policy per group/node)
//   2. Inbound Templates (reusable inbound configs synced across groups)
//
// Both sections are listed side-by-side with deep links to their existing
// edit pages (`/profiles/{id}` and `/templates/{id}`). Edit/create flows stay
// on the original URLs so existing bookmarks keep working — only the sub-nav
// entry moves here in 0.9.50.

use crate::AppState;
use crate::handlers::admin::{get_auth_user, is_authenticated};
use crate::handlers::admin_templates::TemplateWithGroup;
use crate::singbox::policy::ConfigPolicy;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use caramba_db::models::config_profile::ConfigProfile;
use caramba_db::models::groups::{InboundTemplate, NodeGroup};

#[derive(Template, WebTemplate)]
#[template(path = "admin_config_builder.html")]
pub struct AdminConfigBuilderTemplate {
    pub profiles: Vec<ProfileSummary>,
    pub templates: Vec<TemplateWithGroup>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
}

/// Slim row for the Config Profiles table — same fields `admin_profiles.rs`
/// exposes, recomputed here so the unified page doesn't need to depend on the
/// separate `ProfileRow` struct (kept private to that file).
pub struct ProfileSummary {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub is_default: bool,
    pub dns_summary: String,
    pub log_summary: String,
}

fn parse_policy(raw: &str) -> ConfigPolicy {
    serde_json::from_str::<ConfigPolicy>(raw).unwrap_or_default()
}

fn dns_summary(policy: &ConfigPolicy) -> String {
    use crate::singbox::policy::DnsMode;
    match &policy.dns {
        None => "Legacy (8.8.8.8)".to_string(),
        Some(d) => match d.mode {
            DnsMode::System => "System (local)".to_string(),
            DnsMode::Doh => format!("DoH {}", d.upstream),
            DnsMode::Dot => format!("DoT {}", d.upstream),
        },
    }
}

pub async fn get_config_builder_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    // --- Profiles ---
    let profiles: Vec<ConfigProfile> =
        sqlx::query_as::<_, ConfigProfile>("SELECT * FROM config_profiles ORDER BY name ASC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let profile_rows: Vec<ProfileSummary> = profiles
        .into_iter()
        .map(|p| {
            let policy = parse_policy(&p.policy);
            ProfileSummary {
                id: p.id,
                name: p.name,
                slug: p.slug,
                is_default: p.is_default,
                dns_summary: dns_summary(&policy),
                log_summary: policy.log_level.unwrap_or_else(|| "info".to_string()),
            }
        })
        .collect();

    // --- Inbound Templates ---
    let templates: Vec<InboundTemplate> = state
        .infrastructure_service
        .node_repo
        .get_all_inbound_templates()
        .await
        .unwrap_or_default();

    let groups: Vec<NodeGroup> =
        sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY name ASC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let templates_with_group: Vec<TemplateWithGroup> = templates
        .into_iter()
        .map(|t| {
            let group_name = t
                .target_group_id
                .and_then(|gid| groups.iter().find(|g| g.id == gid).map(|g| g.name.clone()));
            TemplateWithGroup { tpl: t, group_name }
        })
        .collect();

    let template = AdminConfigBuilderTemplate {
        profiles: profile_rows,
        templates: templates_with_group,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "config_builder".to_string(),
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
    };

    Html(template.render().unwrap_or_default()).into_response()
}
