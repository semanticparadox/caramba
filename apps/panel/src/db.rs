use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::env;
use anyhow::{Context, Result};

pub async fn init_db() -> Result<SqlitePool> {
    let database_url = env::var("DATABASE_URL")
        .context("DATABASE_URL must be set in .env")?;

    // Create the database file if it doesn't exist
    if !database_url.starts_with("sqlite://") {
        return Err(anyhow::anyhow!("DATABASE_URL must start with sqlite://"));
    }

    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    let options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(10));

    let pool = SqlitePoolOptions::new()
        .max_connections(20)
        .connect_with(options)
        .await
        .context("Failed to connect to SQLite")?;


    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run migrations")?;

    Ok(pool)
}
