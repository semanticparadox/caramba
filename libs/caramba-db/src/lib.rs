pub mod db;
pub mod models;
pub mod repositories;
pub mod utils;

use anyhow::{Context, Result};
pub use sqlx;

pub async fn connect(url: &str) -> Result<sqlx::PgPool> {
    // Явно задаём размер пула вместо дефолтных 10 соединений
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(url)
        .await
        .context("Failed to connect to PostgreSQL")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run DB migrations")?;

    Ok(pool)
}
