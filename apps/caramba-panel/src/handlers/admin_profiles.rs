use crate::AppState;
use crate::handlers::admin::{get_auth_user, is_authenticated};
use crate::singbox::ConfigGenerator;
use crate::singbox::RelayAuthMode;
use crate::singbox::policy::{ConfigPolicy, DnsMode, DnsPolicy};
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use caramba_db::models::config_profile::ConfigProfile;
use caramba_db::models::network::Inbound;
use caramba_db::models::node::Node;
use serde::Deserialize;
use tracing::error;

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Template, WebTemplate)]
#[template(path = "admin_profiles.html")]
pub struct AdminProfilesTemplate {
    pub profiles: Vec<ProfileRow>,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
}

pub struct ProfileRow {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub is_default: bool,
    pub dns_summary: String,
    pub log_summary: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "admin_profile_edit.html")]
pub struct AdminProfileEditTemplate {
    pub is_new: bool,
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub is_default: bool,
    pub log_level: String,
    pub dns_mode: String,
    pub dns_upstream: String,
    pub dns_server_port: String,
    pub dns_path: String,
    pub dns_strategy: String,
    pub dns_ru_direct: bool,
    pub is_auth: bool,
    pub admin_path: String,
    pub active_page: String,
    pub username: String,
}

// ---------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProfileForm {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_default: Option<String>,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub dns_mode: Option<String>,
    #[serde(default)]
    pub dns_upstream: Option<String>,
    #[serde(default)]
    pub dns_server_port: Option<String>,
    #[serde(default)]
    pub dns_path: Option<String>,
    #[serde(default)]
    pub dns_strategy: Option<String>,
    #[serde(default)]
    pub dns_ru_direct: Option<String>,
}

#[derive(Deserialize)]
pub struct GroupProfileForm {
    #[serde(default)]
    pub config_profile_id: Option<String>,
    #[serde(default)]
    pub config_priority: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn clean(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Build a typed `ConfigPolicy` from raw form fields.
fn build_policy(form: &ProfileForm) -> ConfigPolicy {
    let log_level = clean(form.log_level.as_deref());

    let dns_mode_raw = form
        .dns_mode
        .as_deref()
        .map(str::trim)
        .unwrap_or("none")
        .to_ascii_lowercase();

    let dns = match dns_mode_raw.as_str() {
        "system" | "doh" | "dot" => {
            let mode = match dns_mode_raw.as_str() {
                "doh" => DnsMode::Doh,
                "dot" => DnsMode::Dot,
                _ => DnsMode::System,
            };
            Some(DnsPolicy {
                mode,
                upstream: clean(form.dns_upstream.as_deref()).unwrap_or_default(),
                server_port: clean(form.dns_server_port.as_deref())
                    .and_then(|s| s.parse::<u16>().ok()),
                path: clean(form.dns_path.as_deref()),
                strategy: clean(form.dns_strategy.as_deref()),
                ru_direct: form.dns_ru_direct.is_some(),
            })
        }
        // "none" (or anything else) -> legacy hard-coded DNS, no policy override.
        _ => None,
    };

    ConfigPolicy { dns, log_level }
}

/// Short human-readable DNS summary for the list view.
fn dns_summary(policy: &ConfigPolicy) -> String {
    match &policy.dns {
        None => "Legacy (8.8.8.8)".to_string(),
        Some(d) => match d.mode {
            DnsMode::System => "System (local)".to_string(),
            DnsMode::Doh => format!("DoH {}", d.upstream),
            DnsMode::Dot => format!("DoT {}", d.upstream),
        },
    }
}

fn parse_policy(raw: &str) -> ConfigPolicy {
    serde_json::from_str::<ConfigPolicy>(raw).unwrap_or_default()
}

/// If this profile is being marked default, clear the flag on all others first
/// (the partial unique index allows only a single default row).
async fn clear_other_defaults(pool: &sqlx::PgPool, keep_id: Option<i64>) {
    let id = keep_id.unwrap_or(-1);
    let _ = sqlx::query("UPDATE config_profiles SET is_default = FALSE WHERE id <> $1")
        .bind(id)
        .execute(pool)
        .await;
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

pub async fn get_profiles_page(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let profiles: Vec<ConfigProfile> =
        sqlx::query_as::<_, ConfigProfile>("SELECT * FROM config_profiles ORDER BY name ASC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let rows: Vec<ProfileRow> = profiles
        .into_iter()
        .map(|p| {
            let policy = parse_policy(&p.policy);
            ProfileRow {
                id: p.id,
                name: p.name,
                slug: p.slug,
                description: p.description.unwrap_or_default(),
                is_default: p.is_default,
                dns_summary: dns_summary(&policy),
                log_summary: policy.log_level.unwrap_or_else(|| "info".to_string()),
            }
        })
        .collect();

    let template = AdminProfilesTemplate {
        profiles: rows,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "profiles".to_string(),
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

// ---------------------------------------------------------------------------
// New / Edit form
// ---------------------------------------------------------------------------

pub async fn get_profile_new(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let template = AdminProfileEditTemplate {
        is_new: true,
        id: 0,
        name: String::new(),
        slug: String::new(),
        description: String::new(),
        is_default: false,
        log_level: String::new(),
        dns_mode: "none".to_string(),
        dns_upstream: String::new(),
        dns_server_port: String::new(),
        dns_path: String::new(),
        dns_strategy: String::new(),
        dns_ru_direct: false,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "profiles".to_string(),
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

pub async fn get_profile_edit(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let profile = sqlx::query_as::<_, ConfigProfile>("SELECT * FROM config_profiles WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    let profile = match profile {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "Profile not found").into_response(),
    };

    let policy = parse_policy(&profile.policy);
    let (dns_mode, upstream, port, path, strategy, ru_direct) = match &policy.dns {
        None => (
            "none".to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            false,
        ),
        Some(d) => {
            let mode = match d.mode {
                DnsMode::System => "system",
                DnsMode::Doh => "doh",
                DnsMode::Dot => "dot",
            };
            (
                mode.to_string(),
                d.upstream.clone(),
                d.server_port.map(|p| p.to_string()).unwrap_or_default(),
                d.path.clone().unwrap_or_default(),
                d.strategy.clone().unwrap_or_default(),
                d.ru_direct,
            )
        }
    };

    let template = AdminProfileEditTemplate {
        is_new: false,
        id: profile.id,
        name: profile.name,
        slug: profile.slug,
        description: profile.description.unwrap_or_default(),
        is_default: profile.is_default,
        log_level: policy.log_level.unwrap_or_default(),
        dns_mode,
        dns_upstream: upstream,
        dns_server_port: port,
        dns_path: path,
        dns_strategy: strategy,
        dns_ru_direct: ru_direct,
        is_auth: true,
        admin_path: state.admin_path.clone(),
        active_page: "profiles".to_string(),
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
    };
    Html(template.render().unwrap_or_default()).into_response()
}

// ---------------------------------------------------------------------------
// Create / Update / Delete
// ---------------------------------------------------------------------------

pub async fn create_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<ProfileForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let policy = build_policy(&form);
    let policy_json = serde_json::to_string(&policy).unwrap_or_else(|_| "{}".to_string());
    let is_default = form.is_default.is_some();
    let description = clean(form.description.as_deref());

    if is_default {
        clear_other_defaults(&state.pool, None).await;
    }

    let res = sqlx::query(
        "INSERT INTO config_profiles (name, slug, description, policy, is_default) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&form.name)
    .bind(&form.slug)
    .bind(&description)
    .bind(&policy_json)
    .bind(is_default)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            axum::response::Redirect::to(&format!("{}/profiles", admin_path)).into_response()
        }
        Err(e) => {
            error!("Failed to create config profile: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create profile: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn update_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<ProfileForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let policy = build_policy(&form);
    let policy_json = serde_json::to_string(&policy).unwrap_or_else(|_| "{}".to_string());
    let is_default = form.is_default.is_some();
    let description = clean(form.description.as_deref());

    if is_default {
        clear_other_defaults(&state.pool, Some(id)).await;
    }

    let res = sqlx::query(
        "UPDATE config_profiles \
         SET name = $1, slug = $2, description = $3, policy = $4, is_default = $5, \
             updated_at = now() \
         WHERE id = $6",
    )
    .bind(&form.name)
    .bind(&form.slug)
    .bind(&description)
    .bind(&policy_json)
    .bind(is_default)
    .bind(id)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => {
            let admin_path = state.admin_path.clone();
            axum::response::Redirect::to(&format!("{}/profiles", admin_path)).into_response()
        }
        Err(e) => {
            error!("Failed to update config profile {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update profile: {}", e),
            )
                .into_response()
        }
    }
}

pub async fn delete_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    // FK columns use ON DELETE SET NULL, so assignments fall back to default/legacy.
    let _ = sqlx::query("DELETE FROM config_profiles WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

    StatusCode::OK.into_response()
}

// ---------------------------------------------------------------------------
// Validate (canary): render the policy on a real node and run `sing-box check`
// ---------------------------------------------------------------------------

pub async fn validate_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<ProfileForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let policy = build_policy(&form);

    let nodes = state
        .infrastructure_service
        .get_all_nodes()
        .await
        .unwrap_or_default();

    let node: Node = match nodes.into_iter().next() {
        Some(n) => n,
        None => {
            return Html(validate_fragment(
                false,
                "No nodes available to validate against. Add a node first.",
            ))
            .into_response();
        }
    };

    let relay_auth_mode_raw = state.settings.get_or_default("relay_auth_mode", "dual").await;
    let relay_auth_mode = RelayAuthMode::from_setting(Some(relay_auth_mode_raw.as_str()));

    let config = ConfigGenerator::generate_config_with_policy(
        &node,
        Vec::<Inbound>::new(),
        None,
        None,
        vec![],
        relay_auth_mode,
        &policy,
    );

    match ConfigGenerator::validate_config(&config) {
        Ok(()) => Html(validate_fragment(
            true,
            "Config is valid (sing-box check passed).",
        ))
        .into_response(),
        Err(e) => {
            Html(validate_fragment(false, &format!("Validation failed: {}", e))).into_response()
        }
    }
}

fn validate_fragment(ok: bool, msg: &str) -> String {
    let (icon, classes) = if ok {
        (
            "check-circle",
            "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20",
        )
    } else {
        (
            "alert-triangle",
            "bg-rose-500/10 text-rose-600 dark:text-rose-400 border-rose-500/20",
        )
    };
    let safe = msg
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        "<div class=\"flex items-center gap-2 px-4 py-3 rounded-xl border text-sm font-medium {classes}\">\
            <i data-lucide=\"{icon}\" class=\"w-4 h-4 shrink-0\"></i><span>{safe}</span>\
         </div>\
         <script>lucide.createIcons();</script>"
    )
}

// ---------------------------------------------------------------------------
// Group assignment
// ---------------------------------------------------------------------------

pub async fn assign_group_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(group_id): Path<i64>,
    Form(form): Form<GroupProfileForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let profile_id: Option<i64> = form
        .config_profile_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "0")
        .and_then(|s| s.parse::<i64>().ok());

    let priority: i32 = form
        .config_priority
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(100);

    let _ = sqlx::query(
        "UPDATE node_groups SET config_profile_id = $1, config_priority = $2 WHERE id = $3",
    )
    .bind(profile_id)
    .bind(priority)
    .bind(group_id)
    .execute(&state.pool)
    .await;

    let admin_path = state.admin_path.clone();
    axum::response::Redirect::to(&format!("{}/groups/{}", admin_path, group_id)).into_response()
}
