use anyhow::{bail, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Input, Password};

#[derive(Debug)]
pub struct InstallConfig {
    pub domain: String,
    pub sub_domain: Option<String>, // For Hub Mode
    pub admin_path: String,
    pub install_dir: String,
    pub db_pass: String,
}

fn normalize_admin_path(path: String) -> String {
    if path.starts_with('/') {
        path
    } else {
        format!("/{}", path)
    }
}

fn get_or_prompt_text(value: Option<String>, prompt: &str, default: Option<&str>) -> String {
    if let Some(v) = value {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let theme = ColorfulTheme::default();
    let mut input = Input::with_theme(&theme).with_prompt(prompt);
    if let Some(d) = default {
        input = input.default(d.to_string());
    }
    input.interact_text().unwrap_or_default().trim().to_string()
}

fn get_or_prompt_password(value: Option<String>) -> String {
    if let Some(v) = value {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let theme = ColorfulTheme::default();
    Password::with_theme(&theme)
        .with_prompt("PostgreSQL Database Password")
        .with_confirmation("Confirm Password", "Passwords mismatch")
        .interact()
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn resolve_install_config(
    hub_mode: bool,
    domain: Option<String>,
    sub_domain: Option<String>,
    admin_path: Option<String>,
    install_dir: Option<String>,
    db_pass: Option<String>,
) -> Result<InstallConfig> {
    println!("{}", style("\nConfiguring Caramba...").bold());

    let domain = get_or_prompt_text(domain, "Panel Domain (e.g. panel.example.com)", None);
    if domain.is_empty() {
        bail!("Panel domain must not be empty");
    }

    let sub_domain = if hub_mode {
        let raw = get_or_prompt_text(
            sub_domain,
            "Subscription Domain (e.g. sub.example.com)",
            Some(""),
        );
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    } else {
        None
    };

    let admin_path = normalize_admin_path(get_or_prompt_text(
        admin_path,
        "Admin Panel Path",
        Some("/admin"),
    ));
    let install_dir =
        get_or_prompt_text(install_dir, "Installation Directory", Some("/opt/caramba"));
    if install_dir.is_empty() {
        bail!("Installation directory must not be empty");
    }

    let db_pass = get_or_prompt_password(db_pass);
    if db_pass.is_empty() {
        bail!("Database password must not be empty");
    }

    Ok(InstallConfig {
        domain,
        sub_domain,
        admin_path,
        install_dir,
        db_pass,
    })
}

pub fn generate_caddyfile(config: &InstallConfig) -> String {
    let mut caddyfile = format!(
        "{domain} {{\n    reverse_proxy 127.0.0.1:3000\n}}\n",
        domain = config.domain
    );

    if let Some(sub) = &config.sub_domain {
        if sub == &config.domain {
            return caddyfile;
        }
        caddyfile.push_str(&format!(
            "\n{sub} {{\n    reverse_proxy 127.0.0.1:8080\n}}\n"
        ));
    }

    caddyfile
}
