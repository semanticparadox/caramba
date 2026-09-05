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
                            return build_cached_response(ct_val, body.to_vec());
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

    // Собираем content-type и остальные заголовки ДО потребления тела ответа.
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let panel_headers = resp.headers().clone();

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

    build_client_response(
        status.as_u16(),
        &panel_headers,
        &content_type,
        body_bytes.to_vec(),
    )
}

/// Заголовки, которые зеркало собирает САМО и потому не переписывает у панели.
///
/// Это закрытый список транспортных забот — кадрирование тела, кеширование,
/// подпись самого соединения. Он не растёт, когда панель заводит очередной
/// смысловой заголовок, и в этом весь смысл: список ниже описывает работу
/// зеркала, а не словарь панели.
///
/// Обратное правило (белый список из трёх имён) как раз и породило баг, ради
/// которого написан этот код: панель начала объяснять отказ заголовками
/// `x-caramba-*`, а до клиентов через RU-зеркало не доехало ни одного, потому
/// что никто не вспомнил про третий файл в другом сервисе. Зеркало — труба, а
/// не редактор: всё, что панель сказала клиенту, доезжает до клиента.
const MIRROR_OWNED_HEADERS: &[&str] = &[
    // Кадрирование: тело здесь пересобирается, длина и кодировка исходного
    // ответа к новому телу уже не относятся. Пропустить их — отдать клиенту
    // ответ, который он не сможет прочитать.
    "content-length",
    "content-encoding",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
    "proxy-authenticate",
    "proxy-authorization",
    // Ставится явно ниже — одним значением и на ответе панели, и на отдаче из
    // кеша, чтобы клиент видел один и тот же content-type на обоих путях.
    "content-type",
    // Кеширование — политика зеркала (no-store). Два разных значения в одном
    // ответе клиент прочитал бы как повезёт.
    "cache-control",
    "pragma",
    "expires",
    // Проставляются нижним слоем (hyper) и краевым прокси; копия из ответа
    // панели дала бы дубль с чужим временем.
    "date",
    "server",
];

fn mirror_owns_header(name: &str) -> bool {
    MIRROR_OWNED_HEADERS
        .iter()
        .any(|owned| owned.eq_ignore_ascii_case(name))
}

/// Переносит заголовки ответа панели в ответ клиенту.
///
/// Значения копируются байтами, а не через `to_str()`: имя плана в
/// `profile-title` задаёт человек в админке, и один кириллический символ не
/// должен тихо выбрасывать весь заголовок. Повторяющиеся имена сохраняются
/// всеми значениями — `header()` добавляет, а не замещает.
fn forward_panel_headers(
    mut builder: axum::http::response::Builder,
    panel_headers: &reqwest::header::HeaderMap,
) -> axum::http::response::Builder {
    for (name, value) in panel_headers.iter() {
        if mirror_owns_header(name.as_str()) {
            continue;
        }
        let (Ok(n), Ok(v)) = (
            axum::http::HeaderName::from_bytes(name.as_str().as_bytes()),
            axum::http::HeaderValue::from_bytes(value.as_bytes()),
        ) else {
            continue;
        };
        builder = builder.header(n, v);
    }
    builder
}

/// Ответ клиенту на основе ответа панели: код и тело — панели побайтово,
/// заголовки — панели плюс то, чем распоряжается зеркало.
fn build_client_response(
    status: u16,
    panel_headers: &reqwest::header::HeaderMap,
    content_type: &str,
    body: Vec<u8>,
) -> Response {
    let builder = forward_panel_headers(Response::builder().status(status), panel_headers);
    builder
        .header(header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
        .header(header::PRAGMA, "no-cache")
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .unwrap()
        .into_response()
}

/// Имя, которым зеркало помечает конфиг, отданный из кеша при недоступной
/// панели. Единственный заголовок `x-caramba-*`, который печатает зеркало, а не
/// панель, — держать его здесь константой значит держать имя занятым.
pub const HDR_MIRROR_CACHE: &str = "x-caramba-mirror-cache";

/// Ответ из кеша: конфиг живой, но цифры расхода и причина отказа устарели или
/// отсутствуют вовсе.
///
/// Заголовки панели сюда НЕ восстанавливаются намеренно. Отдать сохранённый
/// пять минут назад `subscription-userinfo` значит уверенно сказать «трафик
/// есть» тому, у кого он мог кончиться минуту назад, — ровно та ложь, от
/// которой этот круг правок и затевался. Клиент вместо этого получает признак
/// «ответ из кеша» и волен показать человеку, что данные не свежие.
fn build_cached_response(content_type: &str, body: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
        .header(header::PRAGMA, "no-cache")
        .header(HDR_MIRROR_CACHE, "1")
        .body(axum::body::Body::from(body))
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
mod panel_header_tests {
    use super::*;
    use reqwest::header::{HeaderMap as PanelHeaders, HeaderName as PanelName, HeaderValue};

    /// Ответ панели с отказом по квоте: те заголовки, которые панель печатает
    /// в `access::refusal_headers` плюс `subscription-userinfo`.
    fn panel_refusal() -> PanelHeaders {
        let mut h = PanelHeaders::new();
        for (k, v) in [
            ("content-type", "text/plain; charset=utf-8"),
            ("content-length", "45"),
            (
                "subscription-userinfo",
                "upload=0; download=275775488; total=209715200; expire=253402300799",
            ),
            ("x-caramba-state", "quota_exceeded"),
            ("x-caramba-st", "7"),
            ("x-caramba-reason", "3003"),
            ("x-caramba-reason-name", "daily_allowance_exhausted"),
            ("x-caramba-used", "275775488"),
            ("x-caramba-limit", "209715200"),
            ("x-caramba-period", "day"),
            ("x-caramba-resets-at", "1788652800"),
            ("x-caramba-reset-lag", "1800"),
            ("x-caramba-bytes-after-reset", "143654912"),
        ] {
            h.insert(PanelName::from_static(k), HeaderValue::from_static(v));
        }
        h
    }

    fn header_values(r: &Response, name: &str) -> Vec<String> {
        r.headers()
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect()
    }

    /// Правило пересылки, ради которого всё это написано: причина отказа,
    /// собранная панелью, доезжает до клиента через зеркало ЦЕЛИКОМ. Раньше
    /// здесь стоял белый список из трёх имён, и все десять `x-caramba-*`
    /// пропадали молча.
    #[test]
    fn every_refusal_header_the_panel_printed_reaches_the_client() {
        let panel = panel_refusal();
        let r = build_client_response(
            403,
            &panel,
            "text/plain; charset=utf-8",
            b"Traffic limit reached. Subscription is expired.".to_vec(),
        );

        assert_eq!(r.status().as_u16(), 403);
        for (name, value) in panel.iter() {
            if mirror_owns_header(name.as_str()) {
                continue;
            }
            assert_eq!(
                header_values(&r, name.as_str()),
                vec![value.to_str().unwrap().to_string()],
                "заголовок {} потерялся по дороге через зеркало",
                name
            );
        }
    }

    /// Главная гарантия против повторения бага: заголовок, которого сегодня в
    /// коде зеркала нет ни одной строкой, всё равно доезжает. Правило описывает
    /// то, чем распоряжается зеркало, а не то, что умеет говорить панель.
    #[test]
    fn a_header_this_file_has_never_heard_of_is_forwarded_too() {
        let mut panel = PanelHeaders::new();
        panel.insert(
            PanelName::from_static("x-caramba-invented-after-this-test-was-written"),
            HeaderValue::from_static("42"),
        );
        // Панель уже печатает его на успешном конфиге, а зеркало роняло: ядро
        // берёт отсюда страну пользователя и без неё уходит в чужой пресет.
        panel.insert(
            PanelName::from_static("x-client-country"),
            HeaderValue::from_static("RU"),
        );
        let r = build_client_response(200, &panel, "application/json", b"{}".to_vec());

        assert_eq!(
            header_values(&r, "x-caramba-invented-after-this-test-was-written"),
            vec!["42".to_string()]
        );
        assert_eq!(
            header_values(&r, "x-client-country"),
            vec!["RU".to_string()]
        );
    }

    /// Транспортные заголовки панели не переезжают: тело здесь пересобрано, и
    /// чужая длина или кодировка сделали бы ответ нечитаемым. Каждый из тех,
    /// которыми зеркало распоряжается само, стоит в ответе ровно один раз.
    #[test]
    fn the_framing_headers_belong_to_the_mirror_and_appear_once() {
        let mut panel = panel_refusal();
        panel.insert(
            PanelName::from_static("cache-control"),
            HeaderValue::from_static("public, max-age=600"),
        );
        panel.insert(
            PanelName::from_static("date"),
            HeaderValue::from_static("Thu, 01 Jan 1970 00:00:00 GMT"),
        );
        let r = build_client_response(403, &panel, "text/plain; charset=utf-8", b"x".to_vec());

        assert_eq!(
            header_values(&r, "content-type"),
            vec!["text/plain; charset=utf-8".to_string()]
        );
        assert_eq!(
            header_values(&r, "cache-control"),
            vec!["no-store, no-cache, must-revalidate".to_string()]
        );
        assert!(header_values(&r, "content-length").is_empty());
        assert!(header_values(&r, "date").is_empty());
    }

    /// Имя плана задаёт человек в админке. Кириллица в `profile-title` не
    /// повод выбросить заголовок целиком: копируем байты, а не строку.
    #[test]
    fn a_non_ascii_profile_title_survives_instead_of_vanishing() {
        let mut panel = PanelHeaders::new();
        let title = "Тариф Free";
        panel.insert(
            PanelName::from_static("profile-title"),
            HeaderValue::from_bytes(title.as_bytes()).unwrap(),
        );
        let r = build_client_response(200, &panel, "application/json", b"{}".to_vec());

        assert_eq!(
            r.headers().get("profile-title").map(|v| v.as_bytes()),
            Some(title.as_bytes())
        );
    }

    /// Отдача из кеша помечена, и цифры расхода из неё НЕ воскрешаются:
    /// сохранённое пять минут назад «трафик есть» — это та самая уверенная
    /// ложь, ради которой круг правок и начался.
    #[test]
    fn a_cached_config_is_marked_and_carries_no_stale_numbers() {
        let r = build_cached_response("application/json", b"{}".to_vec());

        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(header_values(&r, HDR_MIRROR_CACHE), vec!["1".to_string()]);
        assert!(header_values(&r, "subscription-userinfo").is_empty());
        assert!(header_values(&r, "x-caramba-state").is_empty());
    }
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
