use sqlx::{Pool, Sqlite};
use tracing::{info, warn, error};

/// Automatically patches the database schema on startup to match init.sql requirements.
/// This works around the issue where existing installs drift from the single init.sql file.
pub async fn patch_database_schema(pool: &Pool<Sqlite>) {
    info!("🔧 Checking database schema health...");

    // 1. Check for 'is_trial' column in 'plans'
    // We try to select it. If error, we add it.
    // 1. Check for 'is_trial' column in 'plans'
    let has_is_trial: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('plans') WHERE name='is_trial'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_is_trial {
        warn!("⚠️ Missing column 'is_trial' in 'plans'. Patching...");
        if let Err(e) = sqlx::query("ALTER TABLE plans ADD COLUMN is_trial BOOLEAN DEFAULT 0").execute(pool).await {
            error!("Failed to add 'is_trial' column: {}", e);
        } else {
            info!("✅ Added 'is_trial' column to 'plans'");
            
            // Also backfill default trial plan if not exists
            let _ = sqlx::query("UPDATE plans SET is_trial = 1 WHERE name = 'Free Trial'").execute(pool).await;
        }
    }

    // 2. Check for 'api_keys' table
    // We check if it exists in sqlite_master
    let table_exists: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM sqlite_master WHERE type='table' AND name='api_keys'")
        .fetch_one(pool)
        .await
        .unwrap_or(false);

    if !table_exists {
        warn!("⚠️ Missing table 'api_keys'. Patching...");
        let sql = r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            type TEXT NOT NULL DEFAULT 'enrollment',
            max_uses INTEGER,
            current_uses INTEGER DEFAULT 0,
            is_active BOOLEAN DEFAULT 1,
            expires_at DATETIME,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            created_by INTEGER,
            FOREIGN KEY (created_by) REFERENCES admins(id)
        );
        CREATE INDEX IF NOT EXISTS idx_api_keys_key ON api_keys(key);
        "#;
        
        if let Err(e) = sqlx::query(sql).execute(pool).await {
            error!("Failed to create 'api_keys' table: {}", e);
        } else {
            info!("✅ Created 'api_keys' table");
        }
    }
    
    // 3. Check for 'reality_sni' in 'nodes' (Just in case)
    // Use fetch_optional to avoid RowNotFound error on empty tables triggering a false negative
    // 3. Check for 'reality_sni' in 'nodes'
    let has_reality_sni: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='reality_sni'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_reality_sni {
        warn!("⚠️ Missing column 'reality_sni' in 'nodes'. Patching...");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN reality_sni TEXT DEFAULT 'www.google.com'").execute(pool).await {
            // Ignore duplicate column error if we raced or failed check
            if !e.to_string().contains("duplicate column") {
                error!("Failed to add 'reality_sni' column: {}", e);
            }
        } else {
             info!("✅ Added 'reality_sni' column to 'nodes'");
        }
    }
    
    // 7. Ensure 'status' in nodes (TEXT)
    let has_status: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='status'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_status {
        info!("Applying schema repair: Adding 'status' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN status TEXT DEFAULT 'new'").execute(pool).await {
             warn!("Failed to add status column: {}", e);
        }
    }

    // 8. Ensure 'last_seen' in nodes (DATETIME)
    let has_last_seen: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='last_seen'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_last_seen {
        info!("Applying schema repair: Adding 'last_seen' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN last_seen DATETIME").execute(pool).await {
             warn!("Failed to add last_seen column: {}", e);
        }
    }

    // 9. Ensure 'join_token' in nodes (TEXT)
    let has_token: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM pragma_table_info('nodes') WHERE name='join_token'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_token {
        info!("Applying schema repair: Adding 'join_token' to nodes table");
        if let Err(e) = sqlx::query("ALTER TABLE nodes ADD COLUMN join_token TEXT").execute(pool).await {
             warn!("Failed to add join_token column: {}", e);
        }
    }

    info!("✅ Database schema check complete.");
}
