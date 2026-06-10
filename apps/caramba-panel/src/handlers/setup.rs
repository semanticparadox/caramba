use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Multipart, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Template, WebTemplate)]
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

pub async fn get_setup(State(state): State<AppState>) -> SetupTemplate {
    let admin_path = state.admin_path.clone();

    SetupTemplate {
        admin_path,
        is_auth: false,
        active_page: "setup".to_string(),
        username: "".to_string(),
    }
}

pub async fn create_admin(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CreateAdminForm>,
) -> impl IntoResponse {
    let hash = match bcrypt::hash(&form.password, 12) {
        Ok(h) => h,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Password hashing failed").into_response();
        }
    };

    // Atomic check-then-insert inside a transaction to prevent TOCTOU race
    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admins")
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(1); // fail safe: assume 1 if error

    if count > 0 {
        let _ = tx.rollback().await;
        return (StatusCode::FORBIDDEN, "Setup already completed").into_response();
    }

    match sqlx::query("INSERT INTO admins (username, password_hash) VALUES ($1, $2)")
        .bind(&form.username)
        .bind(hash)
        .execute(&mut *tx)
        .await
    {
        Ok(_) => {
            if let Err(e) = tx.commit().await {
                error!("Failed to commit admin creation: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
            }
            info!("Setup: Admin {} created successfully.", form.username);

            // Auto-login: создаём UUID-токен сессии и сохраняем в Redis,
            // как это делает login() — никогда не кладём session_secret в cookie
            let admin_path = state.admin_path.clone();
            let token = Uuid::new_v4().to_string();
            let clean_username = form.username.trim().to_string();

            info!(
                "Setup: creating session for user '{}' (token: {}...)",
                clean_username,
                &token[..6]
            );

            let _ = state
                .redis
                .set(
                    &format!("session:{}", token),
                    &clean_username,
                    24 * 60 * 60, // 24 часа
                )
                .await;

            let cookie = Cookie::build(("admin_session", token))
                .path("/")
                .http_only(true)
                .secure(true)
                .same_site(axum_extra::extract::cookie::SameSite::Lax)
                .build();

            let mut headers = axum::http::HeaderMap::new();
            if let Ok(val) = format!("{}/dashboard", admin_path).parse() {
                headers.insert("HX-Redirect", val);
            }

            (StatusCode::OK, jar.add(cookie), headers).into_response()
        }
        Err(e) => {
            error!("Failed to create admin: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
        }
    }
}

pub async fn restore_backup(
    State(state): State<AppState>,
    jar: CookieJar,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // Defence-in-depth: require authenticated admin session
    let is_authed = if let Some(cookie) = jar.get("admin_session") {
        let redis_key = format!("session:{}", cookie.value());
        state
            .redis
            .get(&redis_key)
            .await
            .ok()
            .flatten()
            .is_some()
    } else {
        false
    };
    if !is_authed {
        return (StatusCode::FORBIDDEN, "Authentication required").into_response();
    }

    // Strategy: Write to temp file, then rename/move over caramba.db.
    // Then exit process.

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("backup_file") {
            if let Ok(bytes) = field.bytes().await {
                if bytes.len() > 10 * 1024 * 1024 {
                    // 10MB limit
                    return (StatusCode::BAD_REQUEST, "File too large").into_response();
                }

                // Determine DB path
                // We assume current working dir has caramba.db (standard install)
                let db_path = "caramba.db";

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
                if let Ok(val) = format!("{}/login", admin_path).parse() {
                    headers.insert("HX-Redirect", val);
                }

                return (StatusCode::OK, headers, "Restored. Restarting...").into_response();
            }
        }
    }

    (StatusCode::BAD_REQUEST, "No file uploaded").into_response()
}
