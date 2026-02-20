use crate::AppState;
use axum::{
    body::{Body, to_bytes},
    extract::{Path, Request, State},
    http::{HeaderName, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::Client;
use std::collections::HashSet;

const MAX_PROXY_BODY_BYTES: usize = 8 * 1024 * 1024;

fn should_skip_request_header(name: &HeaderName) -> bool {
    let key = name.as_str();
    key.eq_ignore_ascii_case("host")
        || key.eq_ignore_ascii_case("connection")
        || key.eq_ignore_ascii_case("transfer-encoding")
        || key.eq_ignore_ascii_case("content-length")
}

fn candidate_targets(panel_url: &str, frontend_domain: &str, path: &str) -> Vec<String> {
    let normalized = panel_url.trim_end_matches('/');
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    let push_target = |targets: &mut Vec<String>, seen: &mut HashSet<String>, url: String| {
        if seen.insert(url.clone()) {
            targets.push(url);
        }
    };

    // Primary configured target.
    push_target(
        &mut targets,
        &mut seen,
        format!("{}/api/{}", normalized, path),
    );

    if let Ok(parsed) = reqwest::Url::parse(normalized) {
        // Fallback to plain HTTP on same host when HTTPS between local services is broken.
        if parsed.scheme().eq_ignore_ascii_case("https") {
            let mut http_url = parsed.clone();
            let _ = http_url.set_scheme("http");
            push_target(
                &mut targets,
                &mut seen,
                format!("{}/api/{}", http_url.as_str().trim_end_matches('/'), path),
            );
        }

        // If panel host equals frontend host, prefer direct local panel port as a final fallback.
        if let Some(host) = parsed.host_str() {
            if host.eq_ignore_ascii_case(frontend_domain)
                || host.eq_ignore_ascii_case("localhost")
                || host == "127.0.0.1"
            {
                push_target(
                    &mut targets,
                    &mut seen,
                    format!("http://127.0.0.1:3000/api/{}", path),
                );
            }
        }
    }

    // Universal fallback for single-host deployments.
    push_target(
        &mut targets,
        &mut seen,
        format!("http://127.0.0.1:3000/api/{}", path),
    );
    push_target(
        &mut targets,
        &mut seen,
        format!("http://localhost:3000/api/{}", path),
    );

    targets
}

pub async fn proxy_handler(
    Path(path): Path<String>,
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    let client = Client::new();
    let (parts, body) = req.into_parts();
    let body_bytes = match to_bytes(body, MAX_PROXY_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!("Proxy request body read failed: {}", err);
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body is too large for proxy",
            )
                .into_response();
        }
    };

    let targets = candidate_targets(&state.config.panel_url, &state.config.domain, &path);
    let mut errors: Vec<String> = Vec::new();

    for target_url in targets {
        tracing::debug!("Proxying request to: {}", target_url);

        let mut proxy_req = client
            .request(parts.method.clone(), &target_url)
            .body(body_bytes.clone());

        for (key, value) in &parts.headers {
            if should_skip_request_header(key) {
                continue;
            }
            proxy_req = proxy_req.header(key, value);
        }

        match proxy_req.send().await {
            Ok(res) => {
                let status = res.status();
                let mut response = Response::builder().status(status);

                if let Some(headers) = response.headers_mut() {
                    for (key, value) in res.headers() {
                        headers.insert(key, value.clone());
                    }
                }

                let body = Body::from_stream(res.bytes_stream());
                return response.body(body).unwrap_or_else(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to build response",
                    )
                        .into_response()
                });
            }
            Err(err) => {
                let msg = format!("{} => {}", target_url, err);
                tracing::warn!("Proxy target failed: {}", msg);
                errors.push(msg);
            }
        }
    }

    tracing::error!("All proxy targets failed for /api/{}: {:?}", path, errors);
    (
        StatusCode::BAD_GATEWAY,
        format!(
            "Proxy error: all upstream targets failed. {}",
            errors.join(" | ")
        ),
    )
        .into_response()
}
