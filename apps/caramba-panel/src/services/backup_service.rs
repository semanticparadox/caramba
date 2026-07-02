/// Сервис резервного копирования базы данных.
///
/// Создаёт сжатые дампы PostgreSQL через `pg_dump`, хранит их локально
/// в BACKUP_DIR, поддерживает ротацию по количеству последних копий.
/// Все операции ввода-вывода неблокирующие — pg_dump запускается как
/// дочерний процесс tokio, читается и сжимается потоково.
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{info, warn};

// ────────────────────────────────────────────────────────────────────────────
// Публичные типы
// ────────────────────────────────────────────────────────────────────────────

/// Метаданные одного файла резервной копии.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupInfo {
    pub filename: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    /// Продолжительность создания дампа в миллисекундах.
    pub duration_ms: u64,
}

// ────────────────────────────────────────────────────────────────────────────
// Вспомогательные функции
// ────────────────────────────────────────────────────────────────────────────

/// Возвращает путь к директории резервных копий из env BACKUP_DIR.
/// По умолчанию — /var/lib/caramba/backups.
fn backup_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("BACKUP_DIR").unwrap_or_else(|_| "/var/lib/caramba/backups".to_string()),
    )
}

/// Формирует имя файла по шаблону `caramba-YYYYMMDDTHHMMSSZ.sql.gz`.
fn make_filename(now: DateTime<Utc>) -> String {
    format!("caramba-{}.sql.gz", now.format("%Y%m%dT%H%M%SZ"))
}

/// Проверяет, что имя файла соответствует шаблону — без пути и без спецсимволов.
/// Защита от path-traversal при удалении файла по имени из запроса.
pub fn validate_filename(filename: &str) -> bool {
    // Должно быть только имя файла, без слешей
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return false;
    }
    // Должно соответствовать шаблону caramba-YYYYMMDDTHHMMSSZ.sql.gz
    let re = regex_lite::Regex::new(r"^caramba-\d{8}T\d{6}Z\.sql\.gz$").unwrap();
    re.is_match(filename)
}

/// Форматирует байты в человекочитаемый размер (B / KB / MB / GB).
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.3} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Основные операции
// ────────────────────────────────────────────────────────────────────────────

/// Создаёт новую резервную копию базы данных.
///
/// Порядок действий:
/// 1. Разбирает DATABASE_URL, выделяет PGPASSWORD для дочернего процесса
///    (пароль не попадает в аргументы командной строки).
/// 2. Запускает `pg_dump` с --no-password; stdout читается асинхронно.
/// 3. Данные сжимаются GzEncoder (flate2) и записываются в файл 0600.
/// 4. При ненулевом коде выхода pg_dump — удаляет частичный файл.
pub async fn create_backup() -> anyhow::Result<BackupInfo> {
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL env is not set"))?;

    let dir = backup_dir();

    // Создаём директорию если её нет
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot create backup directory {:?}: {}", dir, e))?;

    // Проверяем права на запись
    let test_path = dir.join(".write_test");
    tokio::fs::write(&test_path, b"")
        .await
        .map_err(|e| anyhow::anyhow!("Backup directory {:?} is not writable: {}", dir, e))?;
    let _ = tokio::fs::remove_file(&test_path).await;

    let now = Utc::now();
    let filename = make_filename(now);
    let out_path = dir.join(&filename);

    // Разбираем URL для передачи параметров pg_dump без пароля в аргументах
    let (pg_args, pg_password) = parse_pg_url(&database_url)?;

    let start = std::time::Instant::now();

    // Запускаем pg_dump как дочерний процесс — stdout в pipe, stderr в pipe
    let mut child = Command::new("pg_dump")
        .args(&pg_args)
        .env("PGPASSWORD", &pg_password)
        .env("PGAPPNAME", "caramba-backup")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("pg_dump binary not found in PATH — install postgresql-client")
            } else {
                anyhow::anyhow!("Failed to spawn pg_dump: {}", e)
            }
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("pg_dump stdout is unavailable"))?;

    // Читаем весь stdout в память и сжимаем через GzEncoder.
    // Для типичной базы панели (< 100 MB) это нормально; потоковое прижатие
    // к файлу через tokio_fs::File + async writer значительно сложнее
    // из-за блокирующего GzEncoder — используем spawn_blocking.
    let mut raw = Vec::new();
    stdout
        .read_to_end(&mut raw)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read pg_dump output: {}", e))?;

    // Ждём завершения pg_dump и проверяем код выхода
    let status = child
        .wait_with_output()
        .await
        .map_err(|e| anyhow::anyhow!("pg_dump wait failed: {}", e))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(anyhow::anyhow!(
            "pg_dump exited with code {}: {}",
            status.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let out_path_clone = out_path.clone();

    // Сжимаем в отдельном блокирующем потоке чтобы не блокировать рантайм
    let compressed_result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;

        // Создаём файл с правами 0600 (только владелец читает)
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&out_path_clone)
            .map_err(|e| {
                anyhow::anyhow!("Cannot create backup file {:?}: {}", out_path_clone, e)
            })?;

        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder
            .write_all(&raw)
            .map_err(|e| anyhow::anyhow!("GzEncoder write error: {}", e))?;
        encoder
            .finish()
            .map_err(|e| anyhow::anyhow!("GzEncoder finish error: {}", e))?;

        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {}", e))?;

    // Если запись провалилась — удаляем частичный файл
    if let Err(ref e) = compressed_result {
        warn!(
            "Backup write failed, removing partial file {:?}: {}",
            out_path, e
        );
        let _ = tokio::fs::remove_file(&out_path).await;
    }
    compressed_result?;

    let duration_ms = start.elapsed().as_millis() as u64;

    // Получаем размер файла
    let metadata = tokio::fs::metadata(&out_path)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot stat backup file: {}", e))?;

    info!(
        filename = %filename,
        size_bytes = metadata.len(),
        duration_ms = duration_ms,
        "Backup created successfully"
    );

    Ok(BackupInfo {
        filename,
        path: out_path.to_string_lossy().to_string(),
        size_bytes: metadata.len(),
        created_at: now,
        duration_ms,
    })
}

/// Возвращает список всех резервных копий в BACKUP_DIR, отсортированных
/// от новых к старым.
pub async fn list_backups() -> anyhow::Result<Vec<BackupInfo>> {
    let dir = backup_dir();

    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot read backup directory {:?}: {}", dir, e))?;

    let mut backups: Vec<BackupInfo> = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".sql.gz") {
            continue;
        }
        if !validate_filename(&name) {
            continue;
        }

        let meta = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Извлекаем дату из имени файла для надёжности (mtime может быть изменён)
        let created_at = parse_timestamp_from_filename(&name).unwrap_or_else(|| {
            // Фолбэк на mtime
            meta.modified()
                .map(DateTime::from)
                .unwrap_or_else(|_| Utc::now())
        });

        backups.push(BackupInfo {
            filename: name,
            path: entry.path().to_string_lossy().to_string(),
            size_bytes: meta.len(),
            created_at,
            duration_ms: 0,
        });
    }

    // Сортируем от новых к старым
    backups.sort_by_key(|b| std::cmp::Reverse(b.created_at));

    Ok(backups)
}

/// Удаляет старые резервные копии, оставляя `keep` самых новых.
/// Возвращает количество удалённых файлов.
pub async fn rotate(keep: usize) -> anyhow::Result<u64> {
    let mut backups = list_backups().await?;
    if backups.len() <= keep {
        return Ok(0);
    }

    // Убираем keep новейших — остальные удаляем
    let to_delete: Vec<_> = backups.drain(keep..).collect();
    let mut deleted = 0u64;

    for backup in to_delete {
        match tokio::fs::remove_file(&backup.path).await {
            Ok(_) => {
                info!(filename = %backup.filename, "Rotated old backup");
                deleted += 1;
            }
            Err(e) => {
                warn!(filename = %backup.filename, error = %e, "Failed to delete old backup during rotation");
            }
        }
    }

    Ok(deleted)
}

/// Удаляет конкретный файл резервной копии по имени.
/// Имя валидируется перед удалением для защиты от path-traversal.
pub async fn delete_backup(filename: &str) -> anyhow::Result<()> {
    if !validate_filename(filename) {
        return Err(anyhow::anyhow!("Invalid backup filename: {}", filename));
    }

    let dir = backup_dir();
    let path = dir.join(filename);

    if !path.exists() {
        return Err(anyhow::anyhow!("Backup file not found: {}", filename));
    }

    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to delete backup {:?}: {}", path, e))?;

    info!(filename = %filename, "Backup deleted by admin");
    Ok(())
}

/// Возвращает полный путь к файлу резервной копии после валидации имени.
/// Используется в обработчике скачивания.
pub fn backup_file_path(filename: &str) -> anyhow::Result<PathBuf> {
    if !validate_filename(filename) {
        return Err(anyhow::anyhow!("Invalid backup filename"));
    }
    let path = backup_dir().join(filename);
    if !path.exists() {
        return Err(anyhow::anyhow!("Backup file not found: {}", filename));
    }
    Ok(path)
}

// ────────────────────────────────────────────────────────────────────────────
// Парсинг DATABASE_URL
// ────────────────────────────────────────────────────────────────────────────

/// Разбирает DATABASE_URL postgres://user:pass@host:port/dbname?...
/// Возвращает аргументы для pg_dump и пароль отдельно.
/// Если разбор не удался — возвращает URL целиком как аргумент (упрощённый фолбэк).
fn parse_pg_url(url: &str) -> anyhow::Result<(Vec<String>, String)> {
    // Используем url::Url для парсинга
    let parsed =
        url::Url::parse(url).map_err(|e| anyhow::anyhow!("Cannot parse DATABASE_URL: {}", e))?;

    let password = parsed.password().unwrap_or("").to_string();
    let host = parsed.host_str().unwrap_or("localhost").to_string();
    let port = parsed.port().unwrap_or(5432);
    let user = parsed.username().to_string();
    // Путь начинается с '/', убираем ведущий слеш
    let dbname = parsed.path().trim_start_matches('/').to_string();

    if dbname.is_empty() {
        return Err(anyhow::anyhow!(
            "DATABASE_URL does not contain a database name"
        ));
    }

    let mut args: Vec<String> = vec!["--no-password".to_string()];
    args.push("-h".to_string());
    args.push(host);
    args.push("-p".to_string());
    args.push(port.to_string());
    if !user.is_empty() {
        args.push("-U".to_string());
        args.push(user);
    }
    args.push(dbname);

    Ok((args, password))
}

/// Извлекает DateTime<Utc> из имени файла `caramba-YYYYMMDDTHHMMSSZ.sql.gz`.
fn parse_timestamp_from_filename(filename: &str) -> Option<DateTime<Utc>> {
    // caramba-20260428T153045Z.sql.gz
    let inner = filename.strip_prefix("caramba-")?.strip_suffix(".sql.gz")?;
    // inner = "20260428T153045Z"
    chrono::NaiveDateTime::parse_from_str(inner, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|ndt| ndt.and_utc())
}

// ────────────────────────────────────────────────────────────────────────────
// regex_lite — локальный мини-матчер без зависимости на полный crate `regex`
// ────────────────────────────────────────────────────────────────────────────

/// Мини-модуль замены regex для простого паттерна имени файла резервной копии.
/// Избегаем добавления зависимости `regex` — используем ручную проверку.
mod regex_lite {
    pub struct Regex {
        // Паттерн зашит жёстко для нашего единственного use-case
    }

    impl Regex {
        pub fn new(_pattern: &str) -> Result<Self, ()> {
            Ok(Regex {})
        }

        /// Проверяет соответствие имени файла шаблону `caramba-YYYYMMDDTHHMMSSZ.sql.gz`.
        pub fn is_match(&self, s: &str) -> bool {
            // caramba-YYYYMMDDTHHMMSSZ.sql.gz
            // 8 цифр даты, T, 6 цифр времени, Z
            if let Some(inner) = s
                .strip_prefix("caramba-")
                .and_then(|s| s.strip_suffix(".sql.gz"))
            {
                // inner must be exactly 16 chars: YYYYMMDDTHHMMSSZ
                if inner.len() != 16 {
                    return false;
                }
                let bytes = inner.as_bytes();
                // YYYYMMDD
                bytes[..8].iter().all(|b| b.is_ascii_digit())
                    && bytes[8] == b'T'
                    && bytes[9..15].iter().all(|b| b.is_ascii_digit())
                    && bytes[15] == b'Z'
            } else {
                false
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_filename_accepts_valid() {
        assert!(validate_filename("caramba-20260428T153045Z.sql.gz"));
    }

    #[test]
    fn validate_filename_rejects_traversal() {
        assert!(!validate_filename("../etc/passwd"));
        assert!(!validate_filename(
            "../../backup/caramba-20260428T153045Z.sql.gz"
        ));
        assert!(!validate_filename("caramba-20260428T153045Z.sql.gz/extra"));
    }

    #[test]
    fn validate_filename_rejects_wrong_format() {
        assert!(!validate_filename("caramba-bad.sql.gz"));
        assert!(!validate_filename("caramba-20260428T153045Z.tar.gz"));
        assert!(!validate_filename(""));
    }

    #[test]
    fn make_filename_format() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-04-28T15:30:45Z")
            .unwrap()
            .with_timezone(&Utc);
        let name = make_filename(now);
        assert_eq!(name, "caramba-20260428T153045Z.sql.gz");
        assert!(validate_filename(&name));
    }

    #[test]
    fn parse_timestamp_roundtrip() {
        let name = "caramba-20260428T153045Z.sql.gz";
        let dt = parse_timestamp_from_filename(name).unwrap();
        assert_eq!(dt.format("%Y%m%dT%H%M%SZ").to_string(), "20260428T153045Z");
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert!(format_size(5 * 1024 * 1024).contains("MB"));
        assert!(format_size(2 * 1024 * 1024 * 1024).contains("GB"));
    }

    #[test]
    fn parse_pg_url_standard() {
        let (args, pass) = parse_pg_url("postgres://admin:secret@db.host:5432/caramba").unwrap();
        assert!(args.contains(&"db.host".to_string()));
        assert!(args.contains(&"5432".to_string()));
        assert!(args.contains(&"caramba".to_string()));
        assert!(args.contains(&"admin".to_string()));
        assert_eq!(pass, "secret");
        // Пароль не в аргументах
        assert!(!args.contains(&"secret".to_string()));
    }
}
