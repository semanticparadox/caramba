//! Centralized, panel-managed sing-box node policy.
//!
//! A [`ConfigPolicy`] is resolved per node (node override -> group -> default)
//! by `services::profile_service` and fed into the config generator. The
//! *default* policy intentionally produces a configuration that is byte-for-byte
//! identical to the historical hard-coded output, so unassigned nodes are never
//! disrupted.
//!
//! The policy is persisted as the JSON `policy` column of `config_profiles`.

use serde::{Deserialize, Serialize};

/// Full per-node configuration policy. Every field is optional so an empty
/// `{}` document deserializes into a no-op policy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigPolicy {
    /// DNS policy. `None` => keep the legacy DNS block untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsPolicy>,

    /// Log level override (`trace|debug|info|warn|error|fatal|panic`).
    /// `None` => keep the default `info`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
}

/// DNS resolution mode selected for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DnsMode {
    /// Resolve via the node's own system/local resolver only.
    #[default]
    System,
    /// DNS-over-HTTPS to an upstream resolver (IP literal recommended).
    Doh,
    /// DNS-over-TLS to an upstream resolver (IP literal recommended).
    Dot,
}

/// DNS sub-policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsPolicy {
    #[serde(default)]
    pub mode: DnsMode,

    /// Upstream resolver. Accepts a bare IP (`1.1.1.1`), a DoH URL
    /// (`https://1.1.1.1/dns-query`) or a DoT URL (`tls://1.1.1.1`).
    /// An IP literal is strongly recommended to avoid a bootstrap loop.
    #[serde(default)]
    pub upstream: String,

    /// Explicit upstream port. Overrides any port parsed from `upstream`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,

    /// Explicit DoH path. Overrides any path parsed from `upstream`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Answer strategy: `prefer_ipv4|prefer_ipv6|ipv4_only|ipv6_only`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,

    /// Resolve Russian domains (`geosite-ru`) via the local resolver to keep
    /// RU CDNs geographically correct and low-latency.
    #[serde(default)]
    pub ru_direct: bool,
}

impl Default for DnsPolicy {
    fn default() -> Self {
        Self {
            mode: DnsMode::System,
            upstream: String::new(),
            server_port: None,
            path: None,
            strategy: None,
            ru_direct: false,
        }
    }
}

impl DnsPolicy {
    /// `upstream` with any known scheme prefix stripped.
    fn stripped(&self) -> &str {
        let u = self.upstream.trim();
        for p in ["https://", "tls://", "tcp://", "udp://", "quic://", "h3://"] {
            if let Some(rest) = u.strip_prefix(p) {
                return rest;
            }
        }
        u
    }

    /// Bare host/IP of the upstream (scheme, path and port removed).
    /// Falls back to a sane public resolver if `upstream` is empty.
    pub fn server_host(&self) -> String {
        let s = self.stripped();
        let host_port = s.split('/').next().unwrap_or(s);
        let host = match host_port.rsplit_once(':') {
            // Strip a trailing `:port` only for IPv4/hostname forms.
            Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => h,
            _ => host_port,
        };
        if host.is_empty() {
            "1.1.1.1".to_string()
        } else {
            host.to_string()
        }
    }

    /// DoH request path (`/dns-query` by default).
    pub fn doh_path(&self) -> String {
        if let Some(p) = &self.path {
            let p = p.trim();
            if !p.is_empty() {
                return if p.starts_with('/') {
                    p.to_string()
                } else {
                    format!("/{p}")
                };
            }
        }
        let s = self.stripped();
        if let Some(idx) = s.find('/') {
            let path = &s[idx..];
            if path.len() > 1 {
                return path.to_string();
            }
        }
        "/dns-query".to_string()
    }

    /// Explicit upstream port, from `server_port` or parsed from `upstream`.
    pub fn explicit_port(&self) -> Option<u16> {
        if let Some(p) = self.server_port {
            return Some(p);
        }
        let s = self.stripped();
        let host_port = s.split('/').next().unwrap_or(s);
        if let Some((h, p)) = host_port.rsplit_once(':')
            && !h.is_empty()
            && let Ok(n) = p.parse::<u16>()
        {
            return Some(n);
        }
        None
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn empty_json_is_noop_default() {
        let p: ConfigPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(p, ConfigPolicy::default());
        assert!(p.dns.is_none());
        assert!(p.log_level.is_none());
    }

    #[test]
    fn parses_bare_ip() {
        let d = DnsPolicy {
            mode: DnsMode::Doh,
            upstream: "1.1.1.1".to_string(),
            ..Default::default()
        };
        assert_eq!(d.server_host(), "1.1.1.1");
        assert_eq!(d.doh_path(), "/dns-query");
        assert_eq!(d.explicit_port(), None);
    }

    #[test]
    fn parses_doh_url() {
        let d = DnsPolicy {
            mode: DnsMode::Doh,
            upstream: "https://9.9.9.9/dns-query".to_string(),
            ..Default::default()
        };
        assert_eq!(d.server_host(), "9.9.9.9");
        assert_eq!(d.doh_path(), "/dns-query");
    }

    #[test]
    fn parses_dot_url_with_port() {
        let d = DnsPolicy {
            mode: DnsMode::Dot,
            upstream: "tls://1.1.1.1:8853".to_string(),
            ..Default::default()
        };
        assert_eq!(d.server_host(), "1.1.1.1");
        assert_eq!(d.explicit_port(), Some(8853));
    }
}
