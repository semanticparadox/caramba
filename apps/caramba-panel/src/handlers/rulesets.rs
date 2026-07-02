//! Публичная отдача rule-set списков для routing-пресетов Go-ядра.
//!
//! Go-ядро (libs/caramba-core/routing) генерирует mihomo RULE-PROVIDER'ы,
//! указывающие на `{BASE}/rulesets/NAME` с `behavior=domain|ipcidr, format=text`
//! (один элемент на строку, без бинарного .mrs-тулинга). Панель — это зеркало:
//! отдаёт текстовый файл за обычным HTTP, доступный даже там, где GitHub
//! заблокирован.
//!
//! Имена rule-set'ов и их апстримы — контракт с Go-стороной
//! (routing.RecommendedUpstreams): ru-blocked, ru-blocked-ip, ir-blocked,
//! by-blocked. Файлы лежат в каталоге RULESETS_DIR (по умолчанию ./rulesets) и
//! периодически обновляются `sync_rulesets` (вызывается на старте сервера и по
//! таймеру; см. main.rs).

use axum::{
    extract::Path,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

/// Известные rule-set'ы и их апстрим-источники.
///
/// Кортеж: (имя/путь, behavior, &[upstream raw-URL списков]).
/// Несколько URL — объединяются и дедуплицируются при синхронизации.
/// Источники намеренно raw-текстовые (по одному домену/CIDR на строку),
/// чтобы не тащить geo-dat/.mrs-конвертацию.
struct RulesetSpec {
    name: &'static str,
    /// "domain" | "ipcidr" — определяет валидацию/нормализацию строк.
    behavior: &'static str,
    upstreams: &'static [&'static str],
}

/// Реестр зеркалируемых rule-set'ов. Должен совпадать по именам с
/// routing.RecommendedUpstreams в Go-ядре.
const RULESETS: &[RulesetSpec] = &[
    // РФ: заблокированные/внутри-РФ домены. itdoginfo/allow-domains — текстовые
    // списки доменов; russia-v2ray-rules-dat отдаёт geosite-производные .lst.
    RulesetSpec {
        name: "ru-blocked",
        behavior: "domain",
        upstreams: &[
            "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/Russia/inside-dnsmasq-nfset.lst",
            "https://raw.githubusercontent.com/runetfreedom/russia-blocked-domains/main/domains.lst",
        ],
    },
    // РФ: заблокированные IP/CIDR.
    RulesetSpec {
        name: "ru-blocked-ip",
        behavior: "ipcidr",
        upstreams: &[
            "https://raw.githubusercontent.com/runetfreedom/russia-blocked-geoip/main/subnets.lst",
        ],
    },
    // Иран: домены иранских сервисов / заблокированные.
    RulesetSpec {
        name: "ir-blocked",
        behavior: "domain",
        upstreams: &[
            "https://raw.githubusercontent.com/bootmortis/iran-hosted-domains/main/clash_rules/domains.txt",
        ],
    },
    // Беларусь: наследует РФ-список + локальные дополнения. Отдельного
    // массового апстрима нет — берём РФ-домены как базу.
    RulesetSpec {
        name: "by-blocked",
        behavior: "domain",
        upstreams: &[
            "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/Russia/inside-dnsmasq-nfset.lst",
        ],
    },
];

/// Каталог с файлами rule-set'ов. RULESETS_DIR или ./rulesets по умолчанию.
fn rulesets_dir() -> std::path::PathBuf {
    std::env::var("RULESETS_DIR")
        .unwrap_or_else(|_| "./rulesets".to_string())
        .into()
}

/// Проверяет, что имя — из реестра (а не произвольный путь). Защита от traversal.
fn spec_for(name: &str) -> Option<&'static RulesetSpec> {
    RULESETS.iter().find(|s| s.name == name)
}

/// GET /rulesets/{name} — отдаёт текстовый rule-provider списком.
///
/// Публичный (без auth): списки доменов не секретны и должны быть доступны
/// клиентам напрямую. Имя валидируется по белому списку, поэтому path traversal
/// невозможен. Если файл ещё не синхронизирован, отдаём 404 — клиент mihomo
/// переживает временную недоступность провайдера (использует кэш/fallback).
pub async fn serve_ruleset(Path(name): Path<String>) -> Response {
    // Допускаем суффикс расширения (mihomo иногда добавляет), нормализуем имя.
    let clean = name.trim_end_matches(".txt").trim_end_matches(".list");

    if spec_for(clean).is_none() {
        return (StatusCode::NOT_FOUND, "Unknown rule-set").into_response();
    }

    let path = rulesets_dir().join(clean);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mut resp = (StatusCode::OK, bytes).into_response();
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            );
            // 12 часов кэша — совпадает с Interval=43200 у RULE-PROVIDER'ов.
            resp.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=43200"),
            );
            resp
        }
        Err(_) => (StatusCode::NOT_FOUND, "Rule-set not yet synced; try later").into_response(),
    }
}

/// Нормализует строку домена в clash domain-формат (`+.example.com`).
///
/// Принимает варианты из апстримов: `example.com`, `.example.com`,
/// `0.0.0.0 example.com` (hosts), `||example.com^` (adblock), `+.example.com`.
/// Возвращает None для комментариев/пустых/мусорных строк.
fn normalize_domain(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    // hosts-формат: "0.0.0.0 domain" / "127.0.0.1 domain"
    let token = line.split_whitespace().last().unwrap_or(line);
    // adblock-формат: ||domain^
    let token = token
        .trim_start_matches("||")
        .trim_end_matches('^')
        .trim_start_matches("*.")
        .trim_start_matches("+.")
        .trim_start_matches('.');
    // Отсекаем явный мусор: пробелы, схемы, пути.
    if token.is_empty()
        || token.contains('/')
        || token.contains(':')
        || token.contains(' ')
        || !token.contains('.')
    {
        return None;
    }
    // clash domain behavior: ведущий "+." матчит и поддомены.
    Some(format!("+.{}", token))
}

/// Нормализует строку IP/CIDR. Возвращает как есть, если похоже на адрес/CIDR.
fn normalize_ipcidr(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let token = line.split_whitespace().next().unwrap_or(line);
    // Грубая проверка: содержит точку (IPv4) или двоеточие (IPv6).
    if token.contains('.') || token.contains(':') {
        // Добавляем /32 для голого IPv4 без маски — mihomo ipcidr ждёт CIDR.
        if token.contains('.') && !token.contains('/') && !token.contains(':') {
            return Some(format!("{}/32", token));
        }
        return Some(token.to_string());
    }
    None
}

/// Скачивает апстрим-списки и пишет нормализованные текстовые файлы в RULESETS_DIR.
///
/// Лёгкий механизм синхронизации (reqwest). Вызывается на старте сервера и по
/// таймеру (см. main.rs), а также доступен как CLI-подкоманда `sync-rulesets`.
/// Сбой одного источника не валит остальные — пишем что смогли, логируем ошибки.
/// Возвращает число успешно записанных rule-set'ов.
pub async fn sync_rulesets() -> anyhow::Result<usize> {
    let dir = rulesets_dir();
    tokio::fs::create_dir_all(&dir).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("caramba-panel/rulesets-sync")
        .build()?;

    let mut written = 0usize;
    for spec in RULESETS {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        for url in spec.upstreams {
            match client.get(*url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.text().await {
                    Ok(body) => {
                        for line in body.lines() {
                            let norm = match spec.behavior {
                                "ipcidr" => normalize_ipcidr(line),
                                _ => normalize_domain(line),
                            };
                            if let Some(v) = norm {
                                set.insert(v);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(ruleset = spec.name, url = url, err = %e, "rulesets: read body failed");
                    }
                },
                Ok(resp) => {
                    tracing::warn!(ruleset = spec.name, url = url, status = %resp.status(), "rulesets: upstream non-200");
                }
                Err(e) => {
                    tracing::warn!(ruleset = spec.name, url = url, err = %e, "rulesets: download failed");
                }
            }
        }

        if set.is_empty() {
            tracing::warn!(
                ruleset = spec.name,
                "rulesets: no entries fetched, keeping existing file"
            );
            continue;
        }

        // Атомарная замена: пишем во временный файл, затем rename.
        let body = set.into_iter().collect::<Vec<_>>().join("\n") + "\n";
        let final_path = dir.join(spec.name);
        let tmp_path = dir.join(format!("{}.tmp", spec.name));
        tokio::fs::write(&tmp_path, body.as_bytes()).await?;
        tokio::fs::rename(&tmp_path, &final_path).await?;
        tracing::info!(ruleset = spec.name, "rulesets: synced");
        written += 1;
    }

    Ok(written)
}
