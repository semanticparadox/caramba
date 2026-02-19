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
