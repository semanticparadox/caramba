use crate::AppState;
use axum::{
    body::{to_bytes, Body},
    extract::{Path, Request, State},
    http::{HeaderName, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::Client;
use std::collections::HashSet;
use std::time::Duration;

const MAX_PROXY_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_PROXY_HOPS: u32 = 3;
const HOP_HEADER: &str = "x-caramba-proxy-hop";

/// Таймауты исходящего запроса к панели.
///
/// Разные для API и для раздачи файлов намеренно. У API ответ маленький и
/// быстрый, поэтому общий потолок полезен: он не даёт зависшему апстриму
/// держать соединение. У сборок клиента (APK/EXE/DMG — десятки мегабайт)
/// общего потолка быть не может: `timeout` в reqwest считает ВЕСЬ цикл, включая
/// чтение тела, и на мобильном канале он оборвал бы файл на середине — человек
/// получил бы битую загрузку вместо ошибки. Вместо него сторожим паузу между
/// кусками тела (`read_timeout`): реально мёртвый апстрим так всё равно
/// отваливается, а медленный клиент — нет.
#[derive(Clone, Copy)]
struct UpstreamTimeouts {
    connect: Duration,
    total: Option<Duration>,
    read: Option<Duration>,
}

const API_TIMEOUTS: UpstreamTimeouts = UpstreamTimeouts {
    connect: Duration::from_secs(3),
    total: Some(Duration::from_secs(10)),
    read: None,
};

const DOWNLOAD_TIMEOUTS: UpstreamTimeouts = UpstreamTimeouts {
    connect: Duration::from_secs(5),
    total: None,
    read: Some(Duration::from_secs(60)),
};

fn should_skip_request_header(name: &HeaderName) -> bool {
    let key = name.as_str();
    key.eq_ignore_ascii_case("host")
        || key.eq_ignore_ascii_case("connection")
        || key.eq_ignore_ascii_case("transfer-encoding")
        || key.eq_ignore_ascii_case("content-length")
}

/// Заголовки ответа, которые нельзя переносить как есть: они описывают
/// конкретное соединение с панелью, а не наш ответ клиенту. Тело мы
/// пересобираем сами (`Body::from_stream`), поэтому чужой `transfer-encoding`
/// здесь означал бы «закодировано дважды». Остальное (в том числе `cache-*`,
/// `content-type`, `content-length`, `etag`) прокидываем без изменений — иначе
/// докачка и кеш браузера у файлов сломались бы.
fn should_skip_response_header(name: &HeaderName) -> bool {
    let key = name.as_str();
    key.eq_ignore_ascii_case("connection") || key.eq_ignore_ascii_case("transfer-encoding")
}

/// Адреса панели, по которым пробуем один и тот же путь.
///
/// `upstream_path` — путь запроса БЕЗ ведущего слэша ровно в том виде, в каком
/// его должна увидеть панель (`api/v2/...`, `downloads/Caramba-...apk`).
/// Раньше здесь был зашит префикс `api/`, и любой другой маршрут зеркала
/// пришлось бы проксировать отдельной копией этой функции.
fn candidate_targets(panel_url: &str, frontend_domain: &str, upstream_path: &str) -> Vec<String> {
    let normalized = panel_url.trim_end_matches('/');
    let path = upstream_path.trim_start_matches('/');
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    let push_target = |targets: &mut Vec<String>, seen: &mut HashSet<String>, url: String| {
        if seen.insert(url.clone()) {
            targets.push(url);
        }
    };

    if let Ok(parsed) = reqwest::Url::parse(normalized) {
        let host = parsed.host_str().unwrap_or_default();
        let is_same_host = host.eq_ignore_ascii_case(frontend_domain)
            || host.eq_ignore_ascii_case("localhost")
            || host == "127.0.0.1";

        // In same-host deployments we must avoid forwarding back to domain URL,
        // otherwise /api may loop sub -> caddy -> sub and end in 502.
        if is_same_host {
            push_target(
                &mut targets,
                &mut seen,
                format!("http://127.0.0.1:3000/{}", path),
            );
            push_target(
                &mut targets,
                &mut seen,
                format!("http://localhost:3000/{}", path),
            );
        } else {
            push_target(&mut targets, &mut seen, format!("{}/{}", normalized, path));
        }

        // Fallback to plain HTTP on same host when HTTPS between local services is broken.
        if parsed.scheme().eq_ignore_ascii_case("https") {
            let mut http_url = parsed.clone();
            let _ = http_url.set_scheme("http");
            if !is_same_host {
                push_target(
                    &mut targets,
                    &mut seen,
                    format!("{}/{}", http_url.as_str().trim_end_matches('/'), path),
                );
            }
        }
    } else {
        // If URL parsing failed, still try raw target first.
        push_target(&mut targets, &mut seen, format!("{}/{}", normalized, path));
    }

    // Universal fallback for single-host deployments.
    push_target(
        &mut targets,
        &mut seen,
        format!("http://127.0.0.1:3000/{}", path),
    );
    push_target(
        &mut targets,
        &mut seen,
        format!("http://localhost:3000/{}", path),
    );

    targets
}

/// Прокси API панели: `/api/*` зеркала уходит на `<panel>/api/*`.
pub async fn proxy_handler(
    Path(path): Path<String>,
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    let upstream = with_query(format!("api/{path}"), req.uri().query());
    forward(state, req, upstream, API_TIMEOUTS).await
}

/// Дописывает строку запроса к пути апстрима.
///
/// Зачем: axum-экстрактор `Path` отдаёт только путь, и прокси молча терял
/// `?platform=android` у `/api/v2/app/version`, `?relay_country=` и любые
/// другие параметры — через зеркало приложение получало «unknown platform»,
/// хотя напрямую к панели тот же запрос работал. Пустая строка запроса
/// («/x?») тоже не дописывается: апстриму она не нужна.
fn with_query(path: String, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{path}?{q}"),
        _ => path,
    }
}

/// Раздача сборок клиента через зеркало: `/downloads/*` уходит на панель как
/// есть (`<panel>/downloads/*`).
///
/// Зачем зеркало: ссылка на файл попадает людям в бота и в мини-апп, и пока в
/// ней стоял адрес панели, он уезжал каждому, кто просто скачивал клиент.
/// Файлы по-прежнему лежат только на панели (их кладёт туда инсталлятор), мы
/// лишь стримим их наружу. 404 панели остаётся 404 — подменять его нельзя:
/// оператор должен видеть, что файла для платформы нет.
pub async fn downloads_handler(
    Path(path): Path<String>,
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response {
    let upstream = with_query(format!("downloads/{path}"), req.uri().query());
    forward(state, req, upstream, DOWNLOAD_TIMEOUTS).await
}

async fn forward(
    state: AppState,
    req: Request<Body>,
    upstream_path: String,
    timeouts: UpstreamTimeouts,
) -> Response {
    let (parts, body) = req.into_parts();
    let hop_count = parts
        .headers
        .get(HOP_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    if hop_count >= MAX_PROXY_HOPS {
        tracing::error!(
            "Proxy loop detected for /{} (hop_count={})",
            upstream_path,
            hop_count
        );
        return (
            StatusCode::BAD_GATEWAY,
            "Proxy loop detected while routing request",
        )
            .into_response();
    }

    let mut builder = Client::builder().connect_timeout(timeouts.connect);
    if let Some(total) = timeouts.total {
        builder = builder.timeout(total);
    }
    if let Some(read) = timeouts.read {
        builder = builder.read_timeout(read);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(err) => {
            tracing::error!("Failed to initialize proxy client: {}", err);
            return (
                StatusCode::BAD_GATEWAY,
                "Proxy upstream client initialization failed",
            )
                .into_response();
        }
    };

    let body_bytes = match to_bytes(body, MAX_PROXY_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!("Proxy request body read failed: {}", err);
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body is too large for proxy",
            )
                .into_response();
        }
    };

    let targets = candidate_targets(
        &state.config.panel_url,
        &state.config.domain,
        &upstream_path,
    );
    let mut errors: Vec<String> = Vec::new();

    for target_url in targets {
        tracing::debug!("Proxying request to: {}", target_url);

        let mut proxy_req = client
            .request(parts.method.clone(), &target_url)
            .body(body_bytes.clone());

        for (key, value) in &parts.headers {
            if should_skip_request_header(key) {
                continue;
            }
            proxy_req = proxy_req.header(key, value);
        }
        proxy_req = proxy_req.header(HOP_HEADER, (hop_count + 1).to_string());

        match proxy_req.send().await {
            Ok(res) => {
                let status = res.status();
                let mut response = Response::builder().status(status);

                if let Some(headers) = response.headers_mut() {
                    for (key, value) in res.headers() {
                        if should_skip_response_header(key) {
                            continue;
                        }
                        // `append`, а не `insert`: заголовок может повторяться
                        // (например `set-cookie`), и вставка оставила бы от
                        // ответа панели только последний.
                        headers.append(key, value.clone());
                    }
                }

                let body = Body::from_stream(res.bytes_stream());
                return response.body(body).unwrap_or_else(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to build response",
                    )
                        .into_response()
                });
            }
            Err(err) => {
                let msg = format!("{} => {}", target_url, err);
                tracing::warn!("Proxy target failed: {}", msg);
                errors.push(msg);
            }
        }
    }

    tracing::error!(
        "All proxy targets failed for /{}: {:?}",
        upstream_path,
        errors
    );
    (
        StatusCode::BAD_GATEWAY,
        format!(
            "Proxy error: all upstream targets failed. {}",
            errors.join(" | ")
        ),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{candidate_targets, with_query};

    /// Путь уходит на панель как есть: ни `api/`, ни чего-либо ещё функция
    /// больше не дописывает — иначе файл клиента искали бы по `/api/downloads`.
    #[test]
    fn the_query_string_survives_the_proxy() {
        assert_eq!(
            with_query("api/v2/app/version".into(), Some("platform=android")),
            "api/v2/app/version?platform=android"
        );
        assert_eq!(
            with_query("api/v2/app/relays".into(), None),
            "api/v2/app/relays"
        );
        assert_eq!(with_query("api/x".into(), Some("")), "api/x");
    }

    #[test]
    fn downloads_path_goes_to_the_panel_unchanged() {
        let targets = candidate_targets(
            "https://panel.exarobot.top",
            "app.exarobot.top",
            "downloads/Caramba-Connect-Android-arm64.apk",
        );
        assert_eq!(
            targets.first().map(String::as_str),
            Some("https://panel.exarobot.top/downloads/Caramba-Connect-Android-arm64.apk")
        );
        assert!(
            targets.iter().all(|t| !t.contains("/api/")
                && t.ends_with("/downloads/Caramba-Connect-Android-arm64.apk")),
            "неожиданные цели: {targets:?}"
        );
    }

    /// Прежнее поведение API-прокси: префикс теперь приносит вызывающий.
    #[test]
    fn api_path_keeps_the_previous_targets() {
        let targets = candidate_targets(
            "https://panel.exarobot.top/",
            "app.exarobot.top",
            "api/v2/app/me",
        );
        assert_eq!(
            targets.first().map(String::as_str),
            Some("https://panel.exarobot.top/api/v2/app/me")
        );
        assert!(targets.contains(&"http://127.0.0.1:3000/api/v2/app/me".to_string()));
    }

    /// Панель и зеркало на одном хосте: запрос обязан идти на локальный порт
    /// панели, иначе загрузка файла закольцуется sub -> caddy -> sub.
    #[test]
    fn same_host_downloads_go_to_the_local_panel_port() {
        let targets = candidate_targets(
            "https://app.exarobot.top",
            "app.exarobot.top",
            "downloads/Caramba-Connect-macOS-arm64.dmg",
        );
        assert_eq!(
            targets.first().map(String::as_str),
            Some("http://127.0.0.1:3000/downloads/Caramba-Connect-macOS-arm64.dmg")
        );
        assert!(
            targets.iter().all(|t| !t.contains("app.exarobot.top")),
            "нельзя ходить обратно через публичный домен: {targets:?}"
        );
    }
}
