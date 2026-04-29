use anyhow::{anyhow, Result};
use console::style;
use dialoguer::Confirm;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use tar::Archive;

pub fn run_restore(backup_path: &str) -> Result<()> {
    println!("{}", style("\n=== CARAMBA RESTORE TOOL ===").bold().green());

    let path = Path::new(backup_path);
    if !path.exists() {
        return Err(anyhow!("Backup file not found: {}", backup_path));
    }

    println!("📦 Backup file: {}", backup_path);

    // 1. Extract
    println!("🔄 Extracting backup...");
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
    println!("✅ Archive extracted to temporary location");

    // 2. Show Env Info
    let env_file = extract_dir.join("env_sanitized.txt");
    if env_file.exists() {
        println!(
            "\n{}",
            style("⚙️  Environment Configuration (Sanitized):").bold()
        );
        let mut content = String::new();
        File::open(env_file)?.read_to_string(&mut content)?;
        println!("{}", content);
        println!(
            "{}",
            style("⚠️  IMPORTANT: Merge these values into your .env file.").yellow()
        );
    }

    // 3. Database Restore
    let sql_file = extract_dir.join("backup.sql"); // Assuming backup.sql is the name
    if sql_file.exists() {
        println!("\nFound database dump: {:?}", sql_file.file_name().unwrap());
        if Confirm::new()
            .with_prompt("Do you want to try and restore this to PostgreSQL? (Requires psql)")
            .interact()?
        {
            // Ask for DB URL or use default?
            let db_url = dialoguer::Input::<String>::new()
                .with_prompt("Enter DATABASE_URL (postgres://user:pass@localhost/db)")
                .interact_text()?;

            println!("Restoring database...");
            // psql $DATABASE_URL < backup.sql — передаём аргументы напрямую, без shell-интерполяции
            let sql_content = std::fs::File::open(&sql_file)?;
            let status = Command::new("psql")
                .arg(&db_url)
                .stdin(sql_content)
                .status()?;

            if status.success() {
                println!("{}", style("Database restored successfully.").green());
            } else {
                println!("{}", style("Database restore failed.").red());
            }
        }
    } else {
        println!("No 'backup.sql' found in archive.");
    }

    println!("\n{}", style("Restore process completed.").green());
    Ok(())
}
