pub mod models;
pub mod db;
pub mod repositories;
pub mod utils;

pub use sqlx;
use anyhow::{Context, Result};

pub async fn connect(url: &str) -> Result<sqlx::PgPool> {
    let pool = sqlx::PgPool::connect(url)
        .await
        .context("Failed to connect to PostgreSQL")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run DB migrations")?;

    Ok(pool)
}
