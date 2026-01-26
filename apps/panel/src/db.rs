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

    // 8. Ensure 'root_password' in nodes (TEXT)
    let has_root_pass: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='root_password'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_root_pass {
        tracing::info!("Applying schema repair: Adding 'root_password' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN root_password TEXT").execute(&pool).await {
             tracing::warn!("Failed to add root_password column: {}", e);
        }
    }
    
    // 9. Ensure 'join_token' in nodes (TEXT)
    let has_token: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='join_token'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_token {
        tracing::info!("Applying schema repair: Adding 'join_token' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN join_token TEXT").execute(&pool).await {
             tracing::warn!("Failed to add join_token column: {}", e);
        }
    }
    
    // 10. Ensure 'auto_configure' in nodes (BOOLEAN)
    let has_auto_conf: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='auto_configure'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_auto_conf {
        tracing::info!("Applying schema repair: Adding 'auto_configure' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN auto_configure BOOLEAN DEFAULT 0").execute(&pool).await {
             tracing::warn!("Failed to add auto_configure column: {}", e);
        }
    }

    // 11. Ensure 'country_code' in nodes (TEXT, 2 chars)
    let has_country: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='country_code'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if !has_country {
        tracing::info!("Applying schema repair: Adding 'country_code' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN country_code TEXT").execute(&pool).await {
             tracing::warn!("Failed to add country_code column: {}", e);
        }
    }

    Ok(pool)
}
