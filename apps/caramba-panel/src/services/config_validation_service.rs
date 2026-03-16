use caramba_db::models::network::{
    AmneziaWgSettings, Hysteria2Settings, NaiveSettings, ShadowsocksSettings, StreamSettings,
    TrojanSettings, TuicSettings, VlessSettings,
};
use jsonschema::{Validator, draft7};
use serde_json::{Value, json};
use std::sync::LazyLock;

static STREAM_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "network": {
                "type": "string",
                "enum": ["tcp", "udp", "quic", "grpc", "ws", "httpupgrade", "xhttp"]
            },
            "security": {
                "type": "string",
                "enum": ["none", "tls", "reality"]
            },
            "tlsSettings": {
                "type": "object",
                "properties": {
                    "serverName": { "type": "string" },
                    "certificates": { "type": "array" }
                },
                "required": ["serverName"]
            },
            "realitySettings": {
                "type": "object",
                "properties": {
                    "dest": { "type": "string" },
                    "serverNames": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "privateKey": { "type": "string" },
                    "shortIds": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["dest"]
            },
            "wsSettings": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "headers": { "type": "object" }
                },
                "required": ["path"]
            },
            "httpUpgradeSettings": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "host": { "type": ["string", "null"] }
                },
                "required": ["path"]
            },
            "xhttpSettings": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "host": { "type": "string" }
                },
                "required": ["path", "host"]
            }
        }
    }))
    .expect("stream settings schema must compile")
});

static VLESS_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "clients": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "flow": { "type": "string" },
                        "email": { "type": "string" }
                    },
                    "required": ["id", "flow", "email"]
                }
            },
            "decryption": { "type": "string" },
            "fallbacks": { "type": ["array", "null"] }
        },
        "required": ["clients", "decryption"]
    }))
    .expect("vless schema must compile")
});

static HYSTERIA2_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "users": { "type": "array", "minItems": 1 },
            "up_mbps": { "type": "integer" },
            "down_mbps": { "type": "integer" },
            "obfs": {
                "type": ["object", "null"],
                "properties": {
                    "type": { "type": "string" },
                    "password": { "type": "string" }
                },
                "required": ["type", "password"]
            },
            "masquerade": { "type": ["string", "null"] }
        },
        "required": ["users"]
    }))
    .expect("hysteria2 schema must compile")
});

static TROJAN_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "clients": { "type": "array", "minItems": 1 },
            "fallback": { "type": ["object", "null"] }
        },
        "required": ["clients"]
    }))
    .expect("trojan schema must compile")
});

static TUIC_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "users": { "type": "array", "minItems": 1 },
            "congestion_control": { "type": "string" },
            "auth_timeout": { "type": "string" },
            "zero_rtt_handshake": { "type": "boolean" },
            "heartbeat": { "type": "string" }
        },
        "required": ["users"]
    }))
    .expect("tuic schema must compile")
});

static SHADOWSOCKS_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "method": { "type": "string" },
            "users": { "type": "array", "minItems": 1 }
        },
        "required": ["method", "users"]
    }))
    .expect("shadowsocks schema must compile")
});

static NAIVE_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "users": { "type": "array", "minItems": 1 }
        },
        "required": ["users"]
    }))
    .expect("naive schema must compile")
});

static AMNEZIAWG_SETTINGS_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    draft7::new(&json!({
        "type": "object",
        "properties": {
            "users": { "type": "array", "minItems": 1 },
            "private_key": { "type": "string" },
            "public_key": { "type": "string" },
            "listen_port": { "type": "integer" }
        },
        "required": ["users", "private_key", "public_key", "listen_port"]
    }))
    .expect("amneziawg schema must compile")
});

pub struct ConfigValidationService;

impl ConfigValidationService {
    pub fn validate_inbound_json(
        protocol: &str,
        settings_json: &str,
        stream_settings_json: &str,
    ) -> Result<(), String> {
        let settings = parse_object_json(settings_json, "settings")?;
        let stream_settings = parse_object_json(stream_settings_json, "stream settings")?;

        validate_schema(
            &stream_settings,
            &STREAM_SETTINGS_VALIDATOR,
            "stream settings",
        )?;
        validate_domain_settings(protocol, settings_json, &settings)?;

        serde_json::from_str::<StreamSettings>(stream_settings_json)
            .map_err(|error| format!("Invalid stream settings payload: {error}"))?;

        Ok(())
    }
}

fn parse_object_json(raw: &str, label: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|error| format!("Invalid {label} JSON: {error}"))?;
    if !value.is_object() {
        return Err(format!("{label} must be a JSON object"));
    }
    Ok(value)
}

fn validate_schema(value: &Value, validator: &Validator, label: &str) -> Result<(), String> {
    let errors = validator.iter_errors(value).collect::<Vec<_>>();
    if errors.is_empty() {
        return Ok(());
    }

    let messages = errors
        .iter()
        .take(3)
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!("Invalid {label} schema: {messages}"))
}

fn validate_domain_settings(protocol: &str, raw: &str, value: &Value) -> Result<(), String> {
    match protocol.trim().to_ascii_lowercase().as_str() {
        "vless" => {
            validate_schema(value, &VLESS_SETTINGS_VALIDATOR, "VLESS settings")?;
            serde_json::from_str::<VlessSettings>(raw)
                .map_err(|error| format!("Invalid VLESS settings payload: {error}"))?;
        }
        "hysteria2" => {
            validate_schema(value, &HYSTERIA2_SETTINGS_VALIDATOR, "Hysteria2 settings")?;
            serde_json::from_str::<Hysteria2Settings>(raw)
                .map_err(|error| format!("Invalid Hysteria2 settings payload: {error}"))?;
        }
        "trojan" => {
            validate_schema(value, &TROJAN_SETTINGS_VALIDATOR, "Trojan settings")?;
            serde_json::from_str::<TrojanSettings>(raw)
                .map_err(|error| format!("Invalid Trojan settings payload: {error}"))?;
        }
        "tuic" => {
            validate_schema(value, &TUIC_SETTINGS_VALIDATOR, "TUIC settings")?;
            serde_json::from_str::<TuicSettings>(raw)
                .map_err(|error| format!("Invalid TUIC settings payload: {error}"))?;
        }
        "shadowsocks" => {
            validate_schema(
                value,
                &SHADOWSOCKS_SETTINGS_VALIDATOR,
                "Shadowsocks settings",
            )?;
            serde_json::from_str::<ShadowsocksSettings>(raw)
                .map_err(|error| format!("Invalid Shadowsocks settings payload: {error}"))?;
        }
        "naive" => {
            validate_schema(value, &NAIVE_SETTINGS_VALIDATOR, "Naive settings")?;
            serde_json::from_str::<NaiveSettings>(raw)
                .map_err(|error| format!("Invalid Naive settings payload: {error}"))?;
        }
        "amneziawg" => {
            validate_schema(value, &AMNEZIAWG_SETTINGS_VALIDATOR, "AmneziaWG settings")?;
            serde_json::from_str::<AmneziaWgSettings>(raw)
                .map_err(|error| format!("Invalid AmneziaWG settings payload: {error}"))?;
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ConfigValidationService;

    #[test]
    fn rejects_non_object_stream_settings() {
        let result = ConfigValidationService::validate_inbound_json(
            "vless",
            r#"{"clients":[{"id":"uuid","flow":"xtls-rprx-vision","email":"u@u"}],"decryption":"none"}"#,
            r#"[]"#,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("stream settings must be a JSON object")
        );
    }

    #[test]
    fn rejects_vless_settings_without_clients() {
        let result = ConfigValidationService::validate_inbound_json(
            "vless",
            r#"{"decryption":"none"}"#,
            r#"{"network":"tcp","security":"reality"}"#,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("VLESS settings"));
    }

    #[test]
    fn accepts_valid_hysteria2_payload() {
        let result = ConfigValidationService::validate_inbound_json(
            "hysteria2",
            r#"{"users":[{"password":"secret"}],"up_mbps":100,"down_mbps":100}"#,
            r#"{"network":"quic","security":"tls","tlsSettings":{"serverName":"example.com"}}"#,
        );

        assert!(result.is_ok());
    }
}
