use askama::Result;

// Helper for Rust code (non-template usage)

pub fn format_bytes_str(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// Helper for Askama templates (must match filter signature)
#[allow(dead_code)]
pub fn format_bytes(s: &i64) -> Result<String> {
    Ok(format_bytes_str(*s as u64))
}

/// Feature flag: AmneziaWG support.
///
/// Disabled by default. Official sing-box (installed from deb.sagernet.org) has
/// NO `wireguard` INBOUND type and does not understand AmneziaWG obfuscation
/// fields, so an AmneziaWG inbound makes `sing-box check` FAIL and takes down the
/// whole node config. Until nodes ship an AmneziaWG-capable sing-box fork, the
/// protocol is hidden: creation is rejected and it is omitted from subscriptions.
///
/// Set `CARAMBA_ENABLE_AMNEZIAWG=1` (or `true`/`yes`/`on`) to re-enable.
pub fn amneziawg_enabled() -> bool {
    matches!(
        std::env::var("CARAMBA_ENABLE_AMNEZIAWG")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// Feature flag: AmneziaWG in the mihomo/clash subscription (the Go client core).
///
/// This is SEPARATE from `amneziawg_enabled()` on purpose. `amneziawg_enabled()`
/// gates the sing-box NODE path: stock sing-box has no `wireguard` inbound, so
/// emitting an AmneziaWG inbound breaks `sing-box check` and the whole node. The
/// mihomo CLIENT, by contrast, speaks AmneziaWG natively and can consume a
/// `wireguard` proxy with the `amnezia-wg-option` block — provided a real
/// AmneziaWG-capable WireGuard server is actually listening on the node (plain
/// sing-box cannot serve it; deployment needs an AmneziaWG-capable sing-box fork).
///
/// Decoupling lets an operator hand a working AmneziaWG proxy to the mihomo
/// client WITHOUT also un-gating the node-breaking sing-box inbound. The clash
/// emission turns on when EITHER flag is set:
///   - `CARAMBA_ENABLE_AMNEZIAWG=1`        — legacy combined switch (node + client),
///   - `CARAMBA_ENABLE_AMNEZIAWG_CLIENT=1` — client/mihomo emission only.
///
/// Safety: this flag only adds a proxy to the clash subscription. It never causes
/// an AmneziaWG inbound to be written to a sing-box node config, so a node that
/// cannot serve AmneziaWG is never put into a failing state by it. The proxy is
/// inert unless a matching AmneziaWG server is running on the node.
pub fn amneziawg_client_enabled() -> bool {
    if amneziawg_enabled() {
        return true;
    }
    matches!(
        std::env::var("CARAMBA_ENABLE_AMNEZIAWG_CLIENT")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

// Askama filters are functions.
// I can define `format_bytes_i64` or just expect i64 since DB uses i64.

pub fn current_panel_version() -> String {
    if let Ok(v) = std::env::var("CARAMBA_VERSION") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    for path in [
        "/opt/caramba/.caramba-version",
        "/opt/caramba/VERSION",
        ".caramba-version",
    ] {
        if let Ok(raw) = std::fs::read_to_string(path) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    let cargo_version = env!("CARGO_PKG_VERSION");
    if cargo_version.starts_with('v') {
        cargo_version.to_string()
    } else {
        format!("v{}", cargo_version)
    }
}
