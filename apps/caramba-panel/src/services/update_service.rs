use crate::settings::SettingsService;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct UpdateService {
    settings: Arc<SettingsService>,
}

impl UpdateService {
    pub fn new(settings: Arc<SettingsService>) -> Self {
        Self { settings }
    }

    /// Checks for a new agent binary in downloads/ and updates settings if found.
    pub async fn initialize_agent_updates(&self) {
        // Initialize Default Settings
        let _ = self.settings.get_or_set("auto_update_panel", "false").await;
        let _ = self.settings.get_or_set("auto_update_agents", "true").await;
        let _ = self
            .settings
            .get_or_set("auto_update_frontend", "false")
            .await;

        info!("🔄 Checking for local agent updates...");

        let download_dir = Path::new("apps/caramba-panel/downloads");
        let version_file = download_dir.join("agent_version.txt");
        let binary_file = download_dir.join("caramba-node-linux-amd64");

        if !version_file.exists() || !binary_file.exists() {
            warn!(
                "⚠️ Agent update files not found in {:?}. Skipping auto-update initialization.",
                download_dir
            );
            return;
        }

        // 1. Read Version
        let version = match tokio::fs::read_to_string(&version_file).await {
            Ok(v) => v.trim().to_string(),
            Err(e) => {
                error!("Failed to read agent version file: {}", e);
                return;
            }
        };

        // 2. Read Binary & Calculate Hash
        let hash = match self.calculate_hash(&binary_file).await {
            Ok(h) => h,
            Err(e) => {
                error!("Failed to calculate agent binary hash: {}", e);
                return;
            }
        };

        info!("📦 Found Agent v{} (Hash: {})", version, hash);

        // 3. Update Settings
        let current_stored_version = self
            .settings
            .get_or_default("agent_latest_version", "0.0.0")
            .await;

        if version != current_stored_version {
            info!(
                "🚀 New agent version detected! Updating settings from {} to {}",
                current_stored_version, version
            );

            let _ = self.settings.set("agent_latest_version", &version).await;
            let _ = self.settings.set("agent_update_hash", &hash).await;

            let _ = self
                .settings
                .set("agent_update_url", "/downloads/caramba-node-linux-amd64")
                .await;
        } else {
            // Ensure hash/url are set even if version matches (idempotency)
            let _ = self.settings.set("agent_update_hash", &hash).await;
            let _ = self
                .settings
                .set("agent_update_url", "/downloads/caramba-node-linux-amd64")
                .await;
        }
    }

    async fn calculate_hash(&self, path: &Path) -> anyhow::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 1024 * 16]; // 16KB buffer

        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }
}
