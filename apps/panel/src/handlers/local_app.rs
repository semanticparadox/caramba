use axum::{
    extract::{Path, State}, // State needed if we toggle feature in future
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
use crate::AppState;

#[derive(RustEmbed)]
#[folder = "../../mini-app/dist"]
struct MiniAppAssets;

pub async fn serve_app(State(state): State<AppState>) -> Response {
    // Check if local app serving is enabled in settings
    let enabled = state.settings.get_or_default("miniapp_enabled", "true")
        .await == "true";
    
    if !enabled { 
        return (StatusCode::NOT_FOUND, "MiniApp disabled").into_response(); 
    }
    
    serve_app_assets(Path("index.html".to_string())).await
}

/// Serve Mini App static assets
pub async fn serve_app_assets(Path(path): Path<String>) -> Response {
    // Remove leading slash if present
    let path = path.trim_start_matches('/');
    
    // Try to get the file from embedded assets
    match MiniAppAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, "public, max-age=3600"), // Cache for 1 hour
                ],
                content.data
            ).into_response()
        }
        None => {
            // If not found and doesn't have extension, try index.html (SPA routing)
            if !path.contains('.') {
                if let Some(index) = MiniAppAssets::get("index.html") {
                    return (
                        [(header::CONTENT_TYPE, "text/html")],
                        index.data
                    ).into_response();
                }
            }
            
            // Otherwise 404
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }
}
