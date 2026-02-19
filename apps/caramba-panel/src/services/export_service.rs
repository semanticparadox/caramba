use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// Service for exporting database and settings for backup/migration
pub struct ExportService;

impl ExportService {
    pub fn new() -> Self {
        Self
    }

    /// Create complete export archive (sanitized settings)
    /// Returns archive as bytes ready for transmission
    pub async fn create_export(&self) -> Result<Vec<u8>> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let export_dir = format!("/tmp/caramba_export_{}", timestamp);

        info!("Starting export to {}", export_dir);

        // Create temp directory
        fs::create_dir_all(&export_dir).context("Failed to create export directory")?;

        // Step 1: Export sanitized environment variables
        self.export_env_sanitized(&export_dir)?;

        // Step 2: Export metadata and instructions
        self.export_metadata(&export_dir)?;

        // Step 3: Create compressed tarball
        let archive_path = format!("{}.tar.gz", export_dir);
        self.create_tarball(&export_dir, &archive_path)?;

        // Step 4: Read archive into memory
        let data = fs::read(&archive_path).context("Failed to read archive")?;

        info!("Export complete: {} bytes", data.len());

        // Cleanup temporary files
        fs::remove_dir_all(&export_dir).ok();
        fs::remove_file(&archive_path).ok();

        Ok(data)
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
            "DATABASE_URL",
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
        fs::write(&dest_path, sanitized).context("Failed to write sanitized env")?;

        info!(
            "Sanitized .env exported ({} keys redacted)",
            sensitive_keys.len()
        );
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
- env_sanitized.txt : Configuration (sensitive keys redacted)
- README.txt        : This file

Restoration Instructions
-------------------------

Note: Database backup via VACUUM INTO is disabled for PostgreSQL.
Please use pg_dump for database backups:
pg_dump $DATABASE_URL > backup.sql

1. Extract Archive:
   tar xzf caramba_backup_*.tar.gz
   cd caramba_export_*/

2. Merge Environment:
   # Compare with your current .env
   diff env_sanitized.txt /opt/caramba/.env

3. Restart Panel:
   systemctl restart caramba
",
            chrono::Utc::now().to_rfc3339(),
            env!("CARGO_PKG_VERSION")
        );

        let dest_path = format!("{}/README.txt", export_dir);
        fs::write(&dest_path, metadata).context("Failed to write metadata")?;

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
            .args(&["czf", archive_path, "-C", "/tmp", dir_name])
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
