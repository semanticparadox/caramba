use axum::{
    extract::{State, Form, Multipart, Path},
    response::{IntoResponse, Html},
    http::StatusCode,
    body::Bytes,
};
use askama::Template;
use serde::Deserialize;
use crate::AppState;
use crate::models::store::{Category, Product, Order};
use axum_extra::extract::cookie::CookieJar;
// // use tracing::error;

// --- Templates ---

#[derive(Template)]
#[template(path = "store_categories.html")]
pub struct CategoriesTemplate {
    pub categories: Vec<Category>,
    pub is_auth: bool,
    pub admin_path: String,
}

#[derive(Template)]
#[template(path = "store_products.html")]
pub struct ProductsTemplate {
    pub products: Vec<Product>,
    pub categories: Vec<Category>,
    pub is_auth: bool,
    pub admin_path: String,
}

#[derive(Template)]
#[template(path = "store_orders.html")]
pub struct OrdersTemplate {
    pub orders: Vec<Order>,
    pub is_auth: bool,
    pub admin_path: String,
}

// --- Forms ---

#[derive(Deserialize)]
pub struct AddCategoryForm {
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i64,
}

// Product form is multipart, handled manually

// --- Handlers: Categories ---

pub async fn categories_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() { return Html("Unauthorized".to_string()); }

    let categories = state.store_service.get_categories().await.unwrap_or_default();
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    Html(CategoriesTemplate { categories, is_auth: true, admin_path }.render().unwrap())
}

pub async fn add_category(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AddCategoryForm>,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() {
        return StatusCode::UNAUTHORIZED.into_response(); 
    }
    
    // Note: The original code checked "session" cookie, but login uses "admin_session".
    // This might be why it failed if user only had "admin_session".
    
    use tracing::info;
    info!("Adding category: {}", form.name);

    match sqlx::query("INSERT INTO categories (name, description, sort_order) VALUES (?, ?, ?)")
        .bind(form.name)
        .bind(form.description)
        .bind(form.sort_order)
        .execute(&state.pool)
        .await {
            Ok(_) => {
                let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
                ([("HX-Redirect", format!("{}/store/categories", admin_path))], "Redirecting...").into_response()
            },
            Err(e) => {
                use tracing::error;
                error!("Failed to add category: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to add category: {}", e)).into_response()
            }
        }
}

pub async fn delete_category(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() { return StatusCode::UNAUTHORIZED; }

    let _ = sqlx::query("UPDATE categories SET is_active = 0 WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    StatusCode::OK
}

// --- Handlers: Products ---

pub async fn products_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() { return Html("Unauthorized".to_string()); }

    // Fetch all products (raw query for now as StoreService filters by category)
    let products = sqlx::query_as::<_, Product>("SELECT * FROM products WHERE is_active = 1 ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    
    let categories = state.store_service.get_categories().await.unwrap_or_default();
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    Html(ProductsTemplate { products, categories, is_auth: true, admin_path }.render().unwrap())
}

pub async fn add_product(
    State(state): State<AppState>,
    jar: CookieJar,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() { 
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut name = String::new();
    let mut description = None;
    let mut category_id = 0;
    let mut price = 0;
    let mut product_type = String::new();
    let mut content = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name_opt = field.name();
        let field_name = if let Some(n) = name_opt {
            n.to_string()
        } else {
            continue;
        };

        if field_name == "content_file" {
            // Handle file upload
            if let Ok(bytes) = field.bytes().await {
                 let b: Bytes = bytes;
                 if !b.is_empty() {
                     // In real app: save to disk/S3. Here: store base64 or path
                     content = Some("File uploaded (Storage pending implementation)".to_string());
                 }
            }
        } else {
             if let Ok(text) = field.text().await {
                 match field_name.as_str() {
                     "name" => name = text,
                     "description" => description = Some(text),
                     "category_id" => category_id = text.parse().unwrap_or(0),
                     "price" => price = text.parse().unwrap_or(0),
                     "product_type" => product_type = text,
                     "content_text" => if !text.is_empty() { content = Some(text) },
                     _ => {}
                 }
             }
        }
    }

    // Validate inputs
    if name.is_empty() || category_id == 0 || price < 0 {
         use tracing::error;
         error!("Invalid product data: name='{}', category_id={}, price={}", name, category_id, price);
         return (StatusCode::BAD_REQUEST, "Invalid product data. Category, Name, and positive Price are required.").into_response();
    }
    
    use tracing::info;
    info!("Adding product '{}' to category {}", name, category_id);

    match sqlx::query("INSERT INTO products (category_id, name, description, price, product_type, content) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(category_id)
        .bind(name)
        .bind(description)
        .bind(price)
        .bind(product_type)
        .bind(content)
        .execute(&state.pool)
        .await {
            Ok(_) => {
                let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
                let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
                ([("HX-Redirect", format!("{}/store/products", admin_path))], "Redirecting...").into_response()
            },
             Err(e) => {
                use tracing::error;
                error!("Failed to add product: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to add product: {}", e)).into_response()
            }
        }
}

pub async fn delete_product(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() { return StatusCode::UNAUTHORIZED; }

    let _ = sqlx::query("UPDATE products SET is_active = 0 WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    StatusCode::OK
}

// --- Handlers: Orders ---

pub async fn orders_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if jar.get("admin_session").is_none() { return Html("Unauthorized".to_string()); }

    let orders = sqlx::query_as::<_, Order>("SELECT * FROM orders ORDER BY created_at DESC LIMIT 100")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };

    Html(OrdersTemplate { orders, is_auth: true, admin_path }.render().unwrap())
}
