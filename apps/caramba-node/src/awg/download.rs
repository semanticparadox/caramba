//! Доставка бинаря `amneziawg-go` на узел.
//!
//! Тем же механизмом, что и sing-box (`ensure_singbox_with_v2ray_api` в
//! main.rs): ассет из релиза caramba, один доверенный источник на всё, что
//! ставится на узлы. Собственных релизов с бинарями у amnezia-vpn/amneziawg-go
//! нет вовсе (в репозитории только теги исходников), поэтому качать «прямо у
//! апстрима» нечего — версия апстрима зафиксирована константой и собирается в
//! нашем релизном пайплайне.
//!
//! Проверка sha256 обязательна и работает fail-closed: пока хеш не зафиксирован
//! константой, бинарь не скачивается и не ставится. Подменённый бинарь на узле
//! означал бы чужой код с сетевым доступом и правами root.

use std::path::{Path, PathBuf};
use std::time::Duration;

use caramba_shared::self_update::sha256_hex_of_file;
use tracing::{info, warn};

/// Версия исходников amneziawg-go, из которой собран наш ассет.
///
/// Верхний тег апстрима на момент внедрения (проверено по
/// github.com/amnezia-vpn/amneziawg-go/tags). Апстрим выпускает версии часто и
/// именно в ответ на новые сигнатуры блокировок, поэтому константа обязана
/// подниматься вместе с релизом узла, а не жить годами.
pub const AWG_GO_VERSION: &str = "v3.1.20260828";

/// sha256 ассета `amneziawg-go` (linux/amd64) из релиза caramba.
///
/// Пустая строка = «хеш ещё не зафиксирован». В этом состоянии установка
/// бинаря НЕ выполняется: выдумывать хеш нельзя, а ставить непроверенный
/// бинарь — тем более. Заполняется в том же коммите, где релизный пайплайн
/// начинает собирать и публиковать ассет (`sha256sum release/amneziawg-go`).
pub const AWG_GO_SHA256: &str = "1123b55049dbbb566d5f24af616604c0aea512e71ec871912e38fa57ce491a08";

/// Куда кладём бинарь. Отдельно от /usr/bin: это наш файл, а не системный.
pub const AWG_GO_PATH: &str = "/opt/caramba/amneziawg-go";

/// Переменная окружения для ручной проверки до первого релиза с ассетом.
///
/// Пока константа пуста, узел не поставит бинарь вообще — а проверить AWG на
/// живой ноде нужно раньше, чем релизный пайплайн научится собирать ассет.
/// Переменная НЕ отключает проверку: она лишь задаёт ожидаемый хеш вместо
/// константы, так что непроверенный бинарь по-прежнему невозможен.
pub const AWG_GO_SHA256_ENV: &str = "CARAMBA_AWG_SHA256";

/// Ожидаемый хеш: константа, а при её отсутствии — значение из окружения.
/// `None` — проверять не с чем, значит ставить нечего.
pub fn expected_sha256() -> Option<String> {
    let from_env = std::env::var(AWG_GO_SHA256_ENV).ok();
    let resolved = resolve_expected(AWG_GO_SHA256, from_env.as_deref());
    if resolved.is_some() && AWG_GO_SHA256.trim().is_empty() {
        warn!(
            "AWG: sha256 взят из {AWG_GO_SHA256_ENV}, а не из кода — так можно только для ручной проверки"
        );
    }
    resolved
}

/// Чистое правило выбора хеша, вынесенное отдельно ради тестов: трогать
/// переменные окружения в тестах нельзя, они глобальны на весь процесс и
/// параллельный тест увидел бы чужое значение.
fn resolve_expected(pinned: &str, from_env: Option<&str>) -> Option<String> {
    let pinned = pinned.trim();
    if !pinned.is_empty() {
        return Some(pinned.to_string());
    }
    from_env
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Ассет в релизе caramba той же версии, что и сам агент.
///
/// Версии узлового агента и ассетов в релизе всегда совпадают — так же устроена
/// доставка sing-box, и второй схемы именования на узлах заводить незачем.
pub fn download_url(agent_version: &str) -> String {
    format!(
        "https://github.com/semanticparadox/caramba/releases/download/v{}/amneziawg-go",
        agent_version.trim().trim_start_matches('v')
    )
}

/// Сверка хешей без учёта регистра и пробелов.
pub fn hashes_match(expected: &str, actual: &str) -> bool {
    let e = expected.trim().to_ascii_lowercase();
    let a = actual.trim().to_ascii_lowercase();
    !e.is_empty() && e == a
}

/// Убедиться, что на узле лежит проверенный бинарь, и вернуть путь к нему.
pub async fn ensure_amneziawg_go(client: &reqwest::Client) -> anyhow::Result<PathBuf> {
    let expected = expected_sha256().ok_or_else(|| {
        anyhow::anyhow!(
            "sha256 бинаря amneziawg-go не зафиксирован (AWG_GO_SHA256 пуст) — \
             установка запрещена; заполните константу после публикации ассета релизом"
        )
    })?;

    let path = PathBuf::from(AWG_GO_PATH);
    if path.exists() {
        match sha256_hex_of_file(&path) {
            Ok(actual) if hashes_match(&expected, &actual) => return Ok(path),
            Ok(actual) => warn!(
                "AWG: {} не совпал с зафиксированным sha256 ({} вместо {}) — перекачиваю",
                path.display(),
                &actual[..actual.len().min(12)],
                &expected[..expected.len().min(12)]
            ),
            Err(e) => warn!(
                "AWG: не посчитать sha256 {} ({e}) — перекачиваю",
                path.display()
            ),
        }
    }

    let url = download_url(env!("CARGO_PKG_VERSION"));
    info!("AWG: качаю amneziawg-go ({AWG_GO_VERSION}) из релиза");

    // Таймауты как у sing-box: зависшая загрузка не должна держать цикл агента.
    let bytes = client
        .get(&url)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("amneziawg-go не скачался: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("релиз не отдал amneziawg-go: {e}"))?
        .bytes()
        .await
        .map_err(|e| anyhow::anyhow!("amneziawg-go не дочитан: {e}"))?;

    // Пишем во временный файл рядом с целевым: так подмена на месте атомарна,
    // и полускачанный бинарь никогда не окажется по рабочему пути.
    let tmp = PathBuf::from(format!("{AWG_GO_PATH}.tmp"));
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&tmp, &bytes).await?;

    let actual = sha256_hex_of_file(&tmp)?;
    if !hashes_match(&expected, &actual) {
        let _ = tokio::fs::remove_file(&tmp).await;
        anyhow::bail!(
            "sha256 скачанного amneziawg-go не совпал с зафиксированным — бинарь отброшен"
        );
    }

    set_executable(&tmp).await?;
    tokio::fs::rename(&tmp, &path).await?;
    info!("AWG: amneziawg-go установлен в {}", path.display());
    Ok(path)
}

async fn set_executable(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(path).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(path, perms).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ассет лежит в релизе caramba под версией агента — та же схема, что у
    /// sing-box. Разъехавшееся имя означало бы 404 на каждом узле.
    #[test]
    fn the_asset_url_follows_the_release_of_the_running_agent() {
        assert_eq!(
            download_url("0.9.80"),
            "https://github.com/semanticparadox/caramba/releases/download/v0.9.80/amneziawg-go"
        );
        assert_eq!(download_url("v0.9.80"), download_url("0.9.80"));
    }

    /// Регистр и пробелы в хеше не должны решать судьбу узла.
    #[test]
    fn hash_comparison_ignores_case_and_padding() {
        assert!(hashes_match("  ABCDEF  ", "abcdef"));
        assert!(!hashes_match("abcdef", "abcdee"));
    }

    /// Пустой хеш никогда не считается совпадением — иначе незаполненная
    /// константа тихо разрешила бы любой бинарь.
    #[test]
    fn an_empty_expected_hash_never_matches() {
        assert!(!hashes_match("", ""));
        assert!(!hashes_match("", "abcdef"));
    }

    /// Пока хеш не зафиксирован, установка запрещена целиком.
    #[tokio::test]
    async fn without_a_pinned_hash_nothing_is_installed() {
        // Переменная окружения тоже задаёт хеш — тогда сценарий не применим.
        if expected_sha256().is_some() {
            // Константу уже заполнили — этот сценарий больше не применим.
            return;
        }
        let client = reqwest::Client::new();
        let err = ensure_amneziawg_go(&client).await.unwrap_err().to_string();
        assert!(
            err.contains("sha256"),
            "ожидали отказ по хешу, получили: {err}"
        );
    }

    /// Константа в коде сильнее окружения: зафиксированный хеш нельзя
    /// подменить переменной на узле.
    #[test]
    fn the_pinned_constant_wins_over_the_environment() {
        assert_eq!(
            resolve_expected("abc123", Some("deadbeef")).as_deref(),
            Some("abc123")
        );
    }

    /// Пока константа пуста, хеш можно принести окружением — но именно хеш,
    /// а не разрешение ставить что попало.
    #[test]
    fn the_environment_fills_in_only_while_the_constant_is_empty() {
        assert_eq!(
            resolve_expected("", Some("  DEADBEEF  ")).as_deref(),
            Some("DEADBEEF")
        );
        assert_eq!(resolve_expected("", Some("   ")), None);
        assert_eq!(resolve_expected("", None), None);
    }

    /// Версия апстрима зафиксирована и выглядит как тег amneziawg-go.
    #[test]
    fn the_upstream_version_is_pinned() {
        assert!(AWG_GO_VERSION.starts_with('v'));
        assert!(AWG_GO_VERSION.len() > 3);
    }
}
