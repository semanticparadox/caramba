//! Public, unauthenticated status page at `/status`. Human-readable HTML
//! summary of system health for end users / status-page consumers.
//!
//! This is intentionally separate from `/api/health` (JSON, for uptime
//! monitors / k8s probes). No PII, no admin data — just per-region node
//! health and overall service indicator.

use askama::Template;
use askama_web::WebTemplate;
use axum::{extract::State, response::Html};
use chrono::{DateTime, Utc};

use crate::AppState;

#[derive(Template, WebTemplate)]
#[template(path = "status.html")]
pub struct StatusTemplate {
    pub overall_status: &'static str, // "operational" | "degraded" | "outage"
    pub overall_label: &'static str,  // human-readable label
    pub overall_color: &'static str,  // tailwind color token
    pub nodes_total: i64,
    pub nodes_active: i64,
    pub nodes_offline: i64,
    pub frontends_active: i64,
    pub regions: Vec<RegionStatus>,
    pub last_checked: String,
    pub brand_name: String,
}

pub struct RegionStatus {
    pub country_code: String,
    pub country_flag: String,
    pub active: i64,
    pub total: i64,
    pub status_color: &'static str, // green | amber | red
}

pub async fn status_page(State(state): State<AppState>) -> Html<String> {
    // 1. Aggregate node counts
    let (nodes_total, nodes_active, nodes_offline) = {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE is_enabled = TRUE")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM nodes WHERE is_enabled = TRUE AND status = 'active'",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        let offline: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM nodes WHERE is_enabled = TRUE AND status = 'offline'",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        (total, active, offline)
    };

    let frontends_active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM frontend_servers WHERE status != 'offline'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    // 2. Per-region (country) breakdown
    #[derive(sqlx::FromRow)]
    struct RegionRow {
        country_code: Option<String>,
        flag: Option<String>,
        total: i64,
        active: i64,
    }
    let region_rows = sqlx::query_as::<_, RegionRow>(
        r#"SELECT
            country_code,
            MAX(flag) AS flag,
            COUNT(*)::BIGINT AS total,
            COUNT(*) FILTER (WHERE status = 'active')::BIGINT AS active
           FROM nodes
           WHERE is_enabled = TRUE AND country_code IS NOT NULL
           GROUP BY country_code
           ORDER BY country_code"#,
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let regions: Vec<RegionStatus> = region_rows
        .into_iter()
        .map(|r| {
            let cc = r.country_code.unwrap_or_else(|| "??".to_string());
            let flag = r.flag.unwrap_or_else(|| "🌐".to_string());
            let color = if r.active == r.total {
                "green"
            } else if r.active == 0 {
                "red"
            } else {
                "amber"
            };
            RegionStatus {
                country_code: cc,
                country_flag: flag,
                active: r.active,
                total: r.total,
                status_color: color,
            }
        })
        .collect();

    // 3. Overall service indicator
    let (overall_status, overall_label, overall_color) = if nodes_active == 0 {
        ("outage", "Service unavailable", "red")
    } else if nodes_offline > 0 || frontends_active == 0 {
        ("degraded", "Partial outage", "amber")
    } else {
        ("operational", "All systems operational", "green")
    };

    let brand_name = state
        .settings
        .get_or_default("brand_name", "Caramba VPN")
        .await;

    let now: DateTime<Utc> = Utc::now();
    let template = StatusTemplate {
        overall_status,
        overall_label,
        overall_color,
        nodes_total,
        nodes_active,
        nodes_offline,
        frontends_active,
        regions,
        last_checked: now.format("%Y-%m-%d %H:%M UTC").to_string(),
        brand_name,
    };

    Html(template.render().unwrap_or_else(|e| {
        tracing::error!("status_page render error: {}", e);
        "<h1>Status page error</h1>".to_string()
    }))
}
