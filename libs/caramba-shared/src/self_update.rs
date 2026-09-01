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
    match Command::new("systemctl")
        .args(["--no-block", "restart", service_name])
        .status()
    {
        Ok(status) if status.success() => {
            tracing::info!("{} restart requested after self-update.", service_name);
        }
        Ok(status) => {
            tracing::error!(
                "Failed to restart {} (exit status: {}). Manual restart required.",
                service_name,
                status
            );
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
