use axum::{
    extract::{State, Path},
    Json,
    response::IntoResponse,
    http::StatusCode,
};
use crate::AppState;
use serde::{Deserialize, Serialize};

// Helper struct for bot verification (stub)
#[derive(Deserialize)]
pub struct VerifyUserRequest {
    pub telegram_id: i64,
}

#[derive(Serialize)]
pub struct VerifyUserResponse {
    pub verified: bool,
    pub user_id: Option<i64>,
    pub username: Option<String>,
}

pub async fn verify_user(
    State(state): State<AppState>,
    Json(payload): Json<VerifyUserRequest>,
) -> impl IntoResponse {
    let user: Option<caramba_db::models::store::User> = state.store_service.get_user_by_tg_id(payload.telegram_id).await.unwrap_or(None);

    if let Some(user) = user {
        Json(VerifyUserResponse {
            verified: true,
            user_id: Some(user.id),
            username: user.username,
        })
    } else {
        Json(VerifyUserResponse {
            verified: false,
            user_id: None,
            username: None,
        })
    }
}

#[derive(Deserialize)]
pub struct UpsertUserRequest {
    pub tg_id: i64,
    pub username: Option<String>,
    pub full_name: Option<String>,
    pub referrer_id: Option<i64>,
}

pub async fn upsert_user(
    State(state): State<AppState>,
    Json(payload): Json<UpsertUserRequest>,
) -> impl IntoResponse {
    match state.store_service.upsert_user(payload.tg_id, payload.username.as_deref(), payload.full_name.as_deref(), payload.referrer_id).await {
        Ok(user) => (StatusCode::OK, Json(Some(user))).into_response(),
        Err(e) => {
            tracing::error!("bot upsert_user failed for tg_id {}: {}", payload.tg_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

pub async fn get_user_by_tg(
    State(state): State<AppState>,
    Path(tg_id): Path<i64>,
) -> impl IntoResponse {
    match state.store_service.get_user_by_tg_id(tg_id).await {
        Ok(user) => Json(user).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn get_plans(State(state): State<AppState>) -> impl IntoResponse {
    match state.catalog_service.get_active_plans().await {
        Ok(plans) => Json(plans).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_user_subs(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    match state.subscription_service.get_user_subscriptions(user_id).await {
        Ok(subs) => Json(subs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_categories(State(state): State<AppState>) -> impl IntoResponse {
    match state.catalog_service.get_categories().await {
        Ok(cats) => Json(cats).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_products_by_category(
    State(state): State<AppState>,
    Path(category_id): Path<i64>,
) -> impl IntoResponse {
    match state.catalog_service.get_products_by_category(category_id).await {
        Ok(prods) => Json(prods).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct PurchasePlanRequest {
    pub duration_id: i64,
}

pub async fn purchase_plan(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(payload): Json<PurchasePlanRequest>,
) -> impl IntoResponse {
    match state.store_service.purchase_plan(user_id, payload.duration_id).await {
        Ok(sub) => Json(sub).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PurchaseProductRequest {
    pub product_id: i64,
}

pub async fn purchase_product(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(payload): Json<PurchaseProductRequest>,
) -> impl IntoResponse {
    match state.store_service.purchase_product_with_balance(user_id, payload.product_id).await {
        Ok(prod) => Json(prod).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

pub async fn get_settings(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let val = state.settings.get_or_default(&key, "").await;
    Json(Some(val))
}

pub async fn get_sub_links(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
) -> impl IntoResponse {
    match state.subscription_service.get_subscription_links(sub_id).await {
        Ok(links) => Json(links).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn activate_sub(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = payload.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0);
    match state.store_service.activate_subscription(sub_id, user_id).await {
        Ok(sub) => Json(sub).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}
