use axum::response::IntoResponse;

pub async fn modern_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../assets/css/modern.css"),
    )
}

/// Keep the product icon available in binary-only installs, like the panel CSS.
pub async fn caramba_icon() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=300"),
        ],
        include_bytes!("../../assets/brand/caramba-icon.png").as_slice(),
    )
}
