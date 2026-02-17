use anyhow::Result;
use console::style;
use std::process::Command;
use std::path::Path;

pub fn run_diagnostics() -> Result<()> {
    println!("{}", style("\n=== CARAMBA DEEP DIAGNOSTIC TOOL ===").bold().yellow());
    println!("Date: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    
    // 1. System Checks
    println!("\n{}", style("--- 1. System Checks ---").bold());
    check_command("sing-box");
    check_command("ufw");
    check_command("openssl");
    check_command("curl");

    // 2. Service Status
    println!("\n{}", style("--- 2. Service Status ---").bold());
    check_service("sing-box");
    check_service("caramba-node");
    check_service("caramba-panel");
    check_service("caramba-bot");

    // 3. Network Ports
    println!("\n{}", style("--- 3. Network Ports (Listening) ---").bold());
    if check_command_silent("ss") {
        run_cmd("ss", &["-tulpn"], "sing-box");
    } else {
        run_cmd("netstat", &["-tulpn"], "sing-box");
    }

    // 4. Firewall
    println!("\n{}", style("--- 4. Firewall (UFW) ---").bold());
    if check_command_silent("ufw") {
        run_cmd("ufw", &["status"], "443");
    } else {
        println!("UFW not found or active.");
    }

    // 5. Certificates
    println!("\n{}", style("--- 5. Certificate Verification ---").bold());
    let cert_path = "/etc/sing-box/certs/cert.pem";
    if Path::new(cert_path).exists() {
        println!("Certificate found at {}", cert_path);
        run_cmd("openssl", &["x509", "-in", cert_path, "-noout", "-subject", "-dates", "-fingerprint"], "");
    } else {
        println!("{}", style(format!("Certificate missing at {}", cert_path)).red());
    }

    // 6. Config Validation
    println!("\n{}", style("--- 6. Config Validation ---").bold());
    let config_path = "/etc/sing-box/config.json";
    if Path::new(config_path).exists() {
        println!("Checking config syntax...");
        let status = Command::new("sing-box").args(&["check", "-c", config_path]).status();
        match status {
            Ok(s) if s.success() => println!("{}", style("Config Valid").green()),
            _ => println!("{}", style("Config Invalid").red()),
        }
    } else {
        println!("{}", style(format!("Config missing at {}", config_path)).red());
    }

    // 7. Recent Log Errors
    println!("\n{}", style("--- 7. Recent Logs (Errors) ---").bold());
    println!("Checking caramba-panel logs for errors...");
    run_cmd("bash", &["-c", "journalctl -u caramba-panel -n 50 --no-pager | grep -i error"], "");
    
    println!("Checking caramba-node logs for errors...");
    run_cmd("bash", &["-c", "journalctl -u caramba-node -n 50 --no-pager | grep -i error"], "");

    println!("\n{}", style("=== END OF DIAGNOSTICS ===").bold().yellow());
    Ok(())
}

fn check_command(cmd: &str) {
    if check_command_silent(cmd) {
         println!("[{}] Command '{}' found", style("OK").green(), cmd);
    } else {
         println!("[{}] Command '{}' NOT found", style("FAIL").red(), cmd);
    }
}

fn check_command_silent(cmd: &str) -> bool {
    Command::new("which").arg(cmd).output().map(|o| o.status.success()).unwrap_or(false)
}

fn check_service(service: &str) {
    let output = Command::new("systemctl").args(&["is-active", service]).output();
    match output {
        Ok(o) => {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if status == "active" {
                println!("[{}] Service '{}' is active", style("OK").green(), service);
            } else {
                println!("[{}] Service '{}' is {}", style("WARN").yellow(), service, status);
            }
        },
        Err(_) => println!("[{}] Service '{}' check failed", style("FAIL").red(), service),
    }
}

fn run_cmd(cmd: &str, args: &[&str], grep: &str) {
    let output = Command::new(cmd).args(args).output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if grep.is_empty() {
                println!("{}", stdout);
            } else {
                for line in stdout.lines() {
                    if line.contains(grep) {
                        println!("{}", line);
                    }
                }
            }
        },
        Err(e) => println!("Error running {}: {}", cmd, e),
    }
}
