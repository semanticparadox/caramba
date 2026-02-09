use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use crate::AppState;
use crate::handlers::admin::auth::get_admin_user_from_cookie;

pub async fn get_frontends(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let (user, new_jar) = match get_admin_user_from_cookie(&state.pool, jar).await {
        Ok(u) => u,
        Err(_) => return axum::response::Redirect::to("/admin/login").into_response(),
    };

    let mut context = tera::Context::new();
    context.insert("user", &user);
    context.insert("active_tab", "frontends");

    // Pass global settings to template if needed
    let frontend_mode = state.settings.get_or_default("frontend_mode", "local").await;
    context.insert("frontend_mode", &frontend_mode);

    match state.tera.render("frontends.html", &context) {
        Ok(html) => (new_jar, Html(html)).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            (new_jar, Html("Internal Server Error".to_string())).into_response()
        }
    }
}
