pub mod models;
pub mod db;
pub mod repositories;
pub mod utils;

pub use sqlx;

pub async fn connect(url: &str) -> Result<sqlx::PgPool, sqlx::Error> {
    sqlx::PgPool::connect(url).await
}
