use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tracing::{info, error, warn};
use sha2::{Sha256, Digest};
use std::io::Write;

pub async fn perform_update(
    client: &reqwest::Client,
    url: &str,
    expected_hash: &str,
) -> anyhow::Result<()> {
    info!("🚀 Starting self-update from {}", url);

    // 1. Download New Binary
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Failed to download update: {}", response.status());
    }

    let bytes = response.bytes().await?;
    
    // 2. Verify Hash
    let calculated_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };

    if calculated_hash != expected_hash {
        anyhow::bail!("Checksum mismatch! Expected {}, got {}", expected_hash, calculated_hash);
    }
    
    info!("✅ Checksum verified: {}", expected_hash);

    // 3. Prepare Paths
    let current_exe = std::env::current_exe()?;
    let new_exe = current_exe.with_extension("new");
    let backup_exe = current_exe.with_extension("bak");

    info!("Current binary: {:?}", current_exe);
    info!("New binary path: {:?}", new_exe);

    // 4. Write New Binary
    {
        let mut file = std::fs::File::create(&new_exe)?;
        file.write_all(&bytes)?;
        
        // Make executable
        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o755);
        file.set_permissions(perms)?;
    }
    
    // 5. Swap Binaries (Atomic Rename)
    // Rename current -> backup
    if let Err(e) = std::fs::rename(&current_exe, &backup_exe) {
        // Just warning, maybe backup already exists or permissions issue
        warn!("Failed to create backup: {}. Proceeding carefully...", e);
    }
    
    // Rename new -> current
    if let Err(e) = std::fs::rename(&new_exe, &current_exe) {
        // Rollback backup if possible
        let _ = std::fs::rename(&backup_exe, &current_exe);
        anyhow::bail!("Failed to replace binary: {}", e);
    }

    info!("✅ Binary replaced successfully. Restarting service...");

    // 6. Restart Service
    // This assumes running as systemd service named 'exarobot-agent' OR the current process name.
    // Generally 'systemctl restart' requires root/sudoers.
    
    let output = Command::new("systemctl")
        .args(&["restart", "exarobot-agent"])
        .output();

    match output {
        Ok(out) => {
             if out.status.success() {
                 info!("Wait for restart...");
                 std::process::exit(0); // Exit cleanly, systemd will restart it if needed or just let systemctl handle it
             } else {
                 error!("Failed to restart service: {}", String::from_utf8_lossy(&out.stderr));
                 // Try to rollback binary? Too risky now. Admin intervention needed.
                 // Actually, if systemctl failed, we are still running old process but file is new.
                 // We should exit so systemd respawns the NEW file.
                 info!("Exiting process to force respawn...");
                 std::process::exit(0);
             }
        },
        Err(e) => {
            error!("Failed to execute systemctl: {}. Exiting to respawn manually.", e);
            std::process::exit(0);
        }
    }
    
    #[allow(unreachable_code)]
    Ok(())
}
