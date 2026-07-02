use anyhow::{anyhow, Result};
use console::style;
use dialoguer::Confirm;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use tar::Archive;

/// Пытается остановить systemd-сервис. Не возвращает ошибку, если сервис не существует.
fn try_stop_service(service: &str) {
    let status = Command::new("systemctl").args(["stop", service]).status();
    match status {
        Ok(s) if s.success() => println!("  Stopped {}", service),
        Ok(_) => println!("  {} was not running (OK)", service),
        Err(e) => println!("  Warning: could not stop {}: {}", service, e),
    }
}

/// Пытается запустить systemd-сервис. Ошибка логируется, не пробрасывается.
fn try_start_service(service: &str) {
    let status = Command::new("systemctl").args(["start", service]).status();
    match status {
        Ok(s) if s.success() => println!("  Started {}", service),
        Ok(s) => println!("  Warning: {} start exited with {}", service, s),
        Err(e) => println!("  Warning: could not start {}: {}", service, e),
    }
}

pub fn run_restore(backup_path: &str) -> Result<()> {
    println!("{}", style("\n=== CARAMBA RESTORE TOOL ===").bold().green());

    let path = Path::new(backup_path);
    if !path.exists() {
        return Err(anyhow!("Backup file not found: {}", backup_path));
    }

    println!("Backup file: {}", backup_path);

    // 1. Extract
    println!("Extracting backup...");
    let file = File::open(path)?;
    let tar = GzDecoder::new(file);
    let mut archive = Archive::new(tar);

    let temp_dir = tempfile::tempdir()?;
    archive.unpack(temp_dir.path())?;

    // Find expected directory (starts with caramba_export_)
    let mut extract_dir = None;
    for entry in std::fs::read_dir(temp_dir.path())? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("caramba_export_") {
                    extract_dir = Some(path);
                    break;
                }
            }
        }
    }

    let extract_dir = extract_dir.ok_or_else(|| anyhow!("Invalid backup archive structure"))?;
    println!("Archive extracted to temporary location");

    // 2. Show Env Info
    let env_file = extract_dir.join("env_sanitized.txt");
    if env_file.exists() {
        println!(
            "\n{}",
            style("Environment Configuration (Sanitized):").bold()
        );
        let mut content = String::new();
        File::open(env_file)?.read_to_string(&mut content)?;
        println!("{}", content);
        println!(
            "{}",
            style("IMPORTANT: Merge these values into your .env file.").yellow()
        );
    }

    // 3. Database Restore
    let sql_file = extract_dir.join("backup.sql");
    if sql_file.exists() {
        println!("\nFound database dump: {:?}", sql_file.file_name().unwrap());

        // Sanity-check the dump format BEFORE we offer to restore it. A bad upload
        // (truncated, wrong file, gzipped twice, etc.) silently piped into psql can
        // partially corrupt the live DB. Cheap header sniff catches obvious garbage.
        if let Err(e) = validate_pg_dump(&sql_file) {
            return Err(anyhow!(
                "Refusing to restore: backup.sql does not look like a PostgreSQL dump ({}). \
                 If you generated this with `pg_dump --format=plain` and the file is intact, \
                 please report this as a bug.",
                e
            ));
        }

        if Confirm::new()
            .with_prompt("Do you want to try and restore this to PostgreSQL? (Requires psql)")
            .interact()?
        {
            let db_url = dialoguer::Input::<String>::new()
                .with_prompt("Enter DATABASE_URL (postgres://user:pass@localhost/db)")
                .interact_text()?;

            // Останавливаем сервисы чтобы избежать конфликтов во время импорта.
            // Это предотвращает повреждение данных при одновременной записи.
            println!(
                "\n{}",
                style("Stopping Caramba services before database restore...").yellow()
            );
            let managed_services = [
                "caramba-panel.service",
                "caramba-sub.service",
                "caramba-bot.service",
            ];
            for svc in &managed_services {
                try_stop_service(svc);
            }

            // Take a pre-restore snapshot so a botched import is recoverable.
            // Best-effort: if pg_dump is missing or the DB is empty/unreachable,
            // we surface a warning and continue — operator has been informed.
            let snapshot_path = std::env::temp_dir().join(format!(
                "caramba-pre-restore-{}.sql",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            ));
            println!(
                "\n{}",
                style(format!(
                    "Taking pre-restore snapshot to {}...",
                    snapshot_path.display()
                ))
                .yellow()
            );
            let snapshot_file = std::fs::File::create(&snapshot_path)?;
            let snap_status = Command::new("pg_dump")
                .arg(&db_url)
                .stdout(snapshot_file)
                .status();
            match snap_status {
                Ok(s) if s.success() => println!(
                    "  {}",
                    style(format!(
                        "Snapshot saved. To revert, run: psql \"{}\" < {}",
                        db_url,
                        snapshot_path.display()
                    ))
                    .green()
                ),
                Ok(s) => println!(
                    "  {}",
                    style(format!(
                        "Warning: pg_dump exited with {}, proceeding without recoverable snapshot",
                        s
                    ))
                    .red()
                ),
                Err(e) => println!(
                    "  {}",
                    style(format!(
                        "Warning: could not run pg_dump ({}), proceeding without recoverable snapshot",
                        e
                    ))
                    .red()
                ),
            }

            println!("Restoring database...");
            // psql $DATABASE_URL < backup.sql — аргументы передаются напрямую, без shell-интерполяции
            let sql_content = std::fs::File::open(&sql_file)?;
            let status = Command::new("psql")
                .arg(&db_url)
                .stdin(sql_content)
                .status()?;

            // Перезапускаем сервисы в любом случае (успех или ошибка).
            println!("\nRestarting Caramba services...");
            for svc in &managed_services {
                try_start_service(svc);
            }

            if status.success() {
                println!("{}", style("Database restored successfully.").green());
            } else {
                // Возвращаем ошибку — caller (main) должен сообщить о сбое.
                return Err(anyhow!(
                    "psql exited with status {}. Database restore may be incomplete. \
                     Check the PostgreSQL logs for details.",
                    status
                ));
            }
        }
    } else {
        println!("No 'backup.sql' found in archive.");
    }

    println!("\n{}", style("Restore process completed.").green());
    Ok(())
}

/// Sniffs the first few KB of `path` to verify it looks like a `pg_dump --format=plain`
/// output. Rejects empty files, custom-format dumps (which need pg_restore, not psql),
/// and obvious garbage.
fn validate_pg_dump(path: &Path) -> Result<()> {
    let mut file = File::open(path)?;
    let mut head = [0u8; 4096];
    let n = file.read(&mut head)?;
    if n == 0 {
        return Err(anyhow!("file is empty"));
    }

    // Custom-format pg_dump starts with "PGDMP". psql can't ingest it.
    if head.starts_with(b"PGDMP") {
        return Err(anyhow!(
            "file is a pg_dump custom-format archive, not plain SQL. Use `pg_restore` instead"
        ));
    }

    let head_str = std::str::from_utf8(&head[..n])
        .map_err(|_| anyhow!("file is not valid UTF-8 (binary or wrong format)"))?;

    let looks_like_dump = head_str.contains("PostgreSQL database dump")
        || (head_str.trim_start().starts_with("--")
            && (head_str.contains("CREATE ")
                || head_str.contains("COPY ")
                || head_str.contains("INSERT ")
                || head_str.contains("SET ")));

    if !looks_like_dump {
        return Err(anyhow!("no recognisable pg_dump header in first 4KB"));
    }
    Ok(())
}
