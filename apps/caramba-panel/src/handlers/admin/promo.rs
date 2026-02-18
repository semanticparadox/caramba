use axum::{
    extract::{State, Form, Path},
    response::{IntoResponse, Html},
};

use askama::Template;
use serde::Deserialize;
use tracing::error;
use crate::AppState;
use caramba_db::models::promo::PromoCode;

#[derive(Debug, Clone)]
struct PromoPlanOption {
    id: i64,
    name: String,
}

#[derive(Template)]
#[template(path = "promo_manage.html")]
struct PromoManageTemplate {
    admin_path: String,
    promos: Vec<PromoCode>,
    plans: Vec<PromoPlanOption>,
    is_auth: bool,
    username: String,
    active_page: String,
}

pub async fn get_promos(
    State(state): State<AppState>,
    jar: axum_extra::extract::cookie::CookieJar,
) -> impl IntoResponse {
    // Check Auth
    let username = match crate::handlers::admin::auth::get_auth_user(&state, &jar).await {
        Some(u) => u,
        None => return axum::response::Redirect::to(&format!("{}/login", state.admin_path)).into_response(),
    };
    
    let promos = state.promo_service.list_promos().await.unwrap_or_default();
    let plans = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, name FROM plans WHERE is_active = TRUE ORDER BY sort_order ASC, name ASC"
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, name)| PromoPlanOption { id, name })
    .collect::<Vec<_>>();

    Html(PromoManageTemplate {
        admin_path: state.admin_path.clone(),
        promos,
        plans,
        is_auth: true,
        username,
        active_page: "promo".to_string(),
    }.render().unwrap()).into_response()
}

#[derive(Deserialize)]
pub struct AddPromoForm {
    pub code: String,
    pub promo_type: String,
    pub plan_id: Option<i64>,
    pub balance_amount: Option<i32>,
    pub duration_days: Option<i32>,
    pub traffic_gb: Option<i32>,
    pub max_uses: i32,
    pub expires_at: Option<String>, // "YYYY-MM-DD"
}

pub async fn add_promo(
    State(state): State<AppState>,
    Form(form): Form<AddPromoForm>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    let promo_type = form.promo_type.trim().to_ascii_lowercase();
    if !matches!(promo_type.as_str(), "balance" | "subscription" | "trial") {
        return (StatusCode::BAD_REQUEST, "Invalid promo type").into_response();
    }
    if form.max_uses <= 0 {
        return (StatusCode::BAD_REQUEST, "max_uses must be greater than 0").into_response();
    }
    if matches!(promo_type.as_str(), "subscription" | "trial") && form.plan_id.is_none() {
        return (StatusCode::BAD_REQUEST, "plan_id is required for subscription/trial promo").into_response();
    }

    let expires_at = form.expires_at.and_then(|s| {
        if s.is_empty() { None }
        else {
            chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()
                .map(|d| d.and_hms_opt(23, 59, 59).unwrap())
                .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
        }
    });

    // For now hardcode admin_id = 1 or get from session
    // In a real app we'd get this from the session/context
    let admin_id = 1;

    match state.promo_service.create_promo(
        &form.code,
        &promo_type,
        form.plan_id,
        form.balance_amount,
        form.duration_days,
        form.traffic_gb,
        form.max_uses,
        expires_at,
        admin_id
    ).await {
        Ok(_) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("HX-Refresh", "true".parse().unwrap());
            (axum::http::StatusCode::OK, headers, "").into_response()
        },
        Err(e) => {
            error!("Failed to create promo: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to create promo").into_response()
        }
    }
}

pub async fn delete_promo(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM promo_codes WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await {
        Ok(_) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("HX-Refresh", "true".parse().unwrap());
            (axum::http::StatusCode::OK, headers, "").into_response()
        },
        Err(e) => {
            error!("Failed to delete promo: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete promo").into_response()
        }
    }
}
