use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
};
use std::path::PathBuf;

const MINI_APP_DIST_DIR: &str = "apps/caramba-app/dist";

fn build_asset_response(bytes: Vec<u8>, mime: &str, cache_control: &str) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache_control);

    if cache_control.starts_with("no-store") {
        builder = builder
            .header(header::PRAGMA, "no-cache")
            .header(header::EXPIRES, "0");
    }

    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to build response"))
            .expect("response builder should work")
    })
}

fn cache_control_for_path(path: &str) -> &'static str {
    if path == "index.html" {
        "no-store, no-cache, must-revalidate, max-age=0"
    } else {
        "public, max-age=300, must-revalidate"
    }
}

async fn read_asset(path: &str) -> Option<Response> {
    if path.contains("..") {
        return None;
    }

    let full_path: PathBuf = PathBuf::from(MINI_APP_DIST_DIR).join(path);
    if !full_path.exists() || !full_path.is_file() {
        return None;
    }

    let bytes = tokio::fs::read(&full_path).await.ok()?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(build_asset_response(
        bytes,
        mime.as_ref(),
        cache_control_for_path(path),
    ))
}

pub async fn serve_app(State(state): State<AppState>) -> Response {
    // Check if local app serving is enabled in settings
    let enabled = state
        .settings
        .get_or_default("miniapp_enabled", "true")
        .await
        == "true";

    if !enabled {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("MiniApp disabled"))
            .expect("response builder should work");
    }

    serve_app_assets(Path("index.html".to_string())).await
}

/// Serve Mini App static assets
pub async fn serve_app_assets(Path(path): Path<String>) -> Response {
    let path = path.trim_start_matches('/');

    if let Some(response) = read_asset(path).await {
        return response;
    }

    // SPA fallback
    if !path.contains('.') {
        if let Some(response) = read_asset("index.html").await {
            return response;
        }
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not Found"))
        .expect("response builder should work")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Path;

    #[tokio::test]
    async fn serve_app_assets_rejects_path_traversal() {
        let response = serve_app_assets(Path("../secrets.txt".to_string())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn serve_app_assets_missing_file_with_extension_returns_not_found() {
        let response = serve_app_assets(Path("definitely-missing-file.js".to_string())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
