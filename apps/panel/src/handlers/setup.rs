use axum::{
    extract::{State, Form, Multipart},
    response::{IntoResponse, Html},
    http::StatusCode,
};
use askama::Template;
use serde::Deserialize;
use crate::AppState;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use tracing::{info, error};

#[derive(Template)]
#[template(path = "setup.html")]
pub struct SetupTemplate {
    pub admin_path: String,
    pub is_auth: bool,
    pub active_page: String,
    pub username: String, // NEW
}

#[derive(Deserialize)]
pub struct CreateAdminForm {
    pub username: String,
    pub password: String,
}

pub async fn get_setup(State(state): State<AppState>) -> impl IntoResponse {
    let admin_path = state.admin_path.clone();
    
    Html(SetupTemplate { 
        admin_path,
        is_auth: false,
        active_page: "setup".to_string(),
        username: "".to_string(),
    }.render().unwrap_or_default())
}

pub async fn create_admin(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CreateAdminForm>,
) -> impl IntoResponse {
    // Double check if admin exists to prevent abuse if middleware fails
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admins")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(1); // fail safe: assume 1 if error

    if count > 0 {
        return (StatusCode::FORBIDDEN, "Setup already completed").into_response();
    }

    let hash = match bcrypt::hash(&form.password, bcrypt::DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Password hashing failed").into_response(),
    };

    match sqlx::query("INSERT INTO admins (username, password_hash) VALUES (?, ?)")
        .bind(&form.username)
        .bind(hash)
        .execute(&state.pool)
        .await
    {
        Ok(_) => {
            info!("Setup: Admin {} created successfully.", form.username);
            
            // Auto-login
            let admin_path = state.admin_path.clone();
            let cookie = Cookie::build(("admin_session", state.session_secret.clone()))
                .path("/")
                .http_only(true)
                .build();
                
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("HX-Redirect", format!("{}/dashboard", admin_path).parse().unwrap());

            (StatusCode::OK, jar.add(cookie), headers).into_response()
        },
        Err(e) => {
            error!("Failed to create admin: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
        }
    }
}

pub async fn restore_backup(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // We don't need State because we are overwriting the DB file directly.
    // However, writing to open DB file is risky.
    // Strategy: Write to temp file, then rename/move over exarobot.db.
    // Then exit process.
    
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("backup_file") {
            if let Ok(bytes) = field.bytes().await {
                if bytes.len() > 10 * 1024 * 1024 { // 10MB limit
                    return (StatusCode::BAD_REQUEST, "File too large").into_response();
                }
                
                // Determine DB path
                // We assume current working dir has exarobot.db (standard install)
                let db_path = "exarobot.db";
                
                // Backup current just in case (though install.sh does it too)
                let _ = std::fs::copy(db_path, format!("{}.pre_restore.bak", db_path));
                
                // Overwrite
                if let Err(e) = std::fs::write(db_path, bytes) {
                     error!("Failed to write restored DB: {}", e);
                     return (StatusCode::INTERNAL_SERVER_ERROR, "Write failed").into_response();
                }
                
                info!("Database restored. Restarting server...");
                
                // Trigger client-side reload after delay
                // Return script to reload page? No, better return header.
                // But the server will die soon.
                
                // Spawn a thread to kill process after 1s allow response to send
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    std::process::exit(0);
                });
                
                let admin_path = state.admin_path.clone();
                let mut headers = axum::http::HeaderMap::new();
                headers.insert("HX-Redirect", format!("{}/login", admin_path).parse().unwrap());
                
                return (StatusCode::OK, headers, "Restored. Restarting...").into_response();
            }
        }
    }
    
    (StatusCode::BAD_REQUEST, "No file uploaded").into_response()
}
