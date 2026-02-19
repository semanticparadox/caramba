use axum::response::IntoResponse;

pub async fn modern_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../assets/css/modern.css"),
    )
}
