use axum::{
    extract::{Path, State, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    body::Body,
};
use crate::AppState;
use reqwest::Client;

pub async fn proxy_handler(
    Path(path): Path<String>,
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    // Construct target URL
    // Remove /api/ prefix logic is tricky with *path?
    // Axum *path on /api/*path means `path` variable contains the suffix.
    // e.g. request /api/client/auth/telegram -> path = "client/auth/telegram"
    
    // We want to forward to {PANEL_URL}/api/{path}
    let target_url = format!("{}/api/{}", state.config.panel_url.trim_end_matches('/'), path);
    
    tracing::debug!("Proxying request to: {}", target_url);

    // Create client
    let client = Client::new();
    
    // Deconstruct request
    let (parts, body) = req.into_parts();
    
    // Build new request
    let mut proxy_req = client
        .request(parts.method, &target_url)
        .body(reqwest::Body::wrap_stream(body.into_data_stream())); // Stream body
        
    // Forward headers
    for (key, value) in parts.headers.iter() {
        // Skip host header to avoid issues
        if key.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        proxy_req = proxy_req.header(key, value);
    }
    
    // Send request
    match proxy_req.send().await {
        Ok(res) => {
            // Convert reqwest response to axum response
            let status = res.status();
            let mut response = Response::builder().status(status);
            
            // Forward response headers
            if let Some(headers) = response.headers_mut() {
                for (key, value) in res.headers().iter() {
                    headers.insert(key, value.clone());
                }
            }
            
            // Stream response body
            let body = Body::from_stream(res.bytes_stream());
            
            response.body(body).unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response())
        }
        Err(e) => {
            tracing::error!("Proxy error: {}", e);
            (StatusCode::BAD_GATEWAY, format!("Proxy error: {}", e)).into_response()
        }
    }
}
