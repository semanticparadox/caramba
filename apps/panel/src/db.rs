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

    // Post-migration repairs (Fix for existing installations missing columns)
    // 1. Ensure 'is_enabled' in nodes
    let has_is_enabled: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='is_enabled'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_is_enabled {
        tracing::info!("Applying schema repair: Adding 'is_enabled' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN is_enabled BOOLEAN DEFAULT 1").execute(&pool).await {
            tracing::warn!("Failed to add is_enabled column (might exist?): {}", e);
        }
    }

    // 2. Ensure 'balance' in users (Integer, stored in cents)
    let has_balance: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('users') WHERE name='balance'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_balance {
        tracing::info!("Applying schema repair: Adding 'balance' to users table");
        if let Err(e) = sqlx::query("ALTER TABLE users ADD COLUMN balance INTEGER DEFAULT 0").execute(&pool).await {
             tracing::warn!("Failed to add balance column: {}", e);
        }
    }

    // 3. Ensure 'traffic_limit_gb' in plans
    let has_traffic_limit: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('plans') WHERE name='traffic_limit_gb'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_traffic_limit {
        tracing::info!("Applying schema repair: Adding 'traffic_limit_gb' to plans table");
        // Default 0 means unlimited or just unset, handled by logic
        if let Err(e) = sqlx::query("ALTER TABLE plans ADD COLUMN traffic_limit_gb INTEGER DEFAULT 0").execute(&pool).await {
             tracing::warn!("Failed to add traffic_limit_gb column: {}", e);
        }
    }

    // 4. Ensure 'terms_accepted_at' in users (DATETIME)
    let has_terms: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('users') WHERE name='terms_accepted_at'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_terms {
        tracing::info!("Applying schema repair: Adding 'terms_accepted_at' to users table");
        if let Err(e) = sqlx::query("ALTER TABLE users ADD COLUMN terms_accepted_at DATETIME").execute(&pool).await {
             tracing::warn!("Failed to add terms_accepted_at column: {}", e);
        }
    }

    // 5. Ensure 'warning_count' in users (INTEGER)
    let has_warnings: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('users') WHERE name='warning_count'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_warnings {
        tracing::info!("Applying schema repair: Adding 'warning_count' to users table");
        if let Err(e) = sqlx::query("ALTER TABLE users ADD COLUMN warning_count INTEGER DEFAULT 0").execute(&pool).await {
             tracing::warn!("Failed to add warning_count column: {}", e);
        }
    }
    
    // 6. Ensure 'status' in nodes (TEXT)
    let has_status: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='status'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_status {
        tracing::info!("Applying schema repair: Adding 'status' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN status TEXT DEFAULT 'new'").execute(&pool).await {
             tracing::warn!("Failed to add status column: {}", e);
        }
    }

    // 7. Ensure 'last_seen' in nodes (DATETIME)
    let has_last_seen: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='last_seen'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_last_seen {
        tracing::info!("Applying schema repair: Adding 'last_seen' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN last_seen DATETIME").execute(&pool).await {
             tracing::warn!("Failed to add last_seen column: {}", e);
        }
    }

    Ok(pool)
}
