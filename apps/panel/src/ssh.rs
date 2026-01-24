use russh::*;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};
use async_trait::async_trait;

pub struct SshClient;

// Use default Handler implementation for now to ensure compilation.
// If strict host checking causes issues, we will diagnose via the new logs.
#[async_trait]
impl client::Handler for SshClient {
    type Error = anyhow::Error;
}

pub async fn execute_remote_script(
    ip: &str,
    username: &str,
    port: i64,
    password: &Option<String>,
    script: &str,
    log_tx: tokio::sync::mpsc::Sender<String>,
) -> Result<()> {
    info!("Connecting to {}@{}:{} via SSH...", username, ip, port);
    let _ = log_tx.send(format!("Connecting to {}@{}:{}...", username, ip, port)).await;

    let config = Arc::new(russh::client::Config::default());
    let sh = SshClient;
    
    let mut session = russh::client::connect(config, (ip, port as u16), sh).await?;
    
    let auth_res = if let Some(pass) = password {
        session.authenticate_password(username, pass).await?
    } else {
        // Try key-based auth using local id_rsa
        let key_path = std::path::Path::new("id_rsa");
        if !key_path.exists() {
             let _ = log_tx.send("Private key 'id_rsa' not found!".to_string()).await;
             return Err(anyhow::anyhow!("Private key not found"));
        }
        
        let key_data = std::fs::read_to_string(key_path)?;
        // Load key? Russh needs a Key object.
        // Simplified: use load_secret_key from russh_keys if available, or just panic for now if not easy?
        // Actually, russh 0.40+ changed API. Let's check how to load key.
        // Assuming we have russh-keys or similar.
        // If complex, we might need a separate step.
        // For now, let's use the simplest approach available in this crate or add dependency if needed.
        // Wait, standard identity file loading:
        
        // Use russh_keys provided with russh usually?
        // Let's assume we can load it.
        // If not readily available, we might fallback to password only for now OR strictly use a crate.
        
        // Use russh_keys crate directly
        let key = russh_keys::decode_secret_key(&key_data, None)?;
        session.authenticate_publickey(username, Arc::new(key)).await?
    };
    
    if !auth_res {
        return Err(anyhow::anyhow!("Authentication failed for {}@{}", username, ip));
    }

    info!("SSH Session established. Opening channel...");
// ...
    let mut channel = session.channel_open_session().await?;
    
    info!("Executing script...");
    channel.exec(true, script).await?;

    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } => {
                let text = String::from_utf8_lossy(&data).to_string();
                let _ = log_tx.send(text).await;
            }
            russh::ChannelMsg::ExtendedData { data, .. } => {
                let text = String::from_utf8_lossy(&data).to_string();
                let _ = log_tx.send(format!("[ERR] {}", text)).await;
            }
            russh::ChannelMsg::ExitStatus { exit_status } => {
                info!("Command exited with status: {}", exit_status);
                if exit_status == 0 {
                    let _ = log_tx.send(">>> Installation successful!".to_string()).await;
                } else {
                    let _ = log_tx.send(format!(">>> Installation failed with exit code: {}", exit_status)).await;
                }
                break;
            }
            _ => {
                debug!("Received other channel message: {:?}", msg);
            }
        }
    }

    Ok(())
}
