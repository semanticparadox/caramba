use caramba_shared::DiscoveredSni;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::DigitallySignedStruct;
use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerifier};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tracing::info;
use x509_parser::prelude::*;

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<tokio_rustls::rustls::client::danger::ServerCertVerified, tokio_rustls::rustls::Error>
    {
        Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
        vec![
            tokio_rustls::rustls::SignatureScheme::RSA_PSS_SHA256,
            tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA256,
            tokio_rustls::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            tokio_rustls::rustls::SignatureScheme::ED25519,
        ]
    }
}

pub struct NeighborScanner {
    local_ip: IpAddr,
}

impl NeighborScanner {
    pub fn new(local_ip: IpAddr) -> Self {
        Self { local_ip }
    }

    pub async fn scan_subnet(&self) -> Vec<DiscoveredSni> {
        let mut discovered = Vec::new();
        let mut seen_domains = HashSet::new();
        let base_ip = match self.local_ip {
            IpAddr::V4(v4) => v4.octets(),
            _ => return discovered, // IPv6 not implemented for sniper yet
        };

        info!(
            "🎯 Neighbor Sniper: Starting scan on {}.{}.{}.0/24",
            base_ip[0], base_ip[1], base_ip[2]
        );

        // Scan the /24 range
        for i in 1..=254 {
            if i == base_ip[3] {
                continue;
            } // Skip self

            let target_ip = IpAddr::V4(std::net::Ipv4Addr::new(
                base_ip[0], base_ip[1], base_ip[2], i,
            ));

            // Optimization: Skip if we already found this IP recently?
            // For now, simple scan.

            if let Ok(mut sni) = self.probe_ip(target_ip).await {
                sni.domain = sni.domain.trim().to_ascii_lowercase();
                if seen_domains.insert(sni.domain.clone()) {
                    info!(
                        "✨ Neighbor Sniper: Discovered potential SNI: {} at {}",
                        sni.domain, sni.ip
                    );
                    discovered.push(sni);
                }
            }
        }

        discovered
    }

    async fn probe_ip(&self, ip: IpAddr) -> anyhow::Result<DiscoveredSni> {
        let addr = SocketAddr::new(ip, 443);
        let timeout = Duration::from_millis(800);

        // 1. TCP Connect
        let stream = tokio::time::timeout(timeout, TcpStream::connect(addr)).await??;
        let start = std::time::Instant::now();

        // 2. TLS Handshake (Insecure/Blind)
        // We use a custom verifier to accept ANY certificate, so we can see who they claim to be.

        let root_store = RootCertStore::empty();

        let mut config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        // Use dangerous configuration to disable verification
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(NoCertificateVerification));
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let connector = TlsConnector::from(Arc::new(config));
        // We use a generic name for SNI just to trigger the handshake.
        // Many servers will return their default cert if SNI doesn't match, or the cert matching the IP.
        let domain_name = ServerName::try_from("www.google.com")?.to_owned();

        // Connect
        let tls_stream =
            tokio::time::timeout(timeout, connector.connect(domain_name, stream)).await??;

        let latency = start.elapsed().as_millis() as u32;

        // 3. Extract Certificate
        let (_, session) = tls_stream.get_ref();

        // Check ALPN
        let h2 = session.alpn_protocol() == Some(b"h2");

        if let Some(certs) = session.peer_certificates() {
            if let Some(cert) = certs.first() {
                // Parse the certificate
                if let Ok(domain) = self.extract_best_domain(cert.as_ref()) {
                    return Ok(DiscoveredSni {
                        domain,
                        ip: ip.to_string(),
                        latency_ms: latency,
                        h2,
                        h3: false, // Hard to detect H3 without QUIC handshake
                    });
                }
            }
        }

        Err(anyhow::anyhow!("No valid certificate or domain found"))
    }

    fn extract_best_domain(&self, cert_der: &[u8]) -> anyhow::Result<String> {
        let (_, cert) = X509Certificate::from_der(cert_der)
            .map_err(|e| anyhow::anyhow!("Cert parse error: {:?}", e))?;

        // 1. Try Subject Alternative Names (SANs) - DNS
        if let Ok(Some(sans)) = cert.subject_alternative_name() {
            for entry in sans.value.general_names.iter() {
                if let GeneralName::DNSName(dns) = entry {
                    let dns_str = dns.to_string();
                    if self.is_valid_public_domain(&dns_str) {
                        return Ok(dns_str);
                    }
                }
            }
        }

        // 2. Fallback to Subject Common Name (CN)
        if let Some(subject) = cert.subject().iter_common_name().next() {
            if let Ok(cn_str) = subject.as_str() {
                let cn = cn_str.to_string();
                if self.is_valid_public_domain(&cn) {
                    return Ok(cn);
                }
            }
        }

        Err(anyhow::anyhow!("No valid public domain found in cert"))
    }

    fn is_valid_public_domain(&self, domain: &str) -> bool {
        let domain = domain.trim().to_ascii_lowercase();

        if domain.len() < 4 || domain.len() > 120 {
            return false;
        }
        if !domain.contains('.') {
            return false;
        }
        if domain.contains(' ') || domain.contains('_') {
            return false;
        }
        if domain.contains('*') {
            return false;
        } // Wildcards are good for matching, but we want a concrete host for Reality? actually wildcards are fine for SNI usually but Reality prefers concrete. Let's skip wildcards for now to be safe.
        // Or specific logic: we want a realistic "stealable" domain.
        if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
            return false;
        }

        if !domain
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
        {
            return false;
        }

        // Exclude IPs
        if domain.parse::<IpAddr>().is_ok() {
            return false;
        }

        const DENY_SUBSTRINGS: &[&str] = &[
            "localhost",
            "traefik",
            "plesk",
            "parallels",
            "easypanel",
            "directadmin",
            "cpanel",
            "access-denied",
            "access denied",
            "forbidden",
            "sni-support-required",
        ];
        if DENY_SUBSTRINGS.iter().any(|needle| domain.contains(needle)) {
            return false;
        }

        const DENY_SUFFIXES: &[&str] = &[
            ".local",
            ".localdomain",
            ".internal",
            ".lan",
            ".invalid",
            ".example",
            ".test",
            ".home.arpa",
            ".traefik.default",
            ".plesk.page",
            ".vps.ovh.net",
        ];
        if DENY_SUFFIXES.iter().any(|suffix| domain.ends_with(suffix)) {
            return false;
        }

        let labels: Vec<&str> = domain.split('.').collect();
        if labels.len() < 2 || labels.len() > 8 {
            return false;
        }

        for label in &labels {
            if label.is_empty() || label.len() > 63 {
                return false;
            }
            if label.starts_with('-') || label.ends_with('-') {
                return false;
            }

            // Drop machine-generated high-entropy labels typical for infra noise.
            if label.len() >= 24
                && label
                    .chars()
                    .all(|ch| ch.is_ascii_hexdigit() || ch.is_ascii_digit())
            {
                return false;
            }
        }

        let tld = labels.last().copied().unwrap_or_default();
        const RESERVED_TLDS: &[&str] = &[
            "local", "internal", "lan", "invalid", "example", "test", "default",
        ];
        if RESERVED_TLDS.contains(&tld) {
            return false;
        }
        if tld.len() < 2 {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::NeighborScanner;
    use std::net::{IpAddr, Ipv4Addr};

    fn scanner() -> NeighborScanner {
        NeighborScanner::new(IpAddr::V4(Ipv4Addr::new(137, 74, 119, 200)))
    }

    #[test]
    fn accepts_normal_public_domains() {
        let sc = scanner();
        assert!(sc.is_valid_public_domain("hitchhive.app"));
        assert!(sc.is_valid_public_domain("api.kd367.fr"));
    }

    #[test]
    fn rejects_control_plane_noise() {
        let sc = scanner();
        assert!(!sc.is_valid_public_domain("Plesk"));
        assert!(!sc.is_valid_public_domain("Parallels Panel"));
        assert!(!sc.is_valid_public_domain("traefik.default"));
        assert!(!sc.is_valid_public_domain("vps-40d02f7d.vps.ovh.net"));
        assert!(!sc.is_valid_public_domain("sni-support-required-for-valid-ssl"));
    }

    #[test]
    fn rejects_malformed_or_generated_domains() {
        let sc = scanner();
        assert!(!sc.is_valid_public_domain("with space.example.com"));
        assert!(!sc.is_valid_public_domain("localhost"));
        assert!(!sc.is_valid_public_domain(
            "9549ca1c6e517b1f5f8db4e7624e0916.f7021182bba21dbfeaa0c9111f25c92d.traefik.default"
        ));
    }
}
