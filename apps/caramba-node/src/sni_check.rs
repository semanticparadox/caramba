use serde_json::Value;
use tokio::net::TcpStream;
use tracing::error;

pub async fn get_current_sni(config_path: &str) -> Option<String> {
    // Read file
    let content = tokio::fs::read_to_string(config_path).await.ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;

    // Traverse: inbounds -> [0] -> tls -> server_name
    // Sing-box structure for VLESS/Reality usually involves `tls` object in inbound

    if let Some(inbounds) = json.get("inbounds").and_then(|v| v.as_array()) {
        for inbound in inbounds {
            if let Some(tls) = inbound.get("tls") {
                if let Some(server_name) = tls.get("server_name").and_then(|v| v.as_str()) {
                    return Some(server_name.to_string());
                }
            }
        }
    }

    None
}

pub async fn check_reachability(sni: &str) -> bool {
    let target = format!("{}:443", sni);
    // info!("🔍 Checking SNI health: {}", target);

    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        TcpStream::connect(&target),
    )
    .await
    {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            error!("❌ SNI {} failed to connect: {}", sni, e);
            false
        }
        Err(_) => {
            error!("❌ SNI {} connection timed out", sni);
            false
        }
    }
}
