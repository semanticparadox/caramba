#[cfg(test)]
mod tests {
    use caramba_db::models::network::Inbound;
    use crate::singbox::subscription_generator::{NodeInfo, UserKeys, generate_singbox_config};
    use serde_json::json;

    // Helper to create a dummy subscription
    fn match_any_sub() -> caramba_db::models::store::Subscription {
         serde_json::from_value(json!({
             "id": 1,
             "user_id": 1,
             "plan_id": 1,
             "status": "active",
             "created_at": "2023-01-01T00:00:00Z",
             "updated_at": "2023-01-01T00:00:00Z",
             "expires_at": "2024-01-01T00:00:00Z",
             "used_traffic": 0,
             "is_trial": false,
             "subscription_uuid": "uuid",
         })).expect("Failed to create mock subscription")
    }

    fn create_mock_node(inbound_protocol: &str, stream_settings: serde_json::Value) -> NodeInfo {
        let inbound = Inbound {
            id: 1,
            node_id: 1,
            tag: "test_inbound".to_string(),
            protocol: inbound_protocol.to_string(),
            listen_port: 8443, // New port
            listen_ip: "0.0.0.0".to_string(),
            settings: "{}".to_string(),
            stream_settings: stream_settings.to_string(),
            remark: Some("Test".to_string()),
            enable: true,
            last_rotated_at: None,
            created_at: None,
        };

        NodeInfo {
            name: "TestNode".to_string(),
            address: "1.2.3.4".to_string(),
            reality_port: Some(443),
            reality_sni: Some("google.com".to_string()),
            reality_public_key: Some("pubkey".to_string()),
            reality_short_id: Some("shortid".to_string()),
            hy2_port: Some(8443),
            hy2_sni: Some("google.com".to_string()),
            frontend_url: None, 
            inbounds: vec![inbound],
            relay_info: None,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        }
    }

    #[test]
    fn test_vless_reality_vision_flow() {
        let user_keys = UserKeys {
            user_uuid: "uuid-123".to_string(),
            hy2_password: "pass".to_string(),
            _awg_private_key: None,
        };

        // Configuration matching the user's setup (Reality TCP)
        let stream_settings = json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "serverNames": ["www.microsoft.com"],
                "publicKey": "pubkey",
                "shortIds": ["shortid"]
            }
        });

        let node = create_mock_node("vless", stream_settings);
        
        // Generate Sing-box config
        let json_config = generate_singbox_config(&match_any_sub(), &[node.clone()], &user_keys).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_config).unwrap();
        
        let outbound = parsed["outbounds"].as_array().unwrap().iter()
            .find(|o| o["tag"] == "TestNode_test_inbound")
            .expect("Outbound not found");

        println!("Generated Outbound: {}", serde_json::to_string_pretty(outbound).unwrap());

        // Check if flow is present and correct
        // Expectation: "flow": "xtls-rprx-vision"
        if let Some(flow) = outbound.get("flow") {
            assert_eq!(flow.as_str().unwrap(), "xtls-rprx-vision", "Flow should be vision");
        } else {
            panic!("Flow field is missing! This is the bug.");
        }
    }
    #[test]
    fn verify_singbox_config_structure() {
        let user_keys = UserKeys {
            user_uuid: "uuid-123".to_string(),
            hy2_password: "pass".to_string(),
            _awg_private_key: None,
        };

        // Configuration matching the user's setup (Reality TCP)
        let stream_settings = json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "serverNames": ["www.microsoft.com"],
                "publicKey": "pubkey",
                "shortIds": ["shortid"]
            }
        });

        let node_info = create_mock_node("vless", stream_settings);
        
        // Generate Sing-box config
        let json_config = generate_singbox_config(&match_any_sub(), &[node_info], &user_keys).unwrap();
        let config: serde_json::Value = serde_json::from_str(&json_config).unwrap();

        // Check for basic structure
        assert!(config.get("dns").is_some());
        assert!(config.get("route").is_some());
        assert!(config.get("inbounds").is_some());
        
        // precise outbound checks
        let outbounds = config["outbounds"].as_array().unwrap();
        
        let has_selector = outbounds.iter().any(|o| o["type"] == "selector" && o["tag"] == "proxy");
        let has_urltest = outbounds.iter().any(|o| o["type"] == "urltest" && o["tag"] == "auto");
        let has_mixed_in = config["inbounds"].as_array().unwrap().iter().any(|i| i["type"] == "mixed" && i["tag"] == "mixed-in");

        assert!(has_selector, "Missing Proxy Selector group");
        assert!(has_urltest, "Missing Auto URLTest group");
        assert!(has_mixed_in, "Missing Mixed Inbound for client");
        
        println!("Sing-box config structure verified successfully.");
    }
}
