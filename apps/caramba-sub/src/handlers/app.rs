use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode},
    response::Response,
};
use std::path::PathBuf;

const MINI_APP_DIST_DIR: &str = "apps/caramba-app/dist";

fn build_asset_response(bytes: Vec<u8>, mime: &str, cache_control: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Failed to build response"))
                .expect("response builder should work")
        })
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
    Some(build_asset_response(bytes, mime.as_ref(), "public, max-age=3600"))
}

/// Serve Mini App main page
pub async fn serve_app() -> Response {
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
        let response = serve_app_assets(Path("../secret.txt".to_string())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn serve_app_assets_missing_file_with_extension_returns_not_found() {
        let response = serve_app_assets(Path("definitely-missing-file.js".to_string())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
