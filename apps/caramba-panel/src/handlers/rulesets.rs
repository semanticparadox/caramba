//! Публичная отдача rule-set списков для клиентских routing-пресетов.
//!
//! Клиенты (sing-box / mihomo) указывают на `{BASE}/rulesets/NAME` с
//! `behavior=domain|ipcidr, format=text` (один элемент на строку, без
//! бинарного .mrs-тулинга). Панель — это зеркало: отдаёт текстовый файл
//! за обычным HTTP, доступный даже там, где GitHub заблокирован.
//!
//! Имена rule-set'ов (`ru-blocked`, `ru-blocked-ip`, `ir-direct`, `by-blocked`,
//! `ads`) — стабильный контракт с пресетами Go-ядра
//! (`libs/caramba-core/routing/presets.go`, поле `Providers`). Переименование
//! ломает уже установленные клиенты, поэтому имена неизменны.
//!
//! # Имя обязано называть НАПРАВЛЕНИЕ, а не только страну
//!
//! Список `ir-direct` до этой правки назывался `ir-blocked` и подключался в
//! пресете `ir-smart` с действием PROXY. Апстрим у него ровно обратного
//! смысла — README bootmortis/iran-hosted-domains, раздел Categories:
//! «`other`: non `.ir` domains, use as `direct`», то есть иранские сервисы на
//! не-.ir доменах. 62 826 доменов иранских банков, госуслуг и торговли
//! уезжали в немецкий выход и там переставали работать: эти сервисы
//! отгорожены по гео на иранские IP.
//!
//! Поэтому у каждой записи реестра есть поле `intent`, и оно едет в
//! `/rulesets/status`: во время инцидента отвечающему видно, с каким
//! действием список задуман, не открывая исходники ядра. Соответствие
//! `intent` и действия в правилах пресета проверяет Go-тест
//! `TestRuleSetActionsMatchTheirDeclaredIntent`.
//!
//! Файлы лежат в каталоге RULESETS_DIR (по умолчанию ./rulesets) и
//! периодически обновляются `sync_rulesets` (на старте сервера и раз в 12 ч;
//! см. main.rs).
//!
//! # Почему апстримы — release-ассеты, а не файлы в ветке
//!
//! Прежние источники (`russia-blocked-domains/main/domains.lst`,
//! `russia-blocked-geoip/main/subnets.lst`,
//! `iran-hosted-domains/main/clash_rules/domains.txt`) отдают 404: проекты
//! перенесли данные из веток в GitHub Releases, а деревья репозиториев теперь
//! содержат только workflow и README. Ссылка вида
//! `/releases/latest/download/NAME` — это стабильный путь, который редиректит
//! на неизменяемый ассет последнего релиза: он не меняется под нами в середине
//! дня, как tip ветки, и при этом не требует ручного обновления тега.
//!
//! # Наблюдаемость
//!
//! Рядом с каждым списком лежит `NAME.meta.json` — результат последней попытки
//! синхронизации. Он переживает рестарт (in-memory состояние — нет) и отвечает
//! на вопрос «когда список обновлялся и сколько в нём записей».
//! `GET /rulesets/status` собирает эти файлы в один JSON.

use axum::{
    extract::Path,
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::collections::BTreeSet;
use std::net::IpAddr;

/// Один апстрим-источник списка.
struct RulesetSpec {
    name: &'static str,
    /// "domain" | "ipcidr" — определяет валидацию/нормализацию строк.
    behavior: &'static str,
    /// С каким действием список осмыслен: "proxy" | "direct" | "reject".
    ///
    /// Панель ничего по этому полю не решает — она зеркалит байты. Поле
    /// существует затем, чтобы НАЗНАЧЕНИЕ списка было записано там же, где
    /// его апстрим, и уезжало в `/rulesets/status`. Пара «апстрим + действие»
    /// — единственное, что отличает список иранских доменов от списка
    /// заблокированных в Иране, и именно её потеря сломала `ir-smart`.
    ///
    /// Зеркало этого поля в ядре — `routing.RuleSetIntent.Action`
    /// (`libs/caramba-core/routing/presets.go`, `RecommendedUpstreams`).
    intent: &'static str,
    upstreams: &'static [&'static str],
    /// Нижняя граница вменяемости: если после нормализации принято меньше
    /// строк, апстрим считается сломанным и файл НЕ заменяется.
    ///
    /// Это защита от самого коварного отказа — HTTP 200 с телом, которое не
    /// является списком правил (страница-заглушка, HTML-редирект, усечённый
    /// ответ CDN). Без порога такой ответ тихо затёр бы рабочий список почти
    /// пустым файлом, и маршрутизация у всех клиентов сломалась бы молча.
    /// Значения — примерно половина фактического объёма источника на момент
    /// подключения (см. таблицу в `sync_rulesets`).
    min_entries: usize,
}

/// Реестр зеркалируемых rule-set'ов. Имена стабильны и используются в
/// пресетах Go-ядра.
const RULESETS: &[RulesetSpec] = &[
    // РФ: заблокированные домены.
    //
    // Основной источник — geosite-срез `ru-blocked.txt` от runetfreedom
    // (~75 тыс. доменов, формат `domain:example.com`). Берём именно его, а не
    // `ru-blocked-all.txt` из того же релиза: полный срез весит 35 МБ, и
    // прокачивать его на мобильные клиенты каждые 12 часов бессмысленно.
    //
    // Второй источник — курируемый вручную список itdoginfo: он маленький, но
    // содержит то, что важно провести через VPN в первую очередь, и до сих пор
    // живёт в ветке (единственный из старого набора, который не переехал).
    //
    // Берётся ветка `Russia/inside` — «ресурсы, которые блокируются, в том
    // числе зарубежные ресурсы, которые сами блокируют российские подсети».
    // У того же проекта есть ЗЕРКАЛЬНАЯ ветка `Russia/outside` — российские
    // ресурсы, доступные только с российских подсетей; она означает ровно
    // обратное, и с действием PROXY сломала бы РФ так же, как перевёрнутый
    // иранский список сломал Иран. Путь `inside-raw.lst` менять нельзя.
    RulesetSpec {
        name: "ru-blocked",
        behavior: "domain",
        intent: "proxy",
        upstreams: &[
            "https://github.com/runetfreedom/russia-blocked-geosite/releases/latest/download/ru-blocked.txt",
            "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/Russia/inside-raw.lst",
        ],
        min_entries: 20_000,
    },
    // РФ: заблокированные IP/CIDR.
    //
    // runetfreedom/russia-blocked-geoip больше НЕ публикует текстовый срез:
    // в релизе только geoip.dat / MaxMind mmdb / srs, то есть бинарь, который
    // клиент с `format=text` прочитать не может. Поэтому источник другой —
    // 1andrevich/Re-filter-lists, ассет `ipsum.lst` (~27 тыс. CIDR, чистый
    // текст, релизы примерно раз в месяц).
    RulesetSpec {
        name: "ru-blocked-ip",
        behavior: "ipcidr",
        intent: "proxy",
        upstreams: &[
            "https://github.com/1andrevich/Re-filter-lists/releases/latest/download/ipsum.lst",
        ],
        min_entries: 5_000,
    },
    // Иран: домены ИРАНСКИХ сервисов, которые обязаны ходить НАПРЯМУЮ.
    //
    // Прежнее имя `ir-blocked` было ошибкой — причём комментарий здесь всё это
    // время описывал список верно («домены иранских сервисов»), а имя и
    // действие в пресете говорили обратное. Апстрим формулирует назначение сам
    // (README, раздел Categories): «`other`: non `.ir` domains, use as
    // `direct`» — это иранские банки, госуслуги и торговля, живущие на .com.
    // Они отгорожены по гео на иранские IP, поэтому через зарубежный выход они
    // не «медленные», а недоступные.
    //
    // Второй апстрим — Chocolate4U/Iran-clash-rules, `ir-lite.txt`: тот же
    // формат `+.domain`, то же назначение («Active non-ir TLD Iranian domains»),
    // 41 616 строк против 62 826 у bootmortis. Он здесь не ради объёма
    // (пересечение почти полное — объединение 62 828 против 62 826), а ради
    // двух вещей: во-первых, один живой источник переживает переезд другого и
    // порог вменяемости не срабатывает; во-вторых, ровно две строки, которых у
    // bootmortis нет структурно, — это оба иранских ccTLD целиком: `+.ir` и
    // `+.xn--mgba3a4f16a` (punycode для `.ایران`). В категории `other` их и не
    // может быть, она по определению «non .ir domains»; у bootmortis они
    // вынесены в отдельную категорию `tld-ir`, которую он в clash-формате не
    // публикует.
    //
    // Списка «заблокировано ВНУТРИ Ирана» здесь нет, потому что его негде
    // взять текстом: у bootmortis категория `proxy` едет только внутри
    // `iran.dat` / `iran-geosite.db` (бинарь v2ray/sing-box), а `format=text`
    // такое не читает; в каталоге Chocolate4U такой категории нет вовсе.
    // Поэтому проксирующая половина `ir-smart` осталась на тегах GEOSITE.
    RulesetSpec {
        name: "ir-direct",
        behavior: "domain",
        intent: "direct",
        upstreams: &[
            "https://github.com/bootmortis/iran-hosted-domains/releases/latest/download/clash_rules_other.txt",
            "https://github.com/Chocolate4U/Iran-clash-rules/releases/latest/download/ir-lite.txt",
        ],
        // Половина объединения (62 828 на момент подключения). Порог намеренно
        // выше вклада ЛЮБОГО одного источника по отдельности не поднимается:
        // 41 616 у Chocolate4U должны проходить в одиночку, иначе смерть
        // bootmortis отключала бы список целиком.
        min_entries: 30_000,
    },
    // Беларусь: отдельного массового апстрима нет — берём курируемый РФ-список
    // как базу (так было и раньше, менялся только путь к файлу). Направление у
    // него правильное (это `Russia/inside`, то есть заблокированное → PROXY),
    // но содержимое остаётся российским: белорусских блокировок в нём нет, и
    // `by-blocked` честнее считать «РФ-список, применённый к BY», а не
    // белорусским списком. Появится белорусский апстрим — добавлять сюда же.
    RulesetSpec {
        name: "by-blocked",
        behavior: "domain",
        intent: "proxy",
        upstreams: &[
            "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/Russia/inside-raw.lst",
        ],
        min_entries: 500,
    },
    // Реклама/трекеры: текстовый срез geosite-категории `category-ads-all`
    // (~149 тыс. доменов) — ровно тот набор, который mihomo матчит по
    // встроенной geosite-базе.
    //
    // Список ПОТРЕБЛЯЕТСЯ: `leadIn(true)` в
    // `libs/caramba-core/routing/presets.go` ставит на него правило
    // `RULE-SET,ads,REJECT`, и так работают пресеты `adblock` и `cn-smart`.
    // Встроенный тег `geosite("category-ads-all")` остался ПОДСТРАХОВКОЙ и
    // попадает в конфиг только тогда, когда зеркало недоступно
    // (`Rule.FallbackFor`) — например у клиента с импортированной подпиской,
    // у которого адреса панели нет вовсе.
    //
    // Практический вывод для дежурного, и он противоположен тому, что кажется:
    // подстраховка выбирается по наличию АДРЕСА зеркала, а не по тому, доехал ли
    // список. У клиента, подключённого к панели, адрес есть всегда, поэтому
    // встроенный тег подавлен — и если этот список протух, пропал или отдаётся
    // с ошибкой, блокировка рекламы у него просто ПЕРЕСТАЁТ работать, молча.
    // Откат на встроенную базу случается только там, где адреса панели нет
    // вовсе, то есть у импортированной подписки.
    //
    // Поэтому пустой или неотдающийся `ads` — это авария, а не деградация, и
    // видно её в `/rulesets/status`, а не по жалобам на вернувшуюся рекламу.
    RulesetSpec {
        name: "ads",
        behavior: "domain",
        intent: "reject",
        upstreams: &[
            "https://github.com/runetfreedom/russia-blocked-geosite/releases/latest/download/category-ads-all.txt",
        ],
        min_entries: 20_000,
    },
];

/// Зарезервированное имя для отчёта о состоянии зеркала.
///
/// Отдельного маршрута `/rulesets` завести нельзя, не трогая main.rs, поэтому
/// статус живёт под именем в том же `/rulesets/{name}`. Имя не пересекается с
/// реестром — проверяется тестом `status_name_is_not_a_ruleset`.
const STATUS_NAME: &str = "status";

/// Через сколько после последнего успеха список считается протухшим.
///
/// Синхронизация идёт раз в 12 часов, поэтому 26 часов — это два пропущенных
/// цикла с запасом на джиттер старта: одна неудачная попытка ещё не повод
/// кричать, две подряд — уже да.
const STALE_AFTER_SECS: i64 = 26 * 3600;

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

// ─── Отчёт о состоянии ─────────────────────────────────────────────────────

/// Итог обращения к одному апстриму.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct SourceReport {
    url: String,
    /// "ok" | "http-<код>" | "network-error" | "not-a-rule-list" | "read-error"
    outcome: String,
    accepted: usize,
    rejected: usize,
    /// Первые несколько непонятых строк — чтобы смена формата апстрима была
    /// видна в отчёте, а не только по упавшему счётчику.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rejected_samples: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Состояние одного rule-set'а после последней попытки синхронизации.
///
/// Пишется на диск рядом со списком (`NAME.meta.json`) при КАЖДОЙ попытке —
/// и удачной, и провальной. Поэтому «последний успех» и «последняя попытка» —
/// два разных поля: список может лежать свежий и рабочий, пока апстрим уже
/// вторые сутки отдаёт 404.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct RulesetMeta {
    name: String,
    behavior: String,
    /// Число строк в лежащем на диске файле (не в последней попытке).
    entries: usize,
    bytes: usize,
    /// RFC3339. None — файл не создавался ни разу.
    last_success_at: Option<String>,
    last_attempt_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    sources: Vec<SourceReport>,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn meta_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    dir.join(format!("{}.meta.json", name))
}

async fn load_meta(dir: &std::path::Path, name: &str) -> Option<RulesetMeta> {
    let raw = tokio::fs::read(meta_path(dir, name)).await.ok()?;
    serde_json::from_slice(&raw).ok()
}

async fn store_meta(dir: &std::path::Path, meta: &RulesetMeta) {
    let path = meta_path(dir, &meta.name);
    match serde_json::to_vec_pretty(meta) {
        Ok(body) => {
            if let Err(e) = tokio::fs::write(&path, &body).await {
                tracing::warn!(ruleset = meta.name, err = %e, "rulesets: не удалось записать meta");
            }
        }
        Err(e) => {
            tracing::warn!(ruleset = meta.name, err = %e, "rulesets: не удалось сериализовать meta")
        }
    }
}

/// Возраст последнего успеха в секундах.
fn age_secs(last_success_at: Option<&str>) -> Option<i64> {
    let ts = last_success_at?;
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    Some((chrono::Utc::now() - parsed.with_timezone(&chrono::Utc)).num_seconds())
}

// ─── HTTP ──────────────────────────────────────────────────────────────────

/// GET /rulesets/status — состояние зеркала в JSON.
///
/// Публичный, как и сами списки: он не раскрывает ничего, чего нет в
/// отдаваемых файлах, зато позволяет проверить зеркало одним curl'ом снаружи,
/// не заходя на хост за логами.
/// Каталог передаётся параметром, а не читается из окружения: так статус
/// проверяется тестом на временном каталоге, без правки глобального env.
async fn serve_status_in(dir: &std::path::Path) -> Response {
    let mut items = Vec::with_capacity(RULESETS.len());
    let mut healthy = 0usize;

    for spec in RULESETS {
        let meta = load_meta(dir, spec.name).await;
        let file_exists = tokio::fs::metadata(dir.join(spec.name)).await.is_ok();
        let age = meta
            .as_ref()
            .and_then(|m| age_secs(m.last_success_at.as_deref()));

        // Состояние честно различает «не знаю» и «знаю, что плохо»: список без
        // meta-файла — это не «ок» и не «сломан», это «никогда не проверялся».
        let state = match (&meta, file_exists) {
            (None, false) => "never-synced",
            (None, true) => "unknown-legacy-file",
            (Some(_), false) => "missing-file",
            (Some(_), true) => match age {
                Some(a) if a > STALE_AFTER_SECS => "stale",
                Some(_) => "ok",
                None => "missing-file",
            },
        };
        if state == "ok" {
            healthy += 1;
        }

        items.push(serde_json::json!({
            "name": spec.name,
            "behavior": spec.behavior,
            // Назначение списка — в отчёте, а не только в комментарии: дежурный
            // должен видеть «этот список уводит трафик НАПРЯМУЮ», не читая
            // пресеты ядра.
            "intent": spec.intent,
            "state": state,
            "file_present": file_exists,
            "entries": meta.as_ref().map(|m| m.entries),
            "bytes": meta.as_ref().map(|m| m.bytes),
            "last_success_at": meta.as_ref().and_then(|m| m.last_success_at.clone()),
            "last_attempt_at": meta.as_ref().map(|m| m.last_attempt_at.clone()),
            "age_seconds": age,
            "last_error": meta.as_ref().and_then(|m| m.last_error.clone()),
            "min_entries": spec.min_entries,
            "sources": meta.as_ref().map(|m| m.sources.clone()).unwrap_or_default(),
        }));
    }

    let body = serde_json::json!({
        "generated_at": now_rfc3339(),
        "dir": dir.to_string_lossy(),
        "stale_after_seconds": STALE_AFTER_SECS,
        "healthy": healthy,
        "total": RULESETS.len(),
        "rulesets": items,
    });

    // 503, когда здоров не весь набор: мониторингу достаточно кода ответа, а
    // тело всё равно объясняет, что именно сломалось.
    let code = if healthy == RULESETS.len() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let mut resp = (code, axum::Json(body)).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// GET /rulesets/{name} — отдаёт текстовый rule-provider списком.
///
/// Публичный (без auth): списки доменов не секретны и должны быть доступны
/// клиентам напрямую. Имя валидируется по белому списку, поэтому path traversal
/// невозможен.
pub async fn serve_ruleset(Path(name): Path<String>) -> Response {
    serve_named(&rulesets_dir(), &name).await
}

/// Тело хендлера с явным каталогом — чтобы тест не трогал переменные окружения.
async fn serve_named(dir: &std::path::Path, name: &str) -> Response {
    // Допускаем суффикс расширения (mihomo иногда добавляет). strip_suffix, а не
    // trim_end_matches: последний срезает суффикс повторно, и "a.txt.txt"
    // схлопывался бы в "a".
    let clean = name
        .strip_suffix(".txt")
        .or_else(|| name.strip_suffix(".list"))
        .unwrap_or(name);

    if clean == STATUS_NAME {
        return serve_status_in(dir).await;
    }

    let Some(spec) = spec_for(clean) else {
        return (StatusCode::NOT_FOUND, "Unknown rule-set").into_response();
    };

    let path = dir.join(clean);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let meta = load_meta(dir, spec.name).await;
            let mut resp = (StatusCode::OK, bytes).into_response();
            let h = resp.headers_mut();
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            );
            // 12 часов кэша — совпадает с Interval=43200 у RULE-PROVIDER'ов.
            h.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=43200"),
            );
            // Свежесть — в заголовках, чтобы `curl -I` отвечал на «когда
            // обновлялось и сколько записей» без разбора тела на мегабайты.
            if let Some(m) = meta {
                if let Ok(v) = HeaderValue::from_str(&m.entries.to_string()) {
                    h.insert(HeaderName::from_static("x-ruleset-entries"), v);
                }
                if let Some(ts) = m.last_success_at.as_deref()
                    && let Ok(v) = HeaderValue::from_str(ts)
                {
                    h.insert(HeaderName::from_static("x-ruleset-synced-at"), v);
                }
            }
            resp
        }
        Err(_) => {
            // «Неизвестно» должно объяснять причину, а не молчать: клиент
            // mihomo переживёт отсутствие провайдера, а человек с curl'ом
            // должен сразу видеть, ни разу не синхронизировались или сломались.
            let meta = load_meta(dir, spec.name).await;
            let reason = match &meta {
                None => "never synced (no sync has completed on this host yet)".to_string(),
                Some(m) => format!(
                    "last attempt {}, last success {}, error: {}",
                    m.last_attempt_at,
                    m.last_success_at.as_deref().unwrap_or("never"),
                    m.last_error.as_deref().unwrap_or("none recorded")
                ),
            };
            (
                StatusCode::NOT_FOUND,
                format!("Rule-set '{}' is not available: {}\n", spec.name, reason),
            )
                .into_response()
        }
    }
}

// ─── Нормализация строк ────────────────────────────────────────────────────

/// Что делать со строкой апстрима.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// Комментарий/пустая строка: это не ошибка, в счётчик отказов не идёт.
    Skip,
    Accept(String),
    /// Строку не удалось привести к формату mihomo. Причина — для отчёта.
    ///
    /// Отдаём отказ, а не строку «как есть»: mihomo молча игнорирует
    /// нераспознанное правило, и сломанный апстрим выглядел бы как рабочий.
    Reject(&'static str),
}

/// Отбрасывает комментарии и пустые строки, общее для обоих behavior'ов.
fn strip_comment(raw: &str) -> Option<&str> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') || line.starts_with("//") {
        return None;
    }
    Some(line)
}

/// Проверяет хост по меткам и возвращает канонический (нижний регистр) вид.
///
/// `explicit_suffix` — строка была ЯВНО помечена как суффикс (`.ua`, `+.ua`,
/// `domain:ua`). Только для таких допускается одна метка: голое `ua` без
/// пометки — почти наверняка мусор, а не правило на весь TLD.
fn validate_host(host: &str, explicit_suffix: bool) -> Result<String, &'static str> {
    if host.is_empty() {
        return Err("empty host");
    }
    if host.len() > 253 {
        return Err("host longer than 253 bytes");
    }
    let host = host.to_ascii_lowercase();

    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 && !explicit_suffix {
        return Err("single-label host without an explicit suffix marker");
    }
    for label in &labels {
        if label.is_empty() {
            return Err("empty label (leading, trailing or doubled dot)");
        }
        if label.len() > 63 {
            return Err("label longer than 63 bytes");
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("label starts or ends with '-'");
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            // Не-ASCII сюда же: IDN обязан приезжать в punycode (xn--…),
            // потому что mihomo сравнивает байты, а не unicode-формы.
            return Err("label has characters outside [a-z0-9-]");
        }
    }
    // "1.2.3.4" в списке доменов — это перепутанный источник, а не домен.
    if labels.iter().all(|l| l.bytes().all(|b| b.is_ascii_digit())) {
        return Err("looks like an IPv4 address, not a domain");
    }
    Ok(host)
}

/// Нормализует строку домена в text-формат domain-провайдера mihomo.
///
/// Понимает ровно те формы, которые реально отдают выбранные апстримы:
///   * `example.com`              — itdoginfo inside-raw.lst, iran domains.txt
///   * `.ua`                      — itdoginfo inside-raw.lst (правило на TLD)
///   * `+.example.com`            — iran clash_rules_*.txt
///   * `*.example.com`            — распространённый вариант той же семантики
///   * `domain:example.com`       — runetfreedom geosite-срезы
///   * `full:example.com`         — v2ray geosite, точное совпадение
///   * `DOMAIN-SUFFIX,example.com` / `DOMAIN,example.com` — clash-правила
///
/// Суффиксные формы дают `+.host` (домен и поддомены), точные — голый `host`.
/// Всё остальное отвергается с причиной.
fn normalize_domain(raw: &str) -> Line {
    let Some(line) = strip_comment(raw) else {
        return Line::Skip;
    };
    // Пробел внутри строки правила — признак чужого формата (hosts, dnsmasq,
    // nftset). Раньше здесь брался последний токен, и такая строка проходила
    // как домен; теперь это явный отказ с причиной.
    if line.contains(char::is_whitespace) {
        return Line::Reject("line contains whitespace; not a bare rule line");
    }

    // Формы, которые text-провайдер mihomo выразить не может. Их надо
    // отвергнуть ИМЕННО как неподдерживаемые, а не как мусор.
    for bad in ["regexp:", "keyword:", "DOMAIN-KEYWORD,", "DOMAIN-REGEX,"] {
        if line.starts_with(bad) {
            return Line::Reject("unsupported rule form (regexp/keyword)");
        }
    }

    // (остаток строки, суффиксная ли форма, явная ли пометка суффикса)
    let (host, as_suffix, explicit) = if let Some(r) = line.strip_prefix("domain:") {
        // v2ray `domain:` — совпадение по домену и поддоменам.
        (r, true, true)
    } else if let Some(r) = line.strip_prefix("full:") {
        (r, false, false)
    } else if let Some(r) = line.strip_prefix("DOMAIN-SUFFIX,") {
        (r, true, true)
    } else if let Some(r) = line.strip_prefix("DOMAIN,") {
        (r, false, false)
    } else if let Some(r) = line.strip_prefix("+.") {
        (r, true, true)
    } else if let Some(r) = line.strip_prefix("*.") {
        (r, true, true)
    } else if let Some(r) = line.strip_prefix('.') {
        (r, true, true)
    } else {
        // Голый домен: списки такого вида означают «домен и его поддомены».
        (line, true, false)
    };

    match validate_host(host, explicit) {
        Ok(h) if as_suffix => Line::Accept(format!("+.{}", h)),
        Ok(h) => Line::Accept(h),
        Err(reason) => Line::Reject(reason),
    }
}

/// Нормализует строку IP/CIDR в канонический `addr/len`.
///
/// Адрес разбирается настоящим парсером, а маска проверяется по границам
/// семейства. Прежняя проверка «есть точка или двоеточие» пропускала в файл
/// и `<!DOCTYPE html>`, и `1.2.3.4.5`, и `10.0.0.0/99`.
fn normalize_ipcidr(raw: &str) -> Line {
    let Some(line) = strip_comment(raw) else {
        return Line::Skip;
    };
    if line.contains(char::is_whitespace) {
        return Line::Reject("line contains whitespace; not a bare CIDR line");
    }

    let (addr_part, prefix_part) = match line.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (line, None),
    };

    let Ok(addr) = addr_part.parse::<IpAddr>() else {
        return Line::Reject("not a valid IP address");
    };
    let max = if addr.is_ipv4() { 32u8 } else { 128u8 };

    let prefix = match prefix_part {
        // Голый адрес — это /32 (или /128): mihomo ipcidr ждёт именно CIDR.
        None => max,
        Some(p) => match p.parse::<u8>() {
            Ok(v) if v <= max => v,
            Ok(_) => return Line::Reject("prefix length out of range for address family"),
            Err(_) => return Line::Reject("prefix length is not a number"),
        },
    };

    Line::Accept(format!("{}/{}", addr, prefix))
}

// ─── Синхронизация ─────────────────────────────────────────────────────────

/// Сколько непонятых строк сохранять в отчёте на каждый источник.
const REJECT_SAMPLES: usize = 5;

/// Скачивает апстрим-списки и пишет нормализованные текстовые файлы в RULESETS_DIR.
///
/// Вызывается на старте сервера, раз в 12 часов и как CLI-подкоманда
/// `sync-rulesets` (см. main.rs). Сбой одного источника не валит остальные.
/// Возвращает число rule-set'ов, файл которых был заменён в этом проходе.
///
/// Провал ГРОМКИЙ: каждый неудачный источник и каждый rule-set, оставшийся без
/// данных, пишутся на уровне ERROR, а состояние каждой попытки — успешной и
/// нет — сохраняется в `NAME.meta.json` и отдаётся из `/rulesets/status`.
/// Прошлая версия логировала это одним WARN и возвращала 0, из-за чего строка
/// `rulesets: sync complete synced=0` годами читалась как норма.
pub async fn sync_rulesets() -> anyhow::Result<usize> {
    let dir = rulesets_dir();
    tokio::fs::create_dir_all(&dir).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("caramba-panel/rulesets-sync")
        .build()?;

    let mut written = 0usize;
    let mut failed: Vec<&str> = Vec::new();

    for spec in RULESETS {
        let mut set: BTreeSet<String> = BTreeSet::new();
        let mut reports: Vec<SourceReport> = Vec::new();

        for url in spec.upstreams {
            reports.push(fetch_source(&client, spec, url, &mut set).await);
        }

        let previous = load_meta(&dir, spec.name).await;
        let attempt_at = now_rfc3339();

        // Порог вменяемости: пустой результат и «подозрительно мало» лечатся
        // одинаково — старый файл остаётся на месте, новый не пишется.
        if set.len() < spec.min_entries {
            let err = format!(
                "collected {} entries, below the sanity floor of {}; keeping the previous file",
                set.len(),
                spec.min_entries
            );
            tracing::error!(
                ruleset = spec.name,
                collected = set.len(),
                min_entries = spec.min_entries,
                sources = ?reports.iter().map(|r| (r.url.as_str(), r.outcome.as_str(), r.accepted)).collect::<Vec<_>>(),
                "rulesets: апстримы не дали пригодных данных, файл НЕ обновлён"
            );
            failed.push(spec.name);

            let file_len = tokio::fs::metadata(dir.join(spec.name))
                .await
                .map(|m| m.len() as usize)
                .unwrap_or(0);
            store_meta(
                &dir,
                &RulesetMeta {
                    name: spec.name.to_string(),
                    behavior: spec.behavior.to_string(),
                    // Числа берём из прошлого успеха: на диске лежит именно тот файл.
                    entries: previous.as_ref().map(|p| p.entries).unwrap_or(0),
                    bytes: file_len,
                    last_success_at: previous.as_ref().and_then(|p| p.last_success_at.clone()),
                    last_attempt_at: attempt_at,
                    last_error: Some(err),
                    sources: reports,
                },
            )
            .await;
            continue;
        }

        let body = set.iter().cloned().collect::<Vec<_>>().join("\n") + "\n";
        let entries = set.len();
        let bytes = body.len();

        // Атомарная замена: пишем во временный файл, затем rename.
        //
        // Ошибка записи НЕ прерывает проход по остальным спискам: диск может
        // кончиться ровно на самом большом файле (`ads` — 3 МБ), и терять из-за
        // этого ещё четыре зеркала незачем. Сбой уходит в meta и в общий ERROR.
        let final_path = dir.join(spec.name);
        let tmp_path = dir.join(format!("{}.tmp", spec.name));
        if let Err(e) = write_atomically(&tmp_path, &final_path, body.as_bytes()).await {
            tracing::error!(ruleset = spec.name, err = %e, "rulesets: запись файла не удалась");
            failed.push(spec.name);
            // Хвост от неудачной записи уберём, иначе он останется навсегда.
            tokio::fs::remove_file(&tmp_path).await.ok();
            store_meta(
                &dir,
                &RulesetMeta {
                    name: spec.name.to_string(),
                    behavior: spec.behavior.to_string(),
                    entries: previous.as_ref().map(|p| p.entries).unwrap_or(0),
                    bytes: 0,
                    last_success_at: previous.as_ref().and_then(|p| p.last_success_at.clone()),
                    last_attempt_at: attempt_at,
                    last_error: Some(format!("write failed: {}", e)),
                    sources: reports,
                },
            )
            .await;
            continue;
        }

        tracing::info!(
            ruleset = spec.name,
            entries,
            bytes,
            rejected = reports.iter().map(|r| r.rejected).sum::<usize>(),
            "rulesets: synced"
        );
        store_meta(
            &dir,
            &RulesetMeta {
                name: spec.name.to_string(),
                behavior: spec.behavior.to_string(),
                entries,
                bytes,
                last_success_at: Some(attempt_at.clone()),
                last_attempt_at: attempt_at,
                last_error: None,
                sources: reports,
            },
        )
        .await;
        written += 1;
    }

    if !failed.is_empty() {
        // Одна итоговая строка ERROR: её видно в journalctl без знания того,
        // как называется каждый отдельный список.
        tracing::error!(
            failed = ?failed,
            ok = written,
            total = RULESETS.len(),
            "rulesets: часть зеркал не обновилась; проверьте GET /rulesets/status"
        );
    }

    Ok(written)
}

/// Пишет файл через временный + rename, чтобы читатель никогда не увидел
/// список наполовину записанным.
async fn write_atomically(
    tmp: &std::path::Path,
    final_path: &std::path::Path,
    body: &[u8],
) -> std::io::Result<()> {
    tokio::fs::write(tmp, body).await?;
    tokio::fs::rename(tmp, final_path).await
}

/// Качает один источник и досыпает нормализованные строки в общий набор.
async fn fetch_source(
    client: &reqwest::Client,
    spec: &RulesetSpec,
    url: &str,
    set: &mut BTreeSet<String>,
) -> SourceReport {
    let mut report = SourceReport {
        url: url.to_string(),
        outcome: String::new(),
        accepted: 0,
        rejected: 0,
        rejected_samples: Vec::new(),
        error: None,
    };

    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(ruleset = spec.name, url, err = %e, "rulesets: скачивание не удалось");
            report.outcome = "network-error".into();
            report.error = Some(e.to_string());
            return report;
        }
    };

    if !resp.status().is_success() {
        // Именно этот случай и убил три прежних источника: они переехали, а
        // 404 оседал в WARN. Теперь это ERROR с полным URL.
        tracing::error!(
            ruleset = spec.name,
            url,
            status = %resp.status(),
            "rulesets: апстрим ответил не 200 — вероятно, список переехал"
        );
        report.outcome = format!("http-{}", resp.status().as_u16());
        return report;
    }

    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(ruleset = spec.name, url, err = %e, "rulesets: не удалось прочитать тело");
            report.outcome = "read-error".into();
            report.error = Some(e.to_string());
            return report;
        }
    };

    // 200 с HTML-телом — это страница-заглушка, а не список правил. Ловим до
    // разбора построчно, иначе отчёт распухнет тысячами бессмысленных отказов.
    let head = body.trim_start();
    if head.starts_with('<') {
        tracing::error!(
            ruleset = spec.name,
            url,
            "rulesets: апстрим вернул HTML вместо списка правил"
        );
        report.outcome = "not-a-rule-list".into();
        report.error = Some("response body starts with '<' (HTML, not a rule list)".into());
        return report;
    }

    for line in body.lines() {
        let outcome = match spec.behavior {
            "ipcidr" => normalize_ipcidr(line),
            _ => normalize_domain(line),
        };
        match outcome {
            Line::Skip => {}
            Line::Accept(v) => {
                report.accepted += 1;
                set.insert(v);
            }
            Line::Reject(reason) => {
                report.rejected += 1;
                if report.rejected_samples.len() < REJECT_SAMPLES {
                    // Строку обрезаем: в сломанном источнике она может быть
                    // одним мегабайтом минифицированного JSON.
                    let shown: String = line.chars().take(120).collect();
                    report
                        .rejected_samples
                        .push(format!("{} -- {}", shown, reason));
                }
            }
        }
    }

    report.outcome = "ok".into();
    if report.rejected > 0 {
        tracing::warn!(
            ruleset = spec.name,
            url,
            accepted = report.accepted,
            rejected = report.rejected,
            samples = ?report.rejected_samples,
            "rulesets: часть строк источника не распознана"
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(v: Line) -> String {
        match v {
            Line::Accept(s) => s,
            other => panic!("ожидался Accept, получено {:?}", other),
        }
    }

    fn is_reject(v: &Line) -> bool {
        matches!(v, Line::Reject(_))
    }

    #[test]
    fn status_name_is_not_a_ruleset() {
        // /rulesets/status перехватывается раньше поиска по реестру — имя не
        // должно однажды стать именем реального списка.
        assert!(spec_for(STATUS_NAME).is_none());
    }

    #[test]
    fn registry_names_are_path_safe() {
        // Имена уходят в имя файла и в URL, и то же ограничение проверяет
        // safeRuleSetPath в Go-ядре (libs/caramba-core/api/bootstrap.go).
        for s in RULESETS {
            assert!(!s.name.is_empty());
            assert!(!s.name.contains(".."));
            assert!(
                s.name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'),
                "имя {} содержит недопустимый символ",
                s.name
            );
            assert!(
                s.behavior == "domain" || s.behavior == "ipcidr",
                "неизвестный behavior у {}",
                s.name
            );
            assert!(
                matches!(s.intent, "proxy" | "direct" | "reject"),
                "неизвестное назначение {:?} у {}",
                s.intent,
                s.name
            );
            assert!(!s.upstreams.is_empty(), "у {} нет апстримов", s.name);
            for u in s.upstreams {
                assert!(u.starts_with("https://"), "{} не https", u);
            }
        }
    }

    /// Назначение каждого списка, выписанное отдельно от реестра.
    ///
    /// Это не тавтология: смысл списка задаёт README апстрима, а не эта
    /// таблица и не реестр. Продублировав его здесь, мы требуем, чтобы правка
    /// `intent` в реестре была ОСОЗНАННОЙ — тест придётся править отдельно, и
    /// на этом шаге есть шанс перечитать апстрим. Ровно этого шага не хватило,
    /// когда список иранских доменов подключили с действием PROXY.
    ///
    /// Второй экземпляр этой же таблицы — `RecommendedUpstreams` в
    /// `libs/caramba-core/routing/presets.go`; там она сверяется с реальными
    /// действиями правил в пресетах.
    #[test]
    fn every_ruleset_intent_is_pinned() {
        // (имя, назначение, чем это подтверждается у апстрима)
        let expected: &[(&str, &str, &str)] = &[
            (
                "ru-blocked",
                "proxy",
                "runetfreedom geosite:ru-blocked — заблокированные в России домены; itdoginfo Russia/inside — блокируемые ресурсы",
            ),
            (
                "ru-blocked-ip",
                "proxy",
                "Re-filter ipsum.lst — подсети заблокированного в РФ",
            ),
            (
                "ir-direct",
                "direct",
                "bootmortis README: `other`: non `.ir` domains, use as `direct`",
            ),
            (
                "by-blocked",
                "proxy",
                "тот же itdoginfo Russia/inside, применённый к BY",
            ),
            (
                "ads",
                "reject",
                "срез geosite-категории category-ads-all — реклама и трекеры",
            ),
        ];
        assert_eq!(
            expected.len(),
            RULESETS.len(),
            "новый список в реестре обязан приехать со своим назначением"
        );
        for (name, intent, why) in expected {
            let spec = spec_for(name).unwrap_or_else(|| panic!("нет списка {}", name));
            assert_eq!(
                spec.intent, *intent,
                "у {} назначение {:?}, ожидалось {:?} ({})",
                name, spec.intent, intent, why
            );
        }
    }

    // ── Реальные строки из выбранных источников ────────────────────────────

    #[test]
    fn runetfreedom_geosite_domain_prefix() {
        // github.com/runetfreedom/russia-blocked-geosite → ru-blocked.txt
        // (все 74 739 строк этого файла имеют префикс `domain:`; прежний
        //  нормализатор отбрасывал их целиком по проверке `contains(':')`,
        //  из-за чего ru-blocked собирался пустым)
        assert_eq!(
            accept(normalize_domain("domain:napensii.ua")),
            "+.napensii.ua"
        );
        assert_eq!(accept(normalize_domain("domain:4pda.to")), "+.4pda.to");
        assert_eq!(
            accept(normalize_domain("domain:zona.media")),
            "+.zona.media"
        );
        assert_eq!(
            accept(normalize_domain(
                "domain:api.app.prod.grazie.aws.intellij.net"
            )),
            "+.api.app.prod.grazie.aws.intellij.net"
        );
        // punycode (в файле 2396 таких меток) должен проходить как есть
        assert_eq!(
            accept(normalize_domain("domain:xn--80aswg.xn--p1ai")),
            "+.xn--80aswg.xn--p1ai"
        );
    }

    #[test]
    fn runetfreedom_ads_slice() {
        // category-ads-all.txt — тот же формат `domain:`
        assert_eq!(
            accept(normalize_domain("domain:jnarigjkijgoh.space")),
            "+.jnarigjkijgoh.space"
        );
    }

    #[test]
    fn itdoginfo_inside_raw() {
        // raw.githubusercontent.com/itdoginfo/allow-domains/main/Russia/inside-raw.lst
        assert_eq!(
            accept(normalize_domain("10minutemail.com")),
            "+.10minutemail.com"
        );
        assert_eq!(accept(normalize_domain("1337x.to")), "+.1337x.to");
        assert_eq!(accept(normalize_domain("24.kg")), "+.24.kg");
        // первая строка файла — правило на весь TLD
        assert_eq!(accept(normalize_domain(".ua")), "+.ua");
    }

    #[test]
    fn bootmortis_clash_rules() {
        // github.com/bootmortis/iran-hosted-domains → clash_rules_other.txt
        // (список `ir-direct`: иранские сервисы на не-.ir доменах, DIRECT)
        assert_eq!(accept(normalize_domain("+.0-4-4.com")), "+.0-4-4.com");
        assert_eq!(
            accept(normalize_domain("+.0098charge.com")),
            "+.0098charge.com"
        );
        // в файле встречается смешанный регистр — нормализуем к нижнему,
        // иначе mihomo (сравнение по байтам) не сматчит хост
        assert_eq!(
            accept(normalize_domain("+.ContentVM.com")),
            "+.contentvm.com"
        );
        // две первые строки файла — комментарии
        assert_eq!(normalize_domain("# Clash"), Line::Skip);
        assert_eq!(
            normalize_domain(
                "# Wiki: https://dreamacro.github.io/clash/premium/rule-providers.html#rule-providers"
            ),
            Line::Skip
        );
    }

    #[test]
    fn chocolate4u_ir_lite() {
        // github.com/Chocolate4U/Iran-clash-rules → ir-lite.txt, второй апстрим
        // того же `ir-direct`: тот же формат `+.domain`, 41 616 строк.
        assert_eq!(accept(normalize_domain("+.0-4-4.com")), "+.0-4-4.com");
        assert_eq!(
            accept(normalize_domain("+.1000bazaar.com")),
            "+.1000bazaar.com"
        );
        // Ради этих двух строк источник и добавлен: оба иранских ccTLD
        // целиком, которых в категории `other` у bootmortis нет структурно.
        // Одна метка проходит только благодаря явной пометке суффикса `+.` —
        // без неё validate_host отвергает такую строку как мусор.
        assert_eq!(accept(normalize_domain("+.ir")), "+.ir");
        // punycode для `.ایران` — второй иранский ccTLD.
        assert_eq!(
            accept(normalize_domain("+.xn--mgba3a4f16a")),
            "+.xn--mgba3a4f16a"
        );
        assert_eq!(
            normalize_domain(
                "# clash rules in text format, require clash permium 1.15.0+ or clash-meta 1.14.4+"
            ),
            Line::Skip
        );
    }

    #[test]
    fn refilter_ipsum_cidr() {
        // github.com/1andrevich/Re-filter-lists → ipsum.lst (27 135 строк CIDR)
        assert_eq!(
            accept(normalize_ipcidr("198.23.57.168/32")),
            "198.23.57.168/32"
        );
        assert_eq!(
            accept(normalize_ipcidr("153.92.126.213/32")),
            "153.92.126.213/32"
        );
        assert_eq!(
            accept(normalize_ipcidr("66.22.202.11/32")),
            "66.22.202.11/32"
        );
    }

    // ── Отказы: то, что раньше молча утекало в mihomo ──────────────────────

    #[test]
    fn rejects_unsupported_geosite_forms() {
        // text-провайдер mihomo не умеет ни regexp, ни keyword: такие строки
        // должны считаться отказами, а не тихо пропадать
        assert!(is_reject(&normalize_domain("regexp:.*\\.example\\.com")));
        assert!(is_reject(&normalize_domain("keyword:doubleclick")));
        assert!(is_reject(&normalize_domain("DOMAIN-KEYWORD,ads")));
    }

    #[test]
    fn rejects_foreign_line_formats() {
        // dnsmasq/nftset-строка старого источника itdoginfo: раньше брался
        // последний токен, и в файл уезжал мусор
        assert!(is_reject(&normalize_domain(
            "nftset=/10minutemail.com/4#inet#fw4#vpn_domains"
        )));
        // hosts-формат
        assert!(is_reject(&normalize_domain("0.0.0.0 ads.example.com")));
        // URL вместо домена
        assert!(is_reject(&normalize_domain("https://example.com/path")));
    }

    #[test]
    fn rejects_html_and_garbage_domains() {
        assert!(is_reject(&normalize_domain("<!DOCTYPE html>")));
        assert!(is_reject(&normalize_domain("<html>")));
        assert!(is_reject(&normalize_domain("localhost")));
        assert!(is_reject(&normalize_domain("-bad.example.com")));
        assert!(is_reject(&normalize_domain("bad-.example.com")));
        assert!(is_reject(&normalize_domain("double..dot.com")));
        assert!(is_reject(&normalize_domain("пример.рф")));
        // IP в списке доменов — перепутанный источник
        assert!(is_reject(&normalize_domain("192.168.0.1")));
    }

    #[test]
    fn rejects_bad_cidr() {
        assert!(is_reject(&normalize_ipcidr("<!DOCTYPE html>")));
        assert!(is_reject(&normalize_ipcidr("1.2.3.4.5")));
        assert!(is_reject(&normalize_ipcidr("10.0.0.0/99")));
        assert!(is_reject(&normalize_ipcidr("10.0.0.0/abc")));
        assert!(is_reject(&normalize_ipcidr("example.com")));
        assert!(is_reject(&normalize_ipcidr("0.0.0.0 8.8.8.8")));
    }

    #[test]
    fn ipcidr_fills_in_the_host_prefix() {
        assert_eq!(accept(normalize_ipcidr("8.8.8.8")), "8.8.8.8/32");
        assert_eq!(accept(normalize_ipcidr("2001:db8::1")), "2001:db8::1/128");
        assert_eq!(accept(normalize_ipcidr("2001:db8::/32")), "2001:db8::/32");
    }

    #[test]
    fn comments_are_skipped_not_rejected() {
        // Skip и Reject — разные вещи: комментарии не должны раздувать
        // счётчик отказов, иначе порог «слишком много мусора» врёт.
        for l in ["", "   ", "# comment", "! adblock header", "// note"] {
            assert_eq!(normalize_domain(l), Line::Skip, "строка {:?}", l);
            assert_eq!(normalize_ipcidr(l), Line::Skip, "строка {:?}", l);
        }
    }

    #[test]
    fn exact_match_forms_do_not_get_the_suffix_marker() {
        assert_eq!(accept(normalize_domain("full:example.com")), "example.com");
        assert_eq!(
            accept(normalize_domain("DOMAIN,example.com")),
            "example.com"
        );
        assert_eq!(
            accept(normalize_domain("DOMAIN-SUFFIX,example.com")),
            "+.example.com"
        );
        // одна метка допустима только с явной пометкой суффикса
        assert!(is_reject(&normalize_domain("ua")));
        assert_eq!(accept(normalize_domain("domain:ua")), "+.ua");
    }

    // ── Хендлер ────────────────────────────────────────────────────────────

    /// Уникальный временный каталог: тесты в одном бинарнике идут параллельно,
    /// общий каталог они бы затирали друг у друга.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "caramba-rulesets-test-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn body_string(resp: Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn status_reports_never_synced_before_the_first_run() {
        let dir = scratch_dir("status-empty");
        let resp = serve_named(&dir, STATUS_NAME).await;
        // Пустое зеркало — это не «ок»: статус обязан отдавать не-200.
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = body_string(resp).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["healthy"], 0);
        assert_eq!(v["total"], RULESETS.len());
        for item in v["rulesets"].as_array().unwrap() {
            assert_eq!(item["state"], "never-synced", "{}", item["name"]);
            assert_eq!(item["file_present"], false);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn status_answers_when_and_how_many() {
        let dir = scratch_dir("status-full");
        // Кладём файл и meta для каждого списка — как после удачной синхронизации.
        for spec in RULESETS {
            std::fs::write(dir.join(spec.name), "+.example.com\n").unwrap();
            let meta = RulesetMeta {
                name: spec.name.to_string(),
                behavior: spec.behavior.to_string(),
                entries: 12_345,
                bytes: 14,
                last_success_at: Some(now_rfc3339()),
                last_attempt_at: now_rfc3339(),
                last_error: None,
                sources: Vec::new(),
            };
            std::fs::write(
                meta_path(&dir, spec.name),
                serde_json::to_vec(&meta).unwrap(),
            )
            .unwrap();
        }

        let resp = serve_named(&dir, STATUS_NAME).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v["healthy"], RULESETS.len());
        let first = &v["rulesets"][0];
        assert_eq!(first["state"], "ok");
        assert_eq!(first["entries"], 12_345);
        assert!(first["last_success_at"].is_string());
        assert!(first["age_seconds"].as_i64().unwrap() >= 0);

        // Тот же каталог отдаёт и сам список — со свежестью в заголовках.
        let resp = serve_named(&dir, "ru-blocked").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["x-ruleset-entries"], "12345");
        assert!(resp.headers().contains_key("x-ruleset-synced-at"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn status_marks_a_long_dead_mirror_stale() {
        let dir = scratch_dir("status-stale");
        std::fs::write(dir.join("ru-blocked"), "+.example.com\n").unwrap();
        let meta = RulesetMeta {
            name: "ru-blocked".into(),
            behavior: "domain".into(),
            entries: 7,
            bytes: 14,
            // Успех был давно, попытки идут и падают — ровно тот случай, из-за
            // которого зеркало и умерло незаметно.
            last_success_at: Some(
                (chrono::Utc::now() - chrono::Duration::days(9))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
            last_attempt_at: now_rfc3339(),
            last_error: Some("http-404".into()),
            sources: Vec::new(),
        };
        std::fs::write(
            meta_path(&dir, "ru-blocked"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();

        let resp = serve_named(&dir, STATUS_NAME).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        let ru = v["rulesets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "ru-blocked")
            .unwrap();
        assert_eq!(ru["state"], "stale");
        assert_eq!(ru["last_error"], "http-404");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn missing_list_explains_itself_instead_of_a_bare_404() {
        let dir = scratch_dir("missing");
        let resp = serve_named(&dir, "ru-blocked").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_string(resp).await;
        assert!(body.contains("never synced"), "тело 404: {}", body);

        // А неизвестное имя остаётся коротким 404 — это защита от traversal,
        // и подробности здесь рассказывать нечему и незачем.
        let resp = serve_named(&dir, "../../etc/passwd").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_string(resp).await, "Unknown rule-set");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn extension_suffix_is_stripped_once() {
        let dir = scratch_dir("suffix");
        std::fs::write(dir.join("ir-direct"), "+.example.com\n").unwrap();
        assert_eq!(
            serve_named(&dir, "ir-direct.txt").await.status(),
            StatusCode::OK
        );
        assert_eq!(
            serve_named(&dir, "ir-direct.list").await.status(),
            StatusCode::OK
        );
        // Повторный суффикс НЕ схлопывается до валидного имени.
        assert_eq!(
            serve_named(&dir, "ir-direct.txt.txt").await.status(),
            StatusCode::NOT_FOUND
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meta_roundtrip_and_age() {
        let m = RulesetMeta {
            name: "ru-blocked".into(),
            behavior: "domain".into(),
            entries: 75_000,
            bytes: 1_900_000,
            last_success_at: Some("2026-09-03T10:00:00Z".into()),
            last_attempt_at: "2026-09-03T22:00:00Z".into(),
            last_error: None,
            sources: vec![SourceReport {
                url: "https://example.invalid/list".into(),
                outcome: "ok".into(),
                accepted: 75_000,
                rejected: 2,
                rejected_samples: vec!["junk -- empty label".into()],
                error: None,
            }],
        };
        let raw = serde_json::to_vec(&m).expect("meta сериализуется");
        let back: RulesetMeta = serde_json::from_slice(&raw).expect("meta читается обратно");
        assert_eq!(back.entries, 75_000);
        assert_eq!(back.sources[0].rejected, 2);

        assert!(age_secs(Some("2026-09-03T10:00:00Z")).is_some());
        assert!(age_secs(None).is_none());
        assert!(age_secs(Some("не дата")).is_none());
    }
}
