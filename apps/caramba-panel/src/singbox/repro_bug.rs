/// Тесты воспроизведения сломанного конфига и верификации корректной структуры.
///
/// Сломанный конфиг, показанный пользователем, содержал:
///   - пустые inbounds
///   - теги вида "01 - Reality (Direct)"
///   - отсутствие блока tls.reality для VLESS
///   - отсутствие flow для Reality TCP
///   - пустые route.rules
///   - отсутствие секции dns
///
/// Этот формат НЕ производится ни одной из текущих функций генерации.
/// generate_singbox_config() производит корректный формат — тесты ниже это доказывают.
/// Вероятная причина показа сломанного конфига: устаревший кеш Redis (ключ sub_config_v2 или
/// раннее написанный под старый генератор ключ sub_config_v3) — нужно flush Redis для UUID.
#[cfg(test)]
mod tests {
    use caramba_db::models::network::Inbound;
    use crate::singbox::subscription_generator::{NodeInfo, UserKeys, generate_singbox_config};
    use serde_json::json;

    fn mock_sub() -> caramba_db::models::store::Subscription {
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
            "subscription_uuid": "test-sub-uuid-0001",
            "vless_uuid": "test-vless-uuid-001",
            "node_id": null,
            "auto_renew": false,
            "alerts_sent": "[]",
            "note": null,
            "traffic_updated_at": null,
            "last_sub_access": null
        })).expect("Failed to deserialize mock subscription")
    }

    fn mock_inbound(protocol: &str, tag: &str, port: i64, stream_settings: serde_json::Value) -> Inbound {
        Inbound {
            id: 1,
            node_id: 1,
            tag: tag.to_string(),
            protocol: protocol.to_string(),
            listen_port: port,
            listen_ip: "0.0.0.0".to_string(),
            settings: "{}".to_string(),
            stream_settings: stream_settings.to_string(),
            remark: Some(format!("{}-{}", protocol, tag)),
            enable: true,
            renew_interval_mins: 0,
            port_range_start: 10000,
            port_range_end: 60000,
            last_rotated_at: None,
            created_at: None,
        }
    }

    fn mock_keys() -> UserKeys {
        UserKeys {
            user_uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
            hy2_password: "12345678:aaaaaaaa".to_string(),
            _awg_private_key: None,
        }
    }

    fn mock_node(name: &str, inbounds: Vec<Inbound>) -> NodeInfo {
        NodeInfo {
            name: name.to_string(),
            address: "1.2.3.4".to_string(),
            reality_port: Some(443),
            reality_sni: Some("timecard365.de".to_string()),
            reality_public_key: Some("test_pub_key_base64".to_string()),
            reality_short_id: Some("abcd1234".to_string()),
            hy2_port: None,
            hy2_sni: None,
            frontend_url: None,
            inbounds,
            relay_info: None,
            country_code: Some("DE".to_string()),
            is_relay: false,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        }
    }

    // ─── Тест 1: Базовая структура конфига ────────────────────────────────────

    #[test]
    fn config_has_required_top_level_sections() {
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "serverNames": ["timecard365.de"],
                "publicKey": "test_pub_key_base64",
                "shortIds": ["abcd1234"]
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        // Верхнеуровневые секции обязаны присутствовать
        assert!(config.get("log").is_some(), "Отсутствует секция log");
        assert!(config.get("dns").is_some(), "Отсутствует секция dns");
        assert!(config.get("inbounds").is_some(), "Отсутствует секция inbounds");
        assert!(config.get("outbounds").is_some(), "Отсутствует секция outbounds");
        assert!(config.get("route").is_some(), "Отсутствует секция route");
        assert!(config.get("experimental").is_some(), "Отсутствует секция experimental");
    }

    // ─── Тест 2: Клиентские inbounds (mixed + tun) ────────────────────────────

    #[test]
    fn config_has_mixed_and_tun_client_inbounds() {
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "serverNames": ["timecard365.de"],
                "publicKey": "pub",
                "shortIds": ["sid"]
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let inbounds = config["inbounds"].as_array().expect("inbounds не массив");
        assert!(!inbounds.is_empty(), "inbounds пустой — клиентские порты отсутствуют");

        let has_mixed = inbounds.iter().any(|i| i["type"] == "mixed" && i["tag"] == "mixed-in");
        let has_tun = inbounds.iter().any(|i| i["type"] == "tun" && i["tag"] == "tun-in");

        assert!(has_mixed, "Отсутствует mixed inbound (порт 2080)");
        assert!(has_tun, "Отсутствует tun inbound для прозрачного проксирования");
    }

    // ─── Тест 3: Route rules не пустые ────────────────────────────────────────

    #[test]
    fn route_rules_are_not_empty() {
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "serverNames": ["timecard365.de"],
                "publicKey": "pub",
                "shortIds": ["sid"]
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let rules = config["route"]["rules"].as_array().expect("route.rules не массив");
        assert!(!rules.is_empty(), "route.rules пустой — весь трафик будет идти напрямую");

        // Проверяем наличие ключевых правил
        let has_dns_rule = rules.iter().any(|r| r["protocol"] == "dns");
        let has_private_rule = rules.iter().any(|r| r["ip_is_private"] == true);
        assert!(has_dns_rule, "Отсутствует DNS-правило в route.rules");
        assert!(has_private_rule, "Отсутствует правило для приватных IP");
    }

    // ─── Тест 4: VLESS+Reality+TCP — наличие блока reality и flow ─────────────
    //
    // ВАЖНО: поле `dest` в realitySettings обязательно для десериализации RealitySettings.
    // Если оно отсутствует, serde вернёт ошибку и stream_settings не распарсятся —
    // в этом случае public_key берётся из node.reality_public_key (node-level fallback).
    // В тесте мы используем node-level ключи для проверки корректного поведения fallback.
    #[test]
    fn vless_reality_tcp_has_reality_block_and_vision_flow() {
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "fingerprint": "chrome",
                "show": false,
                "dest": "timecard365.de:443",
                "serverNames": ["timecard365.de"],
                "publicKey": "RealPubKey123",
                "shortIds": ["shortid01"]
            }
        }));
        // mock_node устанавливает reality_public_key = "test_pub_key_base64" и short_id = "abcd1234"
        // Если realitySettings.publicKey корректно распарсен — будет "RealPubKey123".
        // Если нет (например, отсутствует обязательное поле) — fallback на node-level.
        // Добавляем dest, чтобы serde мог полностью распарсить RealitySettings.
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();

        // Тег должен быть в формате "{flag} {proto_label}", а НЕ "01 - Reality (Direct)"
        let vless_ob = outbounds.iter()
            .find(|o| o["type"] == "vless" && o["tag"].as_str() == Some("🇩🇪 Stealth"))
            .expect("VLESS outbound с тегом '🇩🇪 Stealth' не найден");

        // flow обязателен для Reality+TCP (xtls-rprx-vision)
        let flow = vless_ob["flow"].as_str().unwrap_or("");
        assert_eq!(flow, "xtls-rprx-vision", "Отсутствует или неверный flow для VLESS Reality TCP");

        // tls.reality блок обязан быть
        let reality_block = &vless_ob["tls"]["reality"];
        assert!(reality_block.is_object(), "Отсутствует tls.reality блок");
        assert_eq!(reality_block["enabled"], true, "tls.reality.enabled != true");
        assert_eq!(vless_ob["tls"]["enabled"], true, "tls.enabled != true");

        // public_key: когда realitySettings полностью распарсен (с dest) — используется
        // inbound-level publicKey "RealPubKey123". Это приоритетная ветка.
        assert_eq!(
            vless_ob["tls"]["reality"]["public_key"].as_str().unwrap_or(""),
            "RealPubKey123",
            "Неверный public_key в reality блоке (ожидается значение из inbound stream_settings)"
        );
        assert_eq!(
            vless_ob["tls"]["reality"]["short_id"].as_str().unwrap_or(""),
            "shortid01",
            "Неверный short_id в reality блоке"
        );

    }

    // ─── Тест 4b: Fallback на node-level ключи когда realitySettings без dest ─

    #[test]
    fn vless_reality_falls_back_to_node_level_keys_when_dest_missing() {
        // realitySettings без обязательного поля dest — serde fail → fallback на node-level
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "serverNames": ["timecard365.de"],
                "publicKey": "INBOUND_LEVEL_KEY",
                "shortIds": ["inbound_sid"]
                // dest отсутствует → RealitySettings десериализация упадёт
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        // mock_node: reality_public_key = "test_pub_key_base64", short_id = "abcd1234"
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();
        let vless_ob = outbounds.iter()
            .find(|o| o["type"] == "vless" && o["tag"].as_str() == Some("🇩🇪 Stealth"))
            .expect("VLESS outbound не найден");

        // Должен использоваться node-level public_key
        assert_eq!(
            vless_ob["tls"]["reality"]["public_key"].as_str().unwrap_or(""),
            "test_pub_key_base64",
            "Без dest в realitySettings должен использоваться node-level public_key как fallback"
        );

        // flow всё равно должен быть (определяется по security == reality, не по realitySettings struct)
        assert_eq!(vless_ob["flow"].as_str().unwrap_or(""), "xtls-rprx-vision",
            "flow должен быть даже при fallback на node-level ключи");
    }

    // ─── Тест 5: VLESS+gRPC+TLS — НЕТ блока reality, НЕТ flow ───────────────

    #[test]
    fn vless_grpc_tls_has_no_reality_block_and_no_flow() {
        let inbound = mock_inbound("vless", "grpc_tls", 10400, json!({
            "network": "grpc",
            "security": "tls",
            "tlsSettings": {
                "serverName": "timecard365.de"
            },
            "grpcSettings": {
                "serviceName": "grpc-service"
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();
        let vless_ob = outbounds.iter()
            .find(|o| o["type"] == "vless" && o["tag"].as_str() == Some("🇩🇪 Stream"))
            .expect("VLESS gRPC outbound не найден");

        // gRPC не должен иметь flow (не поддерживается с Vision)
        let flow = vless_ob["flow"].as_str().unwrap_or("");
        assert!(flow.is_empty(), "gRPC inbound не должен иметь flow: got '{}'", flow);

        // Не должно быть reality блока
        assert!(vless_ob["tls"]["reality"].is_null(), "gRPC TLS не должен иметь reality блок");

        // TLS enabled
        assert_eq!(vless_ob["tls"]["enabled"], true, "TLS должен быть включён для gRPC TLS");

        // transport должен быть grpc
        assert_eq!(vless_ob["transport"]["type"], "grpc", "transport.type должен быть grpc");
    }

    // ─── Тест 6: Hysteria2 — insecure и alpn обязательны ─────────────────────

    #[test]
    fn hysteria2_has_insecure_and_alpn_h3() {
        let inbound = mock_inbound("hysteria2", "hy2_direct", 8443, json!({
            "network": "udp",
            "security": "tls",
            "tlsSettings": {
                "serverName": "timecard365.de"
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();
        let hy2_ob = outbounds.iter()
            .find(|o| o["type"] == "hysteria2")
            .expect("Hysteria2 outbound не найден");

        // insecure обязателен для Hysteria2 без валидного сертификата
        assert_eq!(hy2_ob["tls"]["insecure"], true, "Hysteria2 tls.insecure должен быть true");

        // alpn h3 обязателен для Hysteria2/QUIC
        let alpn = hy2_ob["tls"]["alpn"].as_array().expect("alpn должен быть массивом");
        assert!(
            alpn.iter().any(|a| a == "h3"),
            "Hysteria2 tls.alpn должен содержать 'h3'"
        );

        // password формируется как "tg_id:uuid_no_dashes"
        let password = hy2_ob["password"].as_str().unwrap_or("");
        assert!(!password.is_empty(), "Hysteria2 password пустой");
    }

    // ─── Тест 7: Формат тегов — slug·inbound_tag·d (не "01 - ...") ─────────

    #[test]
    fn outbound_tags_use_slug_dot_tag_dot_d_format_not_numbered() {
        let inbounds = vec![
            mock_inbound("vless", "reality_tcp_in", 443, json!({
                "network": "tcp", "security": "reality",
                "realitySettings": { "serverNames": ["t.de"], "publicKey": "k", "shortIds": ["s"] }
            })),
            mock_inbound("hysteria2", "hy2_in", 8443, json!({
                "network": "udp", "security": "tls",
                "tlsSettings": { "serverName": "t.de" }
            })),
        ];
        let node = mock_node("Germany 01", inbounds);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();

        // Ни один тег не должен начинаться с цифры и " - "
        for ob in outbounds {
            if let Some(tag) = ob["tag"].as_str() {
                assert!(
                    !tag.contains(" - "),
                    "Тег '{}' содержит ' - ' — это старый формат, должен быть slug·tag·d",
                    tag
                );
                let starts_with_numbered = tag.len() > 2
                    && tag.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);
                assert!(
                    !starts_with_numbered || tag.starts_with("auto") || tag.starts_with("proxy"),
                    "Тег '{}' начинается с цифры — подозрительный формат",
                    tag
                );
            }
        }

        // Теги прокси-аутбаундов должны быть в формате "{flag} {proto_label}"
        let proxy_tags: Vec<&str> = outbounds.iter()
            .filter(|o| matches!(o["type"].as_str(), Some("vless") | Some("hysteria2")))
            .filter_map(|o| o["tag"].as_str())
            .collect();

        assert!(!proxy_tags.is_empty(), "Нет proxy outbounds");
        // Tags should start with flag emoji and contain a proto label
        for tag in &proxy_tags {
            assert!(
                tag.starts_with("🇩🇪"),
                "Тег '{}' не начинается с флага 🇩🇪 — неверный формат direct outbound",
                tag
            );
        }
    }

    // ─── Тест 8: Пять типов inbounds — полный набор протоколов ───────────────

    #[test]
    fn all_five_inbound_types_produce_correct_outbounds() {
        let inbounds = vec![
            // 1. VLESS + TCP + Reality
            mock_inbound("vless", "reality_tcp", 443, json!({
                "network": "tcp", "security": "reality",
                "realitySettings": {
                    "fingerprint": "chrome",
                    "serverNames": ["timecard365.de"],
                    "publicKey": "pubkey",
                    "shortIds": ["sid"]
                }
            })),
            // 2. VLESS + HTTPUpgrade + TLS
            mock_inbound("vless", "httpupgrade_tls", 443, json!({
                "network": "httpupgrade", "security": "tls",
                "tlsSettings": { "serverName": "timecard365.de" },
                "httpupgradeSettings": { "path": "/upg", "host": "timecard365.de" }
            })),
            // 3. VLESS + WebSocket + TLS
            mock_inbound("vless", "ws_tls", 443, json!({
                "network": "ws", "security": "tls",
                "tlsSettings": { "serverName": "timecard365.de" },
                "wsSettings": { "path": "/ws" }
            })),
            // 4. VLESS + gRPC + TLS
            {
                let mut ib = mock_inbound("vless", "grpc_tls", 10400, json!({
                    "network": "grpc", "security": "tls",
                    "tlsSettings": { "serverName": "timecard365.de" },
                    "grpcSettings": { "serviceName": "grpc-service" }
                }));
                ib.id = 4;
                ib
            },
            // 5. Hysteria2
            {
                let mut ib = mock_inbound("hysteria2", "hy2_direct", 8443, json!({
                    "network": "udp", "security": "tls",
                    "tlsSettings": { "serverName": "timecard365.de" }
                }));
                ib.id = 5;
                ib
            },
        ];

        let node = mock_node("DE-01", inbounds);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();
        let proxy_obs: Vec<_> = outbounds.iter()
            .filter(|o| matches!(
                o["type"].as_str(),
                Some("vless") | Some("hysteria2") | Some("trojan") | Some("shadowsocks")
            ))
            .collect();

        // 5 inbounds → 5 direct outbounds (XHTTP пропускается, но XHTTP нет в нашем тесте)
        assert_eq!(
            proxy_obs.len(), 5,
            "Ожидается 5 proxy outbounds (по одному на каждый inbound), получено {}",
            proxy_obs.len()
        );

        // Проверяем каждый тип
        let vless_obs: Vec<_> = proxy_obs.iter().filter(|o| o["type"] == "vless").collect();
        let hy2_obs: Vec<_> = proxy_obs.iter().filter(|o| o["type"] == "hysteria2").collect();

        assert_eq!(vless_obs.len(), 4, "Ожидается 4 VLESS outbound");
        assert_eq!(hy2_obs.len(), 1, "Ожидается 1 Hysteria2 outbound");

        // Reality outbound должен иметь reality блок (tag: "🇩🇪 Stealth")
        let reality_ob = proxy_obs.iter()
            .find(|o| o["tag"].as_str() == Some("🇩🇪 Stealth"))
            .expect("Reality TCP outbound '🇩🇪 Stealth' не найден");
        assert!(reality_ob["tls"]["reality"]["enabled"] == true, "reality.enabled должен быть true");
        assert_eq!(reality_ob["flow"].as_str().unwrap_or(""), "xtls-rprx-vision",
            "Reality TCP должен иметь flow");

        // HTTPUpgrade должен иметь multiplex (tag: "🇩🇪 HTTP")
        let httpupgrade_ob = proxy_obs.iter()
            .find(|o| o["tag"].as_str() == Some("🇩🇪 HTTP"))
            .expect("HTTPUpgrade outbound '🇩🇪 HTTP' не найден");
        assert!(httpupgrade_ob["multiplex"].is_object(), "HTTPUpgrade должен иметь multiplex");

        // gRPC не должен иметь flow (tag: "🇩🇪 Stream")
        let grpc_ob = proxy_obs.iter()
            .find(|o| o["tag"].as_str() == Some("🇩🇪 Stream"))
            .expect("gRPC outbound '🇩🇪 Stream' не найден");
        let grpc_flow = grpc_ob["flow"].as_str().unwrap_or("");
        assert!(grpc_flow.is_empty(), "gRPC не должен иметь flow");
    }

    // ─── Тест 9: Proxy selector и urltest группы ─────────────────────────────

    #[test]
    fn config_has_proxy_selector_and_urltest_groups() {
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp", "security": "reality",
            "realitySettings": {
                "serverNames": ["t.de"], "publicKey": "k", "shortIds": ["s"]
            }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();

        // Должен быть selector "proxy" как главный outbound
        let proxy_selector = outbounds.iter()
            .find(|o| o["type"] == "selector" && o["tag"] == "proxy")
            .expect("Отсутствует selector outbound с тегом 'proxy'");

        // Список outbounds в selector должен начинаться с "auto-all"
        let selector_list = proxy_selector["outbounds"].as_array().unwrap();
        assert_eq!(selector_list[0], "auto-all", "Первый элемент selector должен быть 'auto-all'");

        // Должен быть urltest "auto-all"
        let has_auto_all = outbounds.iter()
            .any(|o| o["type"] == "urltest" && o["tag"] == "auto-all");
        assert!(has_auto_all, "Отсутствует urltest группа 'auto-all'");

        // Должны быть системные outbounds
        let has_direct = outbounds.iter().any(|o| o["type"] == "direct" && o["tag"] == "direct");
        let has_block = outbounds.iter().any(|o| o["type"] == "block" && o["tag"] == "block");
        let has_dns_out = outbounds.iter().any(|o| o["type"] == "dns" && o["tag"] == "dns-out");
        assert!(has_direct, "Отсутствует системный outbound 'direct'");
        assert!(has_block, "Отсутствует системный outbound 'block'");
        assert!(has_dns_out, "Отсутствует системный outbound 'dns-out'");
    }

    // ─── Тест 10: Ноды с is_relay=true пропускаются ──────────────────────────

    #[test]
    fn relay_only_nodes_are_skipped() {
        let inbound = mock_inbound("shadowsocks", "ss_relay", 10000, json!({}));
        let mut relay_node = mock_node("Relay-RU", vec![inbound]);
        relay_node.is_relay = true;

        // Если только relay нода — ошибка (нет proxy outbounds)
        let result = generate_singbox_config(&mock_sub(), &[relay_node], &mock_keys(), &[]);
        assert!(result.is_err(), "Конфиг только из relay-ноды должен возвращать ошибку");
    }

    // ─── Тест 11: XHTTP outbounds пропускаются (не поддерживается sing-box) ──

    #[test]
    fn xhttp_inbounds_are_skipped_for_singbox() {
        let xhttp_inbound = mock_inbound("vless", "xhttp_in", 443, json!({
            "network": "xhttp",
            "security": "tls",
            "tlsSettings": { "serverName": "t.de" }
        }));
        let reality_inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp",
            "security": "reality",
            "realitySettings": { "serverNames": ["t.de"], "publicKey": "k", "shortIds": ["s"] }
        }));

        let node = mock_node("DE-01", vec![xhttp_inbound, reality_inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();
        let proxy_obs: Vec<_> = outbounds.iter()
            .filter(|o| o["type"] == "vless")
            .collect();

        // Только 1 outbound (xhttp пропускается)
        assert_eq!(proxy_obs.len(), 1, "XHTTP inbound должен быть пропущен — ожидается 1 outbound");

        // Оставшийся должен быть Reality
        let remaining = proxy_obs[0];
        assert!(remaining["tls"]["reality"].is_object(),
            "Оставшийся outbound должен быть Reality");
    }

    // ─── Тест 12: DNS секция содержит нужные серверы ──────────────────────────

    #[test]
    fn dns_section_has_remote_local_and_block_servers() {
        let inbound = mock_inbound("vless", "reality_tcp", 443, json!({
            "network": "tcp", "security": "reality",
            "realitySettings": { "serverNames": ["t.de"], "publicKey": "k", "shortIds": ["s"] }
        }));
        let node = mock_node("DE-01", vec![inbound]);
        let config_str = generate_singbox_config(&mock_sub(), &[node], &mock_keys(), &[]).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();

        let dns_servers = config["dns"]["servers"].as_array().expect("dns.servers не массив");
        assert!(!dns_servers.is_empty(), "DNS серверы отсутствуют");

        let tags: Vec<&str> = dns_servers.iter()
            .filter_map(|s| s["tag"].as_str())
            .collect();
        assert!(tags.contains(&"remote"), "Отсутствует remote DNS сервер");
        assert!(tags.contains(&"local"), "Отсутствует local DNS сервер");
        assert!(tags.contains(&"local-plain"), "Отсутствует local-plain bootstrap DNS");
    }
}
