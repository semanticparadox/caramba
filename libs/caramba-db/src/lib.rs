pub mod db;
pub mod models;
pub mod repositories;
pub mod utils;

use anyhow::{Context, Result};
pub use sqlx;

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
