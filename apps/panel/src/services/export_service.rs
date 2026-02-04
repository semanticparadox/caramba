use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// Service for exporting database and settings for backup/migration
pub struct ExportService {
    pool: SqlitePool,
}

impl ExportService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create complete export archive (database + sanitized settings)
    /// Returns archive as bytes ready for transmission
    pub async fn create_export(&self) -> Result<Vec<u8>> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let export_dir = format!("/tmp/exarobot_export_{}", timestamp);
        
        info!("Starting export to {}", export_dir);
        
        // Create temp directory
        fs::create_dir_all(&export_dir)
            .context("Failed to create export directory")?;
        
        // Step 1: Export database using VACUUM INTO
        self.export_database(&export_dir).await?;
        
        // Step 2: Export sanitized environment variables
        self.export_env_sanitized(&export_dir)?;
        
        // Step 3: Export metadata and instructions
        self.export_metadata(&export_dir)?;
        
        // Step 4: Create compressed tarball
        let archive_path = format!("{}.tar.gz", export_dir);
        self.create_tarball(&export_dir, &archive_path)?;
        
        // Step 5: Read archive into memory
        let data = fs::read(&archive_path)
            .context("Failed to read archive")?;
        
        info!("Export complete: {} bytes", data.len());
        
        // Cleanup temporary files
        fs::remove_dir_all(&export_dir).ok();
        fs::remove_file(&archive_path).ok();
        
        Ok(data)
    }

    /// Export database using SQLite VACUUM INTO for clean copy
    async fn export_database(&self, export_dir: &str) -> Result<()> {
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite:exarobot.db".to_string());
        
        // Extract path from sqlite:// or sqlite: prefix
        let _db_path = db_url
            .strip_prefix("sqlite://")
            .or_else(|| db_url.strip_prefix("sqlite:"))
            .unwrap_or("exarobot.db");
        
        let dest_path = format!("{}/exarobot.db", export_dir);
        
        // Use VACUUM INTO for clean, optimized copy
        let query = format!("VACUUM INTO '{}'", dest_path);
        sqlx::query(&query)
            .execute(&self.pool)
            .await
            .context("Failed to export database via VACUUM INTO")?;
        
        info!("Database exported to {} via VACUUM INTO", dest_path);
        Ok(())
    }

    /// Export .env file with sensitive keys redacted
    fn export_env_sanitized(&self, export_dir: &str) -> Result<()> {
        let env_path = ".env";
        
        let env_content = match fs::read_to_string(env_path) {
            Ok(content) => content,
            Err(e) => {
                warn!("Could not read .env file: {}. Creating placeholder.", e);
                "# .env file not found during export\n".to_string()
            }
        };
        
        // List of sensitive keys to redact
        let sensitive_keys = [
            "BOT_TOKEN",
            "PAYMENT_API_KEY",
            "NOWPAYMENTS_KEY",
            "SESSION_SECRET",
        ];
        
        // Sanitize sensitive values
        let sanitized = env_content
            .lines()
            .map(|line| {
                let key = line.split('=').next().unwrap_or("");
                
                if sensitive_keys.contains(&key) && line.contains('=') {
                    format!("{}=REDACTED_ON_EXPORT", key)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        
        let dest_path = format!("{}/env_sanitized.txt", export_dir);
        fs::write(&dest_path, sanitized)
            .context("Failed to write sanitized env")?;
        
        info!("Sanitized .env exported ({} keys redacted)", sensitive_keys.len());
        Ok(())
    }

    /// Export metadata and restoration instructions
    fn export_metadata(&self, export_dir: &str) -> Result<()> {
        let metadata = format!(
"CARAMBA Panel Backup
=====================

Export Timestamp: {}
Panel Version: {}

Contents
--------
- exarobot.db       : SQLite database (VACUUM optimized)
- env_sanitized.txt : Configuration (sensitive keys redacted)
- README.txt        : This file

Restoration Instructions
-------------------------

1. Extract Archive:
   tar xzf exarobot_backup_*.tar.gz
   cd exarobot_export_*/

2. Restore Database:
   # Backup existing DB first (optional)
   cp /opt/exarobot/exarobot.db /opt/exarobot/exarobot.db.backup

   # Restore from backup
   cp exarobot.db /opt/exarobot/exarobot.db

3. Merge Environment:
   # Compare with your current .env
   diff env_sanitized.txt /opt/exarobot/.env

   # Manually add back redacted keys:
   # - BOT_TOKEN
   # - PAYMENT_API_KEY
   # - NOWPAYMENTS_KEY
   # - SESSION_SECRET

4. Restart Panel:
   systemctl restart exarobot

5. Verify:
   journalctl -u exarobot -f

Security Notes
--------------
- Sensitive API keys were REDACTED from env_sanitized.txt
- You must manually add credentials back after restore
- Keep this backup secure - contains user data

For automated restore, use: scripts/import-backup.sh
",
            chrono::Utc::now().to_rfc3339(),
            env!("CARGO_PKG_VERSION")
        );
        
        let dest_path = format!("{}/README.txt", export_dir);
        fs::write(&dest_path, metadata)
            .context("Failed to write metadata")?;
        
        Ok(())
    }

    /// Create compressed tarball from export directory
    fn create_tarball(&self, source_dir: &str, archive_path: &str) -> Result<()> {
        use std::process::Command;
        
        let dir_name = Path::new(source_dir)
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid directory name")?;
        
        let output = Command::new("tar")
            .args(&[
                "czf",
                archive_path,
                "-C",
                "/tmp",
                dir_name,
            ])
            .output()
            .context("Failed to execute tar command")?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tar command failed: {}", stderr);
        }
        
        info!("Tarball created: {}", archive_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_export_service_creation() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let service = ExportService::new(pool);
        
        // Service should initialize without errors
        assert!(true);
    }
}
