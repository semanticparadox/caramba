//! Релизы клиента Caramba Connect: манифесты, «последняя версия» и разовая
//! рассылка «вышла новая версия».
//!
//! # Откуда панель знает версию клиента
//!
//! CI (`client-android.yml`, `client-desktop.yml`) кладёт рядом с каждым
//! бинарником манифест `Caramba-Connect-<platform>.json` (формат — в
//! `apps/caramba-client/scripts/ci-manifest.sh`), а инсталлятор панели
//! складывает его в `apps/caramba-panel/downloads/` вместе с самим файлом.
//! Этот модуль читает манифесты с диска (с кэшем на [`CACHE_TTL`]: файлы
//! меняются раз в релиз, а `/api/v2/app/version` дёргает каждое приложение
//! при старте) и, если манифеста нет, падает на ручные настройки
//! `client_latest_version` / `client_latest_build` из админки.
//!
//! # Как не разослать дважды
//!
//! Таблица `client_release_notices(platform, build)` — журнал. Решение
//! принимает чистая функция [`decide`]: сборка новее всего, что в журнале, —
//! рассылка; таблица пуста — первый увиденный манифест записывается молча
//! (свежая установка не должна начинать с массовой рассылки; на проде то же
//! страхует бэкфилл в миграции `20260911190000_client_releases.sql`).
//! Сообщение уходит ОДИН раз на номер сборки, а не на платформу: Android,
//! Windows, macOS и Linux релизятся одним тегом с одним `build`, и четыре
//! одинаковых сообщения были бы спамом. Строку журнала цикл сначала
//! «забирает» вставкой (`ON CONFLICT DO NOTHING`) и шлёт только если вставил
//! сам — гонки двух экземпляров панели поэтому безопасны.
//!
//! Текст и кнопки — событие `notify.client_update` в
//! `notification_templates::REGISTRY` (правится в админке «Уведомления»).
//! Аргументы: `{0}` версия, `{1}` «что нового» из настройки
//! `client_release_notes`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::AppState;
use crate::bot::translations::{Lang, default_language_setting, resolve_lang};

/// Категория для `notification_channel_prefs` и карточки во входящих.
pub const CATEGORY: &str = "updates";
/// Ключ события в `notification_templates::REGISTRY`.
pub const EVENT_KEY: &str = "notify.client_update";

pub const SETTING_NOTIFY: &str = "client_update_notify";
pub const SETTING_MIN_BUILD: &str = "client_min_build";
pub const SETTING_RELEASE_NOTES: &str = "client_release_notes";
pub const SETTING_LATEST_VERSION: &str = "client_latest_version";
pub const SETTING_LATEST_BUILD: &str = "client_latest_build";

/// Платформы, под которые CI собирает клиент. iOS ассета не имеет (нет
/// сертификата и таргета Network Extension), поэтому его здесь нет.
pub const PLATFORMS: [&str; 4] = ["android", "windows", "macos", "linux"];

/// Каталог раздачи панели: тот же, что у `/downloads` в `main.rs` и у
/// `update_service` (агент).
pub const DOWNLOADS_DIR: &str = "apps/caramba-panel/downloads";

/// Сколько держать прочитанный манифест, прежде чем перечитать с диска.
pub const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Период фонового цикла рассылки.
const LOOP_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Первая проверка — не сразу на старте: пусть панель поднимется, а
/// инсталлятор доложит файлы.
const LOOP_INITIAL_DELAY: Duration = Duration::from_secs(60);

/// Один файл из манифеста. У Android два APK по ABI, у Windows инсталлятор и
/// портативный ZIP.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    #[serde(default)]
    pub arch: String,
    pub file: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
}

/// Манифест платформы, как его пишет `ci-manifest.sh`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientManifest {
    pub platform: String,
    pub version: String,
    pub build: i64,
    #[serde(default)]
    pub tag: String,
    /// Главный файл платформы — по нему строится download_url.
    pub file: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
}

/// Имя манифеста платформы в `downloads/` и в ассетах релиза.
pub fn manifest_file_name(platform: &str) -> String {
    format!("Caramba-Connect-{platform}.json")
}

/// Известная ли это платформа клиента. Значение приходит query-параметром от
/// кого угодно, поэтому в имя файла попадает только строка из списка.
pub fn is_known_platform(platform: &str) -> bool {
    PLATFORMS.contains(&platform)
}

/// Разбор JSON манифеста с проверкой того, без чего он бесполезен.
pub fn parse_manifest(raw: &str) -> Result<ClientManifest> {
    let m: ClientManifest = serde_json::from_str(raw).context("манифест: не JSON нужной формы")?;
    if !is_known_platform(&m.platform) {
        anyhow::bail!("манифест: неизвестная платформа {:?}", m.platform);
    }
    if m.build <= 0 {
        anyhow::bail!(
            "манифест: build должен быть положительным, а не {}",
            m.build
        );
    }
    if m.version.trim().is_empty() {
        anyhow::bail!("манифест: пустая version");
    }
    // Имя файла уходит в URL и в путь на диске: слэши и обратные слэши здесь
    // означают, что кто-то подсунул не наш манифест.
    if m.file.trim().is_empty() || m.file.contains('/') || m.file.contains('\\') {
        anyhow::bail!("манифест: недопустимое имя файла {:?}", m.file);
    }
    Ok(m)
}

/// Читает манифест платформы с диска. `None` — файла нет или он битый (в лог).
pub fn read_manifest_from(dir: &Path, platform: &str) -> Option<ClientManifest> {
    if !is_known_platform(platform) {
        return None;
    }
    let path = dir.join(manifest_file_name(platform));
    let raw = std::fs::read_to_string(&path).ok()?;
    match parse_manifest(&raw) {
        Ok(m) if m.platform == platform => Some(m),
        Ok(m) => {
            warn!(path = %path.display(), inside = %m.platform, "манифест клиента лежит не под своей платформой");
            None
        }
        Err(e) => {
            warn!(path = %path.display(), err = %e, "манифест клиента не разобрался");
            None
        }
    }
}

fn downloads_dir() -> PathBuf {
    PathBuf::from(DOWNLOADS_DIR)
}

/// Кэш манифестов: платформа → (когда прочитан, что прочитано). Хранится
/// глобально, а не в `AppState`: потребителей два (API и цикл рассылки), и
/// оба хотят одно и то же без протаскивания ещё одного `Arc` через `main.rs`.
fn cache() -> &'static Mutex<HashMap<String, (Instant, Option<ClientManifest>)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Option<ClientManifest>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Манифест платформы с диска, не чаще раза в [`CACHE_TTL`].
pub fn cached_manifest(platform: &str) -> Option<ClientManifest> {
    if !is_known_platform(platform) {
        return None;
    }
    let now = Instant::now();
    if let Ok(map) = cache().lock()
        && let Some((read_at, value)) = map.get(platform)
        && now.duration_since(*read_at) < CACHE_TTL
    {
        return value.clone();
    }
    let fresh = read_manifest_from(&downloads_dir(), platform);
    if let Ok(mut map) = cache().lock() {
        map.insert(platform.to_string(), (now, fresh.clone()));
    }
    fresh
}

/// Сбрасывает кэш: после сохранения настроек админкой или ручного
/// обновления файлов ждать пять минут незачем.
pub fn invalidate_cache() {
    if let Ok(mut map) = cache().lock() {
        map.clear();
    }
}

/// Откуда взялась «последняя версия».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatestSource {
    /// Манифест CI в `downloads/`.
    Manifest,
    /// Ручные настройки `client_latest_version` / `client_latest_build`.
    Manual,
}

/// Последняя версия клиента для платформы — то, что уходит приложению.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestClient {
    pub platform: String,
    pub version: String,
    pub build: i64,
    /// Имя файла в `downloads/`; у ручного источника его нет.
    pub file: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub published_at: Option<String>,
    pub source: LatestSource,
}

/// Чистое правило выбора источника: манифест сильнее ручной настройки, ручная
/// настройка засчитывается только целиком (и версия, и сборка).
pub fn resolve_latest(
    platform: &str,
    manifest: Option<&ClientManifest>,
    manual_version: &str,
    manual_build: &str,
) -> Option<LatestClient> {
    if let Some(m) = manifest {
        return Some(LatestClient {
            platform: platform.to_string(),
            version: m.version.clone(),
            build: m.build,
            file: Some(m.file.clone()),
            size: Some(m.size),
            sha256: Some(m.sha256.clone()),
            published_at: Some(m.published_at.clone()).filter(|p| !p.is_empty()),
            source: LatestSource::Manifest,
        });
    }
    let version = manual_version.trim();
    let build = manual_build.trim().parse::<i64>().ok().filter(|b| *b > 0)?;
    if version.is_empty() {
        return None;
    }
    Some(LatestClient {
        platform: platform.to_string(),
        version: version.to_string(),
        build,
        file: None,
        size: None,
        sha256: None,
        published_at: None,
        source: LatestSource::Manual,
    })
}

/// Минимальная сборка для платформы: платформенная настройка сильнее
/// глобальной; мусор и пустота — ноль, то есть «не требовать».
pub fn min_build_for(platform_specific: &str, global: &str) -> i64 {
    let parse = |v: &str| v.trim().parse::<i64>().ok().filter(|b| *b > 0);
    parse(platform_specific)
        .or_else(|| parse(global))
        .unwrap_or(0)
}

/// Разбор `X-Caramba-App-Version` из заголовка: печатный ASCII, обрезка, не
/// длиннее 32 символов. Значение недоверенное и попадает только в колонку
/// для показа.
pub fn sanitize_app_version(raw: Option<&str>) -> Option<String> {
    let cleaned: String = raw?
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(32)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

// ---------------------------------------------------------------------------
// Рассылка
// ---------------------------------------------------------------------------

/// Что сделать с набором манифестов, увиденных на диске, при известном
/// журнале.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Журнал пуст: записать всё молча (свежая установка / первый запуск).
    Seed(Vec<(String, i64, String)>),
    /// Появилась сборка новее журнала: разослать один раз и записать все
    /// строки этой сборки.
    Notify {
        build: i64,
        version: String,
        rows: Vec<(String, i64, String)>,
    },
    /// Новее ничего нет; строки платформ, которых ещё нет в журнале
    /// (например, Windows-сборка того же build доехала позже), записать молча.
    Record(Vec<(String, i64, String)>),
}

/// Чистая логика цикла: `manifests` — что лежит на диске, `notified` — пары
/// (платформа, build) из журнала.
pub fn decide(manifests: &[ClientManifest], notified: &[(String, i64)]) -> Decision {
    let known = |m: &ClientManifest| {
        notified
            .iter()
            .any(|(p, b)| *p == m.platform && *b == m.build)
    };
    let rows_of = |pred: &dyn Fn(&ClientManifest) -> bool| {
        manifests
            .iter()
            .filter(|m| pred(m))
            .map(|m| (m.platform.clone(), m.build, m.version.clone()))
            .collect::<Vec<_>>()
    };

    if notified.is_empty() {
        return Decision::Seed(rows_of(&|_| true));
    }
    let high_water = notified.iter().map(|(_, b)| *b).max().unwrap_or(0);
    let newest = manifests.iter().max_by_key(|m| m.build);
    match newest {
        Some(m) if m.build > high_water => Decision::Notify {
            build: m.build,
            version: m.version.clone(),
            rows: rows_of(&|x| x.build == m.build && !known(x)),
        },
        _ => Decision::Record(rows_of(&|x| !known(x))),
    }
}

/// Строка «что нового» для подстановки: пустая настройка превращается в
/// нейтральную фразу, чтобы в сообщении не осталось пустого места.
pub fn release_notes_text(raw: &str, lang: Lang) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return match lang {
            Lang::Ru => "Исправления и улучшения стабильности.".to_string(),
            Lang::En => "Fixes and stability improvements.".to_string(),
        };
    }
    // Текст уходит в HTML parse mode: экранируем то, что Telegram примет за
    // разметку, иначе одна «<» в заметках уронит всё сообщение.
    trimmed
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Забирает строку журнала за собой. `true` — вставили мы.
async fn claim(pool: &PgPool, platform: &str, build: i64, version: &str) -> Result<bool> {
    let affected = sqlx::query(
        "INSERT INTO client_release_notices (platform, build, version) VALUES ($1, $2, $3) \
         ON CONFLICT (platform, build) DO NOTHING",
    )
    .bind(platform)
    .bind(build)
    .bind(version)
    .execute(pool)
    .await
    .context("client releases: не удалось записать журнал")?
    .rows_affected();
    Ok(affected == 1)
}

async fn notified_pairs(pool: &PgPool) -> Result<Vec<(String, i64)>> {
    sqlx::query_as("SELECT platform, build FROM client_release_notices")
        .fetch_all(pool)
        .await
        .context("client releases: не удалось прочитать журнал")
}

/// Журнал для админки: последние записи, новые сверху.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NoticeRow {
    pub platform: String,
    pub build: i64,
    pub version: String,
    pub sent_count: i64,
    pub notified_at: chrono::DateTime<chrono::Utc>,
}

pub async fn recent_notices(pool: &PgPool, limit: i64) -> Vec<NoticeRow> {
    sqlx::query_as(
        "SELECT platform, build, version, sent_count, notified_at \
         FROM client_release_notices ORDER BY build DESC, platform LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Получатель рассылки.
#[derive(Debug, Clone)]
struct Recipient {
    user_id: i64,
    tg_id: i64,
    language_code: Option<String>,
    dm_enabled: bool,
}

async fn recipients(pool: &PgPool) -> Result<Vec<Recipient>> {
    let rows: Vec<(i64, i64, Option<String>, bool)> = sqlx::query_as(
        r#"
        SELECT u.id,
               u.tg_id,
               u.language_code,
               COALESCE(
                   (SELECT p.enabled FROM notification_channel_prefs p
                     WHERE p.user_id = u.id AND p.category = $1 AND p.channel = 'bot_dm'),
                   TRUE
               ) AS dm_enabled
        FROM users u
        WHERE u.tg_id IS NOT NULL
          AND u.tg_id > 0
          AND u.is_banned IS NOT TRUE
        ORDER BY u.id
        "#,
    )
    .bind(CATEGORY)
    .fetch_all(pool)
    .await
    .context("client releases: не удалось прочитать получателей")?;
    Ok(rows
        .into_iter()
        .map(|(user_id, tg_id, language_code, dm_enabled)| Recipient {
            user_id,
            tg_id,
            language_code,
            dm_enabled,
        })
        .collect())
}

/// Рассылает «вышла версия {0}» всем tg-пользователям. Возвращает число
/// доставленных в бот сообщений; карточка во входящие ложится каждому.
pub async fn broadcast_release(state: &AppState, version: &str, build: i64) -> Result<usize> {
    let list = recipients(&state.pool).await?;
    let default_lang = default_language_setting(&state.pool).await;
    let notes_raw = state
        .settings
        .get_or_default(SETTING_RELEASE_NOTES, "")
        .await;
    let version_label = format!("{version} ({build})");
    let mut sent = 0usize;
    for r in &list {
        let lang = resolve_lang(r.language_code.as_deref(), default_lang.as_deref());
        let notes = release_notes_text(&notes_raw, lang);
        let rendered = state
            .notification_templates
            .render_with(&state.settings, EVENT_KEY, lang, &[&version_label, &notes])
            .await;
        if r.dm_enabled {
            match state
                .bot_manager
                .send_rich_notification(r.tg_id, rendered.payload)
                .await
            {
                Ok(()) => sent += 1,
                Err(e) => warn!(
                    user_id = r.user_id,
                    tg_id = r.tg_id,
                    err = %e,
                    "client releases: сообщение не доставлено"
                ),
            }
        }
        if let Err(e) = state
            .notifications_svc
            .create_inbox_only(
                r.user_id,
                CATEGORY,
                "info",
                &rendered.title,
                &rendered.body,
                Some(serde_json::json!({ "version": version, "build": build })),
            )
            .await
        {
            warn!(user_id = r.user_id, err = %e, "client releases: карточка во входящие не записана");
        }
    }
    Ok(sent)
}

/// Один проход: читает манифесты, сверяет с журналом, при необходимости шлёт.
/// Возвращает число ушедших в бот сообщений.
pub async fn sweep(state: &AppState) -> Result<usize> {
    let manifests: Vec<ClientManifest> = PLATFORMS
        .iter()
        .filter_map(|p| cached_manifest(p))
        .collect();
    if manifests.is_empty() {
        return Ok(0);
    }
    let notified = notified_pairs(&state.pool).await?;
    match decide(&manifests, &notified) {
        Decision::Seed(rows) => {
            for (p, b, v) in rows {
                claim(&state.pool, &p, b, &v).await?;
            }
            info!("client releases: журнал пуст, текущие манифесты записаны без рассылки");
            Ok(0)
        }
        Decision::Record(rows) => {
            for (p, b, v) in rows {
                claim(&state.pool, &p, b, &v).await?;
            }
            Ok(0)
        }
        Decision::Notify {
            build,
            version,
            rows,
        } => {
            // Забираем ВСЕ строки сборки до отправки: тот экземпляр, который
            // вставил хотя бы одну, и шлёт. Второй экземпляр (или следующий
            // проход) не вставит ничего и промолчит.
            let mut claimed_any = false;
            for (p, b, v) in &rows {
                if claim(&state.pool, p, *b, v).await? {
                    claimed_any = true;
                }
            }
            if !claimed_any {
                return Ok(0);
            }
            let enabled = state
                .settings
                .get_bool_or_default(SETTING_NOTIFY, true)
                .await;
            if !enabled {
                info!(
                    build,
                    "client releases: новая сборка записана, рассылка выключена настройкой"
                );
                return Ok(0);
            }
            let sent = broadcast_release(state, &version, build).await?;
            let _ =
                sqlx::query("UPDATE client_release_notices SET sent_count = $1 WHERE build = $2")
                    .bind(sent as i64)
                    .bind(build)
                    .execute(&state.pool)
                    .await;
            info!(build, version = %version, sent, "client releases: рассылка о новой версии выполнена");
            Ok(sent)
        }
    }
}

/// Фоновый цикл: раз в десять минут. Запускается из `main.rs` рядом с
/// онбордингом.
pub async fn run_client_release_loop(state: AppState) {
    tokio::time::sleep(LOOP_INITIAL_DELAY).await;
    loop {
        match sweep(&state).await {
            Ok(_) => {}
            Err(e) => error!(err = %e, "client releases: проход не выполнен"),
        }
        tokio::time::sleep(LOOP_INTERVAL).await;
    }
}

/// Записывает версию приложения в лизу устройства аккаунта. Best-effort и
/// дёшево: индекс `(user_id, client_device_id)` уже есть, обновление
/// пропускается, если значение не изменилось.
pub async fn record_device_app_version(
    pool: &PgPool,
    user_id: i64,
    client_device_id: &str,
    app_version: &str,
) {
    if let Err(e) = sqlx::query(
        "UPDATE subscription_device_leases SET app_version = $3 \
         WHERE user_id = $1 AND client_device_id = $2 AND app_version IS DISTINCT FROM $3",
    )
    .bind(user_id)
    .bind(client_device_id)
    .bind(app_version)
    .execute(pool)
    .await
    {
        warn!(user_id, err = %e, "client releases: версия приложения в лизу не записана");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(platform: &str, build: i64) -> ClientManifest {
        ClientManifest {
            platform: platform.to_string(),
            version: "1.0.0".to_string(),
            build,
            tag: format!("client-v1.0.0+{build}"),
            file: format!("Caramba-Connect-{platform}.bin"),
            size: 10,
            sha256: "ab".repeat(32),
            published_at: "2026-09-11T00:00:00Z".to_string(),
            files: vec![],
        }
    }

    /// Формат ci-manifest.sh разбирается целиком, включая список файлов.
    #[test]
    fn parses_the_ci_manifest_format() {
        let raw = r#"{
          "platform": "android", "version": "1.0.0", "build": 110,
          "tag": "client-v1.0.0+110", "file": "Caramba-Connect-Android-arm64.apk",
          "size": 3, "sha256": "ba78", "published_at": "2026-09-11T00:00:00Z",
          "files": [{"arch":"arm64","file":"Caramba-Connect-Android-arm64.apk","size":3,"sha256":"ba78"},
                    {"arch":"armv7","file":"Caramba-Connect-Android-armv7.apk","size":6,"sha256":"bef5"}]
        }"#;
        let m = parse_manifest(raw).expect("валидный манифест");
        assert_eq!(m.platform, "android");
        assert_eq!(m.build, 110);
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.files[1].arch, "armv7");
        assert_eq!(
            manifest_file_name("android"),
            "Caramba-Connect-android.json"
        );
    }

    #[test]
    fn rejects_garbage_manifests() {
        assert!(parse_manifest("{}").is_err());
        assert!(
            parse_manifest(r#"{"platform":"ios","version":"1","build":1,"file":"a"}"#).is_err(),
            "iOS ассета нет"
        );
        assert!(
            parse_manifest(r#"{"platform":"linux","version":"1","build":0,"file":"a"}"#).is_err()
        );
        assert!(
            parse_manifest(r#"{"platform":"linux","version":"1","build":1,"file":"../x"}"#)
                .is_err(),
            "слэш в имени файла — не наш манифест"
        );
        assert!(!is_known_platform("ios"));
        assert!(!is_known_platform("../etc"));
    }

    /// Манифест с диска по чужому имени платформы не засчитывается.
    #[test]
    fn reads_manifest_from_a_directory() {
        let dir = std::env::temp_dir().join(format!("caramba-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(manifest_file_name("linux")),
            serde_json::to_string(&manifest("linux", 111)).unwrap(),
        )
        .unwrap();
        // Под именем windows лежит манифест linux — не наш.
        std::fs::write(
            dir.join(manifest_file_name("windows")),
            serde_json::to_string(&manifest("linux", 111)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            read_manifest_from(&dir, "linux").map(|m| m.build),
            Some(111)
        );
        assert!(read_manifest_from(&dir, "windows").is_none());
        assert!(read_manifest_from(&dir, "macos").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Манифест сильнее ручной настройки; ручная засчитывается только целиком.
    #[test]
    fn manifest_beats_manual_settings() {
        let m = manifest("windows", 110);
        let latest = resolve_latest("windows", Some(&m), "9.9.9", "999").unwrap();
        assert_eq!(latest.build, 110);
        assert_eq!(latest.source, LatestSource::Manifest);
        assert_eq!(latest.file.as_deref(), Some("Caramba-Connect-windows.bin"));

        let manual = resolve_latest("windows", None, " 1.0.1 ", "112").unwrap();
        assert_eq!(manual.version, "1.0.1");
        assert_eq!(manual.build, 112);
        assert_eq!(manual.source, LatestSource::Manual);
        assert!(manual.file.is_none());

        assert!(resolve_latest("windows", None, "1.0.1", "").is_none());
        assert!(resolve_latest("windows", None, "", "112").is_none());
        assert!(resolve_latest("windows", None, "1.0.1", "мусор").is_none());
    }

    #[test]
    fn min_build_prefers_platform_over_global_and_ignores_garbage() {
        assert_eq!(min_build_for("", ""), 0);
        assert_eq!(min_build_for("", "100"), 100);
        assert_eq!(min_build_for("105", "100"), 105);
        assert_eq!(min_build_for("abc", "100"), 100);
        assert_eq!(min_build_for("0", "-5"), 0);
    }

    #[test]
    fn app_version_header_is_sanitized() {
        assert_eq!(
            sanitize_app_version(Some(" 1.0.0+110\r\n")).as_deref(),
            Some("1.0.0+110")
        );
        assert_eq!(sanitize_app_version(Some("   ")), None);
        assert_eq!(sanitize_app_version(None), None);
        assert_eq!(
            sanitize_app_version(Some(&"9".repeat(100))).unwrap().len(),
            32
        );
    }

    /// Пустой журнал — записать молча: свежая установка не начинается с
    /// массовой рассылки.
    #[test]
    fn empty_journal_seeds_silently() {
        let d = decide(&[manifest("android", 109), manifest("linux", 109)], &[]);
        assert_eq!(
            d,
            Decision::Seed(vec![
                ("android".into(), 109, "1.0.0".into()),
                ("linux".into(), 109, "1.0.0".into()),
            ])
        );
    }

    /// Ровно то, что делает бэкфилл миграции: 109 записан, манифест 109 на
    /// диске — тишина; 110 — одна рассылка на сборку, все платформы одной
    /// строкой каждая.
    #[test]
    fn notifies_once_per_build_not_per_platform() {
        let journal: Vec<(String, i64)> = PLATFORMS.iter().map(|p| (p.to_string(), 109)).collect();
        assert_eq!(
            decide(&[manifest("android", 109)], &journal),
            Decision::Record(vec![])
        );
        let d = decide(
            &[
                manifest("android", 110),
                manifest("windows", 110),
                manifest("macos", 109),
            ],
            &journal,
        );
        assert_eq!(
            d,
            Decision::Notify {
                build: 110,
                version: "1.0.0".into(),
                rows: vec![
                    ("android".into(), 110, "1.0.0".into()),
                    ("windows".into(), 110, "1.0.0".into()),
                ],
            }
        );
    }

    /// Windows-сборка того же build, доехавшая позже, записывается молча —
    /// второго сообщения не будет.
    #[test]
    fn late_platform_of_an_announced_build_is_recorded_without_a_message() {
        let journal = vec![("android".to_string(), 110)];
        let d = decide(
            &[manifest("android", 110), manifest("windows", 110)],
            &journal,
        );
        assert_eq!(
            d,
            Decision::Record(vec![("windows".into(), 110, "1.0.0".into())])
        );
    }

    /// Откат на диске (старый манифест) не считается новой версией.
    #[test]
    fn older_manifest_never_notifies() {
        let journal = vec![("android".to_string(), 110)];
        assert_eq!(
            decide(&[manifest("android", 108)], &journal),
            Decision::Record(vec![("android".into(), 108, "1.0.0".into())])
        );
    }

    #[test]
    fn release_notes_are_escaped_or_defaulted() {
        assert_eq!(
            release_notes_text("  ", Lang::Ru),
            "Исправления и улучшения стабильности."
        );
        assert_eq!(
            release_notes_text("a <b> & c", Lang::En),
            "a &lt;b&gt; &amp; c"
        );
    }

    /// Бэкфилл миграции и константы модуля не должны разъехаться.
    #[test]
    fn migration_backfills_every_platform_and_settings() {
        let migration = include_str!(
            "../../../../libs/caramba-db/migrations/20260911190000_client_releases.sql"
        );
        for p in PLATFORMS {
            assert!(
                migration.contains(&format!("('{p}', 109")),
                "миграция не бэкфиллит платформу {p}"
            );
        }
        for key in [
            SETTING_NOTIFY,
            SETTING_MIN_BUILD,
            SETTING_RELEASE_NOTES,
            SETTING_LATEST_VERSION,
            SETTING_LATEST_BUILD,
        ] {
            assert!(migration.contains(key), "миграция не заводит {key}");
        }
        assert!(migration.contains("app_version"));
    }
}
