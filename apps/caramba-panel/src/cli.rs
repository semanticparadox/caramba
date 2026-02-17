use sqlx::PgPool;
use anyhow::{Result, Context};
use std::fs;
use std::env;

pub async fn reset_password(pool: &PgPool, username: &str, new_pass: &str) -> Result<()> {
    let hash = bcrypt::hash(new_pass, bcrypt::DEFAULT_COST)
        .context("Failed to hash password")?;

    // Try update first
    let result = sqlx::query("UPDATE admins SET password_hash = $1 WHERE username = $2")
        .bind(&hash)
        .bind(username)
        .execute(pool)
        .await
        .context("Failed to update password in database")?;

    if result.rows_affected() == 0 {
        // If no rows affected, insert new admin
        sqlx::query("INSERT INTO admins (username, password_hash) VALUES ($1, $2)")
            .bind(username)
            .bind(&hash)
            .execute(pool)
            .await
            .context("Failed to create new admin")?;
        println!("New admin user '{}' created successfully.", username);
    } else {
        println!("Password for user '{}' has been successfully reset.", username);
    }
    
    Ok(())
}

pub fn install_service() -> Result<()> {
    let exe_path = env::current_exe()?;
    let exe_name = exe_path.file_name().unwrap().to_str().unwrap();
    let working_dir = env::current_dir()?;

    let service_content = format!(
        r#"[Unit]
Description=Caramba VPN Control Plane
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory={}
ExecStart={} serve
Restart=always
EnvironmentFile={}/.env

[Install]
WantedBy=multi-user.target
"#,
        working_dir.display(),
        exe_path.display(),
        working_dir.display()
    );

    let service_path = format!("/etc/systemd/system/{}.service", exe_name);
    
    // Check if running as root
    if unsafe { libc::getuid() } != 0 {
        return Err(anyhow::anyhow!("This command must be run as root (sudo) to install systemd service."));
    }

    fs::write(&service_path, service_content)
        .context(format!("Failed to write service file to {}", service_path))?;

    println!("Systemd service created at {}", service_path);
    println!("You can now start the service using:");
    println!("  systemctl daemon-reload");
    println!("  systemctl enable --now {}", exe_name);

    Ok(())
}
