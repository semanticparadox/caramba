use crate::AppState;
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use tracing::{error, info, warn};

#[derive(Deserialize)]
pub struct SubParams {
    pub client: Option<String>,
    pub relay_country: Option<String>,
    /// Закрепление конкретного выходного узла. Панель (`/sub/{uuid}`) принимает
    /// `node_id` как i64; здесь храним как строку и пробрасываем без разбора,
    /// чтобы не терять выбор сервера (например, `caramba up <server>` из CLI).
    pub node_id: Option<String>,
}

/// Detect client type from User-Agent header when ?client= is not specified.
fn detect_client_from_ua(headers: &HeaderMap) -> &'static str {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Check sing-box clients FIRST — Hiddify UA contains "ClashMeta" and "v2ray"
    // e.g. "HiddifyNext/4.0.0 (ios) like ClashMeta v2ray sing-box"
    if ua.contains("hiddify") || ua.contains("sing-box") || ua.contains("sfi") || ua.contains("sfa")
    {
        "singbox"
    } else if ua.contains("clash") || ua.contains("stash") || ua.contains("mihomo") {
        "clash"
    } else if ua.contains("shadowrocket")
        || ua.contains("v2rayn")
        || ua.contains("v2rayng")
        || ua.contains("streisand")
        || ua.contains("fair")
        || ua.contains("nekoray")
        || ua.contains("happ")
    {
        "v2ray"
    } else {
        "singbox"
    }
}

pub async fn subscription_handler(
    Path(uuid): Path<String>,
    Query(params): Query<SubParams>,
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let client_type = match params.client.as_deref() {
        Some(c) => match c.to_ascii_lowercase().as_str() {
            "hiddify" | "singbox" | "sing-box" | "sfi" | "sfa" | "nekobox" => "singbox",
            "clash" | "clashmeta" | "mihomo" | "stash" => "clash",
            "v2ray" | "v2rayn" | "v2rayng" | "xray" | "shadowrocket" | "streisand" => "v2ray",
            _ => detect_client_from_ua(&headers),
        },
        None => detect_client_from_ua(&headers),
    };

    // Заголовков может не быть, если sub стоит на краю сам, без обратного прокси.
    // Тогда единственный достоверный источник — адрес самого соединения. Раньше сюда
    // подставлялся 0.0.0.0, и панель молча выбрасывала такой запрос из учёта.
    let client_ip = get_client_ip(&headers).unwrap_or_else(|| canonical_ip(peer.ip()).to_string());
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    info!(
        "Subscription request: UUID={}, client={}, ip={}, ua={:?}",
        uuid, client_type, client_ip, user_agent
    );

    // All formats proxied to panel — panel has the authoritative generators
    // for sing-box (geo-based auto-relay, proper naming), v2ray, and clash.
    // Forward client IP and User-Agent so panel can do device tracking and geo filtering.
    proxy_to_panel(
        &state,
        &uuid,
        client_type,
        &client_ip,
        user_agent.as_deref(),
        params.relay_country.as_deref(),
        params.node_id.as_deref(),
    )
    .await
}

/// Proxy subscription requests to the panel, which has the authoritative
/// generators for all formats (sing-box, v2ray, clash).
/// При успешном ответе панели — кешируем тело в Redis на 5 минут.
/// При недоступности панели — отдаём закешированный конфиг вместо 502.
async fn proxy_to_panel(
    state: &AppState,
    uuid: &str,
    client_type: &str,
    client_ip: &str,
    user_agent: Option<&str>,
    relay_country: Option<&str>,
    node_id: Option<&str>,
) -> Response {
    let mut panel_sub_url = format!(
        "{}/sub/{}?client={}",
        state.config.panel_url, uuid, client_type
    );
    // URL-кодируем значения: node_id пробрасывается «как есть» строкой, поэтому
    // его (как и relay_country) нельзя интерполировать в query сырым.
    if let Some(rc) = relay_country {
        panel_sub_url.push_str(&format!("&relay_country={}", urlencoding::encode(rc)));
    }
    // Пробрасываем node_id, иначе выбор сервера (`caramba up <server>`) теряется
    // при прохождении через sub-сервис.
    if let Some(nid) = node_id {
        panel_sub_url.push_str(&format!("&node_id={}", urlencoding::encode(nid)));
    }

    // Ключ кеша включает relay_country и node_id — разные узлы/страны дают разные конфиги.
    let cache_key = format!(
        "sub:config:{}:{}:{}:{}",
        uuid,
        client_type,
        relay_country.unwrap_or(""),
        node_id.unwrap_or("")
    );

    let resp = match state
        .panel_client
        .proxy_subscription(&panel_sub_url, &state.config.domain, client_ip, user_agent)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to proxy subscription to panel: {}", e);

            // Панель недоступна — пробуем отдать закешированный конфиг.
            // Кеш хранит тело и content-type вместе (разделитель \x00).
            // Без правильного content-type клиенты (sing-box, clash) не распознают формат.
            if let Some(ref redis_client) = state.redis_client {
                if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
                    if let Ok(cached_raw) = redis::cmd("GET")
                        .arg(&cache_key)
                        .query_async::<Vec<u8>>(&mut conn)
                        .await
                    {
                        if !cached_raw.is_empty() {
                            warn!("Serving cached config for {} (panel down)", uuid);
                            // Извлекаем content-type и тело из закешированного blob'а
                            let (ct, body) = split_cached_entry(&cached_raw);
                            let ct_val = ct.unwrap_or("application/json");
                            return Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, ct_val)
                                .header(
                                    header::CACHE_CONTROL,
                                    "no-store, no-cache, must-revalidate",
                                )
                                .header(header::PRAGMA, "no-cache")
                                .body(axum::body::Body::from(body.to_vec()))
                                .unwrap()
                                .into_response();
                        }
                    }
                }
            }

            return (
                StatusCode::BAD_GATEWAY,
                "Panel subscription endpoint unavailable",
            )
                .into_response();
        }
    };

    let status = resp.status();

    if status.is_redirection() {
        let location = resp.headers().get("location").and_then(|v| v.to_str().ok());
        error!(
            "Panel returned redirect {} -> {:?} (Host header may not match subscription_domain)",
            status, location
        );
        return (StatusCode::BAD_GATEWAY, "Panel subscription redirect loop").into_response();
    }

    let mut builder = Response::builder().status(status.as_u16());

    // Forward relevant headers from panel response.
    // content-type не включён здесь — он будет установлен явно ниже
    // из захваченного значения для согласованности с Redis fallback.
    for key in &[
        "profile-title",
        "profile-update-interval",
        "subscription-userinfo",
    ] {
        if let Some(val) = resp.headers().get(*key) {
            if let Ok(v) = val.to_str() {
                builder = builder.header(*key, v);
            }
        }
    }

    builder = builder
        .header(header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
        .header(header::PRAGMA, "no-cache");

    // Собираем content-type ДО потребления тела ответа
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let body_bytes = resp.bytes().await.unwrap_or_default();

    // Кешируем успешный ответ панели на 5 минут (только для 2xx).
    // Формат blob: "<content-type>\x00<body>" — позволяет восстановить заголовок при fallback.
    if status.is_success() {
        if let Some(ref redis_client) = state.redis_client {
            if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
                let cached_entry = build_cached_entry(&content_type, &body_bytes);
                let _: Result<(), _> = redis::cmd("SET")
                    .arg(&cache_key)
                    .arg(cached_entry.as_slice())
                    .arg("EX")
                    .arg(300u64) // TTL 5 минут
                    .query_async(&mut conn)
                    .await;
            }
        }
    }

    builder
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body_bytes))
        .unwrap()
        .into_response()
}

/// Собирает blob для хранения в Redis: "<content-type>\x00<body>".
fn build_cached_entry(content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut entry = content_type.as_bytes().to_vec();
    entry.push(0x00); // разделитель
    entry.extend_from_slice(body);
    entry
}

/// Разбирает blob из Redis на (content-type, body).
/// Если разделитель не найден — весь blob считается телом (обратная совместимость).
fn split_cached_entry(raw: &[u8]) -> (Option<&str>, &[u8]) {
    if let Some(pos) = raw.iter().position(|&b| b == 0x00) {
        let ct = std::str::from_utf8(&raw[..pos]).ok();
        let body = &raw[pos + 1..];
        (ct, body)
    } else {
        // Старый формат кеша без content-type заголовка
        (None, raw)
    }
}

/// IPv4-mapped IPv6 (`::ffff:1.2.3.4`) приводим к обычному IPv4, иначе один и тот же
/// клиент попадает в учёт панели под двумя разными адресами.
fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

/// Адрес пригоден для передачи панели, только если он разбирается и не является
/// заглушкой. `0.0.0.0` от кривого прокси не должен побеждать реальный адрес
/// соединения — иначе учёт устройств слепнет и никак об этом не сообщает.
fn usable_ip(raw: &str) -> Option<String> {
    let ip: IpAddr = raw.trim().parse().ok()?;
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return None;
    }
    Some(canonical_ip(ip).to_string())
}

fn get_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(ip) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(usable_ip)
    {
        return Some(ip);
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').find_map(usable_ip))
}

#[cfg(test)]
mod client_ip_tests {
    use super::*;

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn placeholder_address_is_rejected_so_the_peer_address_can_win() {
        assert_eq!(
            get_client_ip(&headers_with("x-forwarded-for", "0.0.0.0")),
            None
        );
        assert_eq!(get_client_ip(&HeaderMap::new()), None);
    }

    #[test]
    fn first_usable_hop_of_the_chain_is_taken() {
        assert_eq!(
            get_client_ip(&headers_with(
                "x-forwarded-for",
                "0.0.0.0, 203.0.113.10, 198.51.100.7"
            )),
            Some("203.0.113.10".to_string())
        );
    }

    #[test]
    fn cloudflare_header_wins_over_the_forwarded_chain() {
        let mut headers = headers_with("x-forwarded-for", "203.0.113.10");
        headers.insert("cf-connecting-ip", "198.51.100.7".parse().unwrap());
        assert_eq!(get_client_ip(&headers), Some("198.51.100.7".to_string()));
    }

    #[test]
    fn mapped_ipv6_collapses_to_ipv4() {
        assert_eq!(
            get_client_ip(&headers_with("x-forwarded-for", "::ffff:203.0.113.10")),
            Some("203.0.113.10".to_string())
        );
    }
}
