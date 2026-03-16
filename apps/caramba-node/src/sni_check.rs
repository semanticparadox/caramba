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

use std::sync::Arc;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

pub async fn check_reachability(sni: &str) -> Result<(), String> {
    let target = format!("{}:443", sni);

    let stream = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        TcpStream::connect(&target),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("❌ SNI {} failed to connect: {}", sni, e);
            return Err(format!("TCP Connect Error: {}", e));
        }
        Err(_) => {
            error!("❌ SNI {} connection timed out", sni);
            return Err("TCP Connection Timeout".to_string());
        }
    };

    // TLS Validation Layer
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));
    let server_name = match ServerName::try_from(sni.to_string()) {
        Ok(name) => name,
        Err(e) => return Err(format!("Invalid ServerName: {}", e)),
    };

    // Verify TLS Handshake with actual validation
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        connector.connect(server_name, stream),
    )
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => {
            error!("❌ SNI {} failed TLS handshake validation: {}", sni, e);
            Err(format!("TLS Handshake Error: {}", e))
        }
        Err(_) => {
            error!("❌ SNI {} TLS handshake timed out", sni);
            Err("TLS Handshake Timeout".to_string())
        }
    }
}
