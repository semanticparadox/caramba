#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayAuthMode {
    Legacy,
    V1,
    Dual,
}

impl RelayAuthMode {
    pub fn from_setting(raw: Option<&str>) -> Self {
        match raw.unwrap_or("dual").trim().to_ascii_lowercase().as_str() {
            "legacy" => Self::Legacy,
            "v1" | "hashed" | "derived" => Self::V1,
            "dual" => Self::Dual,
            _ => Self::Dual,
        }
    }
}

pub fn validate_manual_json(json_str: &str) -> anyhow::Result<()> {
    let value: serde_json::Value = serde_json::from_str(json_str)?;
    if !value.is_object() {
        return Err(anyhow::anyhow!("JSON must be an object"));
    }
    Ok(())
}
