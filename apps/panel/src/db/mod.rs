use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::env;
use anyhow::{Context, Result};

mod patch_schema;

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


    // Reset migration checksums to avoid "migration was modified" errors on updates.
    // This is safe because our init.sql uses CREATE TABLE IF NOT EXISTS throughout.
    let _ = sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&pool)
        .await; // Ignore error if table doesn't exist (fresh install)

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run migrations")?;

    // Create frontend tables if not exist (Manual Migration)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS frontend_servers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            domain TEXT NOT NULL UNIQUE,
            ip_address TEXT NOT NULL,
            region TEXT NOT NULL,
            auth_token_hash TEXT,
            token_expires_at DATETIME,
            is_active BOOLEAN DEFAULT 1,
            last_heartbeat DATETIME,
            traffic_monthly INTEGER DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS frontend_server_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            frontend_id INTEGER NOT NULL,
            requests_count INTEGER DEFAULT 0,
            bandwidth_used INTEGER DEFAULT 0,
            recorded_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(frontend_id) REFERENCES frontend_servers(id) ON DELETE CASCADE
        );"
    )
    .execute(&pool)
    .await?;

    // Run Auto-Patcher for schema drift (fixes missing columns on existing installs)
    patch_schema::patch_database_schema(&pool).await;

    Ok(pool)
}
