use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Проверяет SHA-256 хеш загруженных байт.
/// Возвращает false если expected_hex невалиден или хеши не совпадают.
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    let expected = expected_hex.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = format!("{:x}", hasher.finalize());
    actual == expected
}

/// Скачивает бинарник по URL, проверяет SHA-256 (если задан), атомарно заменяет
/// текущий исполняемый файл. После возврата Ok(()) нужно перезапустить процесс.
pub async fn apply_self_update(
    asset_url: &str,
    expected_sha256: Option<&str>,
    tmp_prefix: &str,
) -> anyhow::Result<()> {
    let response = reqwest::Client::new()
        .get(asset_url)
        .send()
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;

    if let Some(hash) = expected_sha256
        && !hash.trim().is_empty()
        && !verify_sha256(&bytes, hash)
    {
        return Err(anyhow::anyhow!("SHA256 mismatch for downloaded binary"));
    }

    let exe_path = std::env::current_exe()?;
    let exe_parent = exe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Failed to detect executable parent directory"))?;

    // Уникальный временный файл чтобы избежать конкурентных обновлений
    let tmp_path = exe_parent.join(format!(
        ".{}.update.{}.tmp",
        tmp_prefix,
        uuid::Uuid::new_v4().to_string().replace('-', "")
    ));

    tokio::fs::write(&tmp_path, &bytes).await?;

    // Устанавливаем права ПЕРЕД rename, чтобы новый бинарник сразу был исполняемым.
    // Ошибка прав → удаляем tmp файл и прокидываем ошибку.
    if let Err(e) = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755)) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!(
            "Failed to set permissions on update binary: {}",
            e
        ));
    }

    // rename() — атомарная операция на том же разделе ФС.
    // При ошибке rename удаляем tmp файл чтобы не оставлять мусор.
    if let Err(e) = std::fs::rename(&tmp_path, &exe_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!(
            "Failed to atomically replace binary at {}: {}",
            exe_path.display(),
            e
        ));
    }

    Ok(())
}

/// Перезапускает systemd-сервис через `systemctl --no-block restart <service_name>`.
/// Логирует результат через tracing; не паникует при ошибке.
///
/// `--no-block` обязателен: вызывающий процесс — это и есть перезапускаемый
/// сервис. Блокирующий `restart` ждёт завершения задания, а задание начинает с
/// SIGTERM самому вызывающему — команда «падает» с сигналом 15, а systemd
/// засчитывает лишний старт. С лимитом по умолчанию (5 за 10 с) это кончалось
/// `start-limit-hit`, и узел оставался без агента (Canada, 2026-09-01).
pub fn restart_service(service_name: &str) {
    use std::os::unix::process::ExitStatusExt;

    match Command::new("systemctl")
        .args(["--no-block", "restart", service_name])
        .status()
    {
        Ok(status) => {
            match classify_restart_status(status.success(), status.code(), status.signal()) {
                RestartOutcome::Requested => {
                    tracing::info!("{} restart requested after self-update.", service_name);
                }
                // Не ошибка: задание systemd гасит нашу cgroup вместе с только что
                // запущенным `systemctl`. Перезапуск при этом уже принят, поэтому
                // прежний ERROR «Manual restart required» врал оператору.
                RestartOutcome::KilledBySignal(sig) => {
                    tracing::info!(
                        "systemctl для {} убит сигналом {} — ожидаемо для --no-block restart собственного юнита; перезапуск принят, вмешательство не нужно.",
                        service_name,
                        sig
                    );
                }
                RestartOutcome::Failed(code) => {
                    tracing::error!(
                        "Failed to restart {} (exit code: {:?}). Manual restart required.",
                        service_name,
                        code
                    );
                }
            }
        }
        Err(e) => {
            tracing::error!(
                "Failed to execute systemctl restart for {}: {}",
                service_name,
                e
            );
        }
    }
}

/// Считает SHA-256 файла и возвращает hex в нижнем регистре.
///
/// Нужен, чтобы агент мог сравнить УЖЕ ЗАПУЩЕННЫЙ бинарник с ассетом релиза:
/// релизы v0.9.81…v0.9.87 собирались с незаходившей версией крейта 0.9.80, и
/// узел бесконечно «обновлялся» сам на себя (~4400 циклов в час). Совпадение
/// хешей — единственный надёжный признак «в релизе лежит то же самое».
///
/// Читаем потоком: бинарник агента — десятки мегабайт, целиком в память его
/// тянуть незачем.
pub fn sha256_hex_of_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Чем закончился запрос перезапуска сервиса.
///
/// Нужен отдельным типом, чтобы классификацию можно было проверить тестом:
/// собрать настоящий `ExitStatus` с сигналом в тесте нельзя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartOutcome {
    /// systemd принял задание — перезапуск идёт.
    Requested,
    /// Процесс `systemctl` убит сигналом. Это НОРМА для `--no-block restart`:
    /// задание останавливает наш же юнит, а вместе с ним и всю его cgroup,
    /// включая только что запущенный `systemctl`. Перезапуск при этом
    /// происходит, ручное вмешательство не нужно.
    KilledBySignal(i32),
    /// Ненулевой код возврата НЕ от сигнала — вот это настоящая ошибка.
    Failed(Option<i32>),
}

/// Классифицирует результат `systemctl --no-block restart`.
///
/// Раньше любой неуспех логировался как ERROR «Manual restart required», хотя
/// в 100% случаев это был SIGTERM самому `systemctl` — оператора гоняли чинить
/// то, что уже работало.
pub fn classify_restart_status(
    success: bool,
    code: Option<i32>,
    signal: Option<i32>,
) -> RestartOutcome {
    if success {
        return RestartOutcome::Requested;
    }
    match signal {
        Some(sig) => RestartOutcome::KilledBySignal(sig),
        None => RestartOutcome::Failed(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_a_plain_request() {
        assert_eq!(
            classify_restart_status(true, Some(0), None),
            RestartOutcome::Requested
        );
    }

    /// Главный случай инцидента: `--no-block restart` возвращает сигнал 15,
    /// потому что systemd гасит cgroup нашего юнита вместе с потомком.
    #[test]
    fn sigterm_from_our_own_restart_is_not_a_failure() {
        assert_eq!(
            classify_restart_status(false, None, Some(15)),
            RestartOutcome::KilledBySignal(15)
        );
        assert_eq!(
            classify_restart_status(false, None, Some(9)),
            RestartOutcome::KilledBySignal(9)
        );
    }

    /// Сигнал главнее кода: если ядро сообщило и то и другое, это всё равно
    /// смерть по сигналу, а не отказ systemd.
    #[test]
    fn a_signal_wins_over_a_reported_code() {
        assert_eq!(
            classify_restart_status(false, Some(143), Some(15)),
            RestartOutcome::KilledBySignal(15)
        );
    }

    #[test]
    fn a_nonzero_code_without_a_signal_is_a_real_failure() {
        assert_eq!(
            classify_restart_status(false, Some(1), None),
            RestartOutcome::Failed(Some(1))
        );
        assert_eq!(
            classify_restart_status(false, None, None),
            RestartOutcome::Failed(None)
        );
    }

    #[test]
    fn file_hash_matches_the_hash_of_the_same_bytes() {
        let dir = std::env::temp_dir().join(format!(
            "caramba-self-update-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload.bin");
        // Больше буфера чтения (64 КиБ), чтобы проверить именно потоковый путь.
        let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();

        let hex = sha256_hex_of_file(&path).unwrap();
        assert!(verify_sha256(&bytes, &hex));
        assert_eq!(hex.len(), 64);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_bogus_hash() {
        assert!(sha256_hex_of_file(std::path::Path::new("/nonexistent/caramba/x")).is_err());
    }
}
