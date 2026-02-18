// Authentication Module
// Handles login, logout, and session management

use axum::{
    extract::{State, Form},
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use tracing::info;
use time::Duration;

use crate::AppState;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub admin_path: String,
    pub is_auth: bool,
    pub active_page: String,
    pub username: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get authenticated username from session cookie
pub async fn get_auth_user(state: &AppState, jar: &CookieJar) -> Option<String> {
    if let Some(cookie) = jar.get("admin_session") {
        let token = cookie.value();
        // Check Redis
        if let Ok(Some(username)) = state.redis.get(&format!("session:{}", token)).await {
             return Some(username);
        }
    }
    None
}

/// Check if user is authenticated
pub async fn is_authenticated(state: &AppState, jar: &CookieJar) -> bool {
    if let Some(cookie) = jar.get("admin_session") {
        let token = cookie.value();
        let redis_key = format!("session:{}", token);
        
        // Check if token exists in Redis
        if let Ok(Some(username)) = state.redis.get(&redis_key).await {
            // Verify this username actually exists in the DB
            let user_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admins WHERE username = $1)")
                .bind(&username)
                .fetch_one(&state.pool)
                .await
                .unwrap_or(false);
            
            return user_exists;
        }
    }
    false
}

/// Helper to create CookieJar with a cookie
fn jar_with_cookie(cookie: Cookie<'static>) -> CookieJar {
    let jar = CookieJar::new();
    jar.add(cookie)
}

// ============================================================================
// Route Handlers
// ============================================================================

/// GET /admin/login - Show login page
pub async fn get_login(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let admin_path = state.admin_path.clone();
    Html(LoginTemplate { 
        admin_path,
        is_auth: false,
        active_page: "login".to_string(),
        username: "".to_string(),
    }.render().unwrap())
}

/// POST /admin/login - Process login
pub async fn login(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>
) -> impl IntoResponse {
    // Check Database for user
    let admin_opts: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM admins WHERE username = $1")
        .bind(&form.username)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    // Verify password against DB hash (NO fallback for security)
    let is_valid = if let Some((hash,)) = admin_opts {
        bcrypt::verify(&form.password, &hash).unwrap_or(false)
    } else {
        false
    };

    if is_valid {
        let admin_path = state.admin_path.clone();
        
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("HX-Redirect", format!("{}/dashboard", admin_path).parse().unwrap());
        
        // Create session in Redis
        let token = uuid::Uuid::new_v4().to_string();
        let clean_username = form.username.trim().to_string();
        
        info!("Creating session for user: '{}' (token: {}...)", clean_username, &token[..6]);

        let _ = state.redis.set(
            &format!("session:{}", token),
            &clean_username,
            24 * 60 * 60 // 24 hours
        ).await;

        let cookie = Cookie::build(("admin_session", token))
            .path("/")
            .http_only(true)
            .build();

        (axum::http::StatusCode::OK, jar_with_cookie(cookie), headers, "Success").into_response()
    } else {
        Html("<div class='text-red-500 text-sm mt-2'>Invalid username or password</div>").into_response()
    }
}

/// POST /admin/logout - Logout user
pub async fn logout(jar: CookieJar) -> impl IntoResponse {
    let mut cookie = Cookie::from("admin_session");
    cookie.set_value("");
    cookie.set_path("/");
    cookie.set_max_age(Duration::seconds(0)); // Expire immediately
    
    let admin_path = std::env::var("ADMIN_PATH").unwrap_or_else(|_| "/admin".to_string());
    let admin_path = if admin_path.starts_with('/') { admin_path } else { format!("/{}", admin_path) };
    
    // Use HX-Redirect for HTMX clients (force full page reload)
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("HX-Redirect", format!("{}/login", admin_path).parse().unwrap());
    
    (jar.add(cookie), headers, "Logging out...")
}
