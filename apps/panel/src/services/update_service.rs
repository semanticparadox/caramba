use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error, warn};
use std::path::Path;
use sha2::{Sha256, Digest};
use std::io::Read;
use crate::settings::SettingsService;

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
        let _ = self.settings.get_or_set("auto_update_frontend", "false").await;

        info!("🔄 Checking for local agent updates...");

        let download_dir = Path::new("apps/panel/downloads");
        let version_file = download_dir.join("agent_version.txt");
        let binary_file = download_dir.join("exarobot-agent-linux-amd64");

        if !version_file.exists() || !binary_file.exists() {
            warn!("⚠️ Agent update files not found in {:?}. Skipping auto-update initialization.", download_dir);
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
        let current_stored_version = self.settings.get_or_default("agent_latest_version", "0.0.0").await;
        
        if version != current_stored_version {
            info!("🚀 New agent version detected! Updating settings from {} to {}", current_stored_version, version);
            
            // Construct download URL relative to panel
            // NOTE: We assume the panel domain is set in settings or we use a relative path if supported by agent?
            // Agent usually expects a full URL. Panel URL is known by agent.
            // If we store just the path "/downloads/exarobot-agent-linux-amd64", the agent needs to prepend panel_url.
            // Let's check agent code: `format!("{}/api/v2/node/update-info", panel_url)` -> returns JSON with "url".
            // If "url" is absolute, agent uses it.
            // If we don't know our own public domain here easily without request context, 
            // we might need to store a relative path and have the agent or the API handler resolve it.
            // FASTEST FIX: Store relative path, modify API handler to prepend panel_url if needed?
            // OR: Just store the filename and let the API handler construct the full URL based on the request host?
            // Actually, `get_update_info` in `api/v2/node.rs` simply returns what's in settings.
            
            // For now, let's assume the admin has set a `panel_url` setting, or we can try to guess.
            // Better: update the `get_update_info` handler to handle relative URLs.
            // But to be safe and simple: let's store `/downloads/exarobot-agent-linux-amd64`.
            // The AGENT `self_update.rs` does `client.get(url)`. If URL starts with `/`, reqwest might fail if no base url.
            
            // LET'S MODIFY `get_update_info` to handle this. 
            // But for now, let's push the settings.
            
            let _ = self.settings.set("agent_latest_version", &version).await;
            let _ = self.settings.set("agent_update_hash", &hash).await;
            
            // We'll set a relative path marker. 
            // The `get_update_info` handler will need to prepend the panel URL (which it knows or can be passed).
            // Wait, `get_update_info` endpoint is inside the panel.
            // It can reconstruct the URL using the `Host` header of the incoming request!
            let _ = self.settings.set("agent_update_url", "/downloads/exarobot-agent-linux-amd64").await; 
        } else {
            // Ensure hash/url are set even if version matches (idempotency)
             let _ = self.settings.set("agent_update_hash", &hash).await;
             let _ = self.settings.set("agent_update_url", "/downloads/exarobot-agent-linux-amd64").await; 
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
