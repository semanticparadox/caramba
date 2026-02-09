// Store Module
// Store categories and products management

use axum::{
    extract::{State, Form, Path},
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use tracing::info;

use crate::AppState;
use super::auth::get_auth_user;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "store_categories.html")]
pub struct StoreCategoriesTemplate {
    pub categories: Vec<crate::models::store::Category>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "store_products.html")]
pub struct StoreProductsTemplate {
    pub products: Vec<crate::models::store::Product>,
    pub categories: Vec<crate::models::store::Category>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Deserialize)]
pub struct CategoryForm {
    pub name: String,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
}

#[derive(Deserialize)]
pub struct ProductForm {
    pub category_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub price: i64,
    pub product_type: String,
    pub content: Option<String>,
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_store_categories_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let categories = state.store_service.get_categories().await.unwrap_or_default();
    
    let admin_path = state.admin_path.clone();

    let template = StoreCategoriesTemplate {
        categories,
        is_auth: true,
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "store".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn create_category(
    State(state): State<AppState>,
    Form(form): Form<CategoryForm>,
) -> impl IntoResponse {
    info!("Adding new category: {}", form.name);

    let res = sqlx::query("INSERT INTO categories (name, description, sort_order) VALUES (?, ?, ?)")
        .bind(&form.name)
        .bind(&form.description)
        .bind(form.sort_order)
        .execute(&state.pool)
        .await;

    let admin_path = state.admin_path.clone();

    match res {
        Ok(_) => [(("HX-Redirect", format!("{}/store/categories", admin_path)))].into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

pub async fn delete_category(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE category_id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    if count > 0 {
        return (axum::http::StatusCode::BAD_REQUEST, "Cannot delete category with existing products.").into_response();
    }

    let res = sqlx::query("DELETE FROM categories WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;
        
    match res {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response(),
    }
}

pub async fn get_store_products_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let products = sqlx::query_as::<_, crate::models::store::Product>("SELECT * FROM products ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let categories = state.store_service.get_categories().await.unwrap_or_default();
    
    let admin_path = state.admin_path.clone();

    let template = StoreProductsTemplate {
        products,
        categories,
        is_auth: true,
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path,
        active_page: "store".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn create_product(
    State(state): State<AppState>,
    Form(form): Form<ProductForm>,
) -> impl IntoResponse {
    info!("Adding new product: {} to category {}", form.name, form.category_id);

    let category_id = form.category_id;
    let name = form.name;
    let description = form.description.unwrap_or_default();
    let price = form.price;
    let product_type = form.product_type;
    let content = form.content.unwrap_or_default();

    let res = sqlx::query(
        "INSERT INTO products (category_id, name, description, price, product_type, content) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(category_id)
    .bind(name)
    .bind(description)
    .bind(price)
    .bind(product_type)
    .bind(content)
    .execute(&state.pool)
    .await;

    let admin_path = state.admin_path.clone();

    match res {
        Ok(_) => [(("HX-Redirect", format!("{}/store/products", admin_path)))].into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

pub async fn delete_product(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let res = sqlx::query("DELETE FROM products WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;
        
    match res {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(), 
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response(),
    }
}
