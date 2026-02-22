use anyhow::{bail, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Input, Password};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug)]
pub struct InstallConfig {
    pub domain: String,
    pub sub_domain: Option<String>, // For Hub Mode
    pub admin_path: String,
    pub install_dir: String,
    pub db_pass: String,
    pub admin_username: String,
    pub admin_password: String,
    pub hub_bot_token: Option<String>,
}

#[derive(Debug, Default)]
struct ExistingInstallDefaults {
    install_dir: String,
    existing_install: bool,
    domain: Option<String>,
    sub_domain: Option<String>,
    admin_path: Option<String>,
    db_pass: Option<String>,
    admin_username: Option<String>,
    admin_password: Option<String>,
    hub_bot_token: Option<String>,
}

fn normalize_admin_path(path: String) -> String {
    if path.starts_with('/') {
        path
    } else {
        format!("/{}", path)
    }
}

fn parse_key_value_file(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return map;
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        map.insert(key.trim().to_string(), value.trim().to_string());
    }

    map
}

fn normalize_domain_like(value: &str) -> Option<String> {
    let mut raw = value.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    raw = raw.trim_start_matches("https://").to_string();
    raw = raw.trim_start_matches("http://").to_string();
    if let Some((head, _)) = raw.split_once('/') {
        raw = head.to_string();
    }
    raw = raw.trim_end_matches('/').to_string();
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

fn parse_db_password_from_url(database_url: &str) -> Option<String> {
    let after_scheme = database_url.trim().split_once("://")?.1;
    let auth = after_scheme.split_once('@')?.0;
    let encoded_pass = auth.split_once(':')?.1;
    let decoded = urlencoding::decode(encoded_pass).ok()?.to_string();
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn parse_install_summary(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return (None, None);
    };

    let mut admin_user = None;
    let mut admin_pass = None;
    for line in content.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("admin username:") {
            admin_user = line
                .split_once(':')
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty());
        } else if lower.starts_with("admin password:") {
            admin_pass = line
                .split_once(':')
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty());
        }
    }

    (admin_user, admin_pass)
}

fn load_existing_install_defaults(install_dir_hint: Option<&str>) -> ExistingInstallDefaults {
    let install_dir = install_dir_hint
        .unwrap_or("/opt/caramba")
        .trim()
        .trim_end_matches('/')
        .to_string();
    let env_path = Path::new(&install_dir).join(".env");
    if !env_path.exists() {
        return ExistingInstallDefaults {
            install_dir,
            existing_install: false,
            ..ExistingInstallDefaults::default()
        };
    }

    let env = parse_key_value_file(&env_path);
    let domain = env
        .get("SERVER_DOMAIN")
        .and_then(|v| normalize_domain_like(v))
        .or_else(|| env.get("PANEL_URL").and_then(|v| normalize_domain_like(v)));
    let admin_path = env.get("ADMIN_PATH").cloned().filter(|v| !v.is_empty());
    let db_pass = env
        .get("DATABASE_URL")
        .and_then(|v| parse_db_password_from_url(v));

    let sub_env = parse_key_value_file(&Path::new(&install_dir).join("sub.env"));
    let sub_domain = sub_env
        .get("FRONTEND_DOMAIN")
        .and_then(|v| normalize_domain_like(v));

    let bot_env = parse_key_value_file(&Path::new(&install_dir).join("bot.env"));
    let hub_bot_token = bot_env
        .get("BOT_TOKEN")
        .cloned()
        .filter(|v| !v.trim().is_empty());

    let (admin_username, admin_password) =
        parse_install_summary(&Path::new(&install_dir).join("INSTALL_SUMMARY.txt"));

    ExistingInstallDefaults {
        install_dir,
        existing_install: true,
        domain,
        sub_domain,
        admin_path,
        db_pass,
        admin_username,
        admin_password,
        hub_bot_token,
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

fn get_or_prompt_admin_password(value: Option<String>) -> String {
    if let Some(v) = value {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let theme = ColorfulTheme::default();
    Password::with_theme(&theme)
        .with_prompt("Admin Password")
        .with_confirmation("Confirm Admin Password", "Passwords mismatch")
        .interact()
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn get_or_prompt_optional_password(value: Option<String>, prompt: &str) -> Option<String> {
    if let Some(v) = value {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
        return None;
    }

    let theme = ColorfulTheme::default();
    let raw = Password::with_theme(&theme)
        .with_prompt(prompt)
        .allow_empty_password(true)
        .interact()
        .unwrap_or_default();
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn resolve_install_config(
    hub_mode: bool,
    domain: Option<String>,
    sub_domain: Option<String>,
    admin_path: Option<String>,
    install_dir: Option<String>,
    db_pass: Option<String>,
    admin_username: Option<String>,
    admin_password: Option<String>,
    hub_bot_token: Option<String>,
) -> Result<InstallConfig> {
    let existing = load_existing_install_defaults(install_dir.as_deref());
    if existing.existing_install {
        println!(
            "{}",
            style(format!(
                "\nDetected existing installation in {}. Reusing current configuration defaults.",
                existing.install_dir
            ))
            .cyan()
        );
    } else {
        println!("{}", style("\nConfiguring Caramba...").bold());
    }

    let domain_default = existing.domain.clone();
    let domain = get_or_prompt_text(
        domain.or(domain_default.clone()),
        "Panel Domain (e.g. panel.example.com)",
        domain_default.as_deref(),
    );
    if domain.is_empty() {
        bail!("Panel domain must not be empty");
    }

    let sub_domain = if hub_mode {
        let raw = if existing.existing_install && sub_domain.is_none() {
            existing.sub_domain.clone().unwrap_or_default()
        } else {
            get_or_prompt_text(
                sub_domain.or(existing.sub_domain.clone()),
                "Subscription Domain (e.g. sub.example.com)",
                Some(""),
            )
        };
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    } else {
        None
    };

    let admin_path_default = existing.admin_path.clone();
    let admin_path = normalize_admin_path(get_or_prompt_text(
        admin_path.or(admin_path_default.clone()),
        "Admin Panel Path",
        admin_path_default.as_deref().or(Some("/admin")),
    ));

    let default_install_dir = existing.install_dir.clone();
    let install_dir = get_or_prompt_text(
        install_dir.or(Some(default_install_dir.clone())),
        "Installation Directory",
        Some(default_install_dir.as_str()),
    );
    if install_dir.is_empty() {
        bail!("Installation directory must not be empty");
    }

    let db_pass = get_or_prompt_password(db_pass.or(existing.db_pass.clone()));
    if db_pass.is_empty() {
        bail!("Database password must not be empty");
    }

    let admin_user_default = existing.admin_username.clone();
    let admin_username = get_or_prompt_text(
        admin_username.or(admin_user_default.clone()),
        "Admin Username",
        admin_user_default.as_deref().or(Some("admin")),
    );
    if admin_username.is_empty() {
        bail!("Admin username must not be empty");
    }

    let admin_password = get_or_prompt_admin_password(admin_password.or(existing.admin_password));
    if admin_password.is_empty() {
        bail!("Admin password must not be empty");
    }

    let hub_bot_token = if hub_mode {
        if existing.existing_install && hub_bot_token.is_none() {
            existing.hub_bot_token
        } else {
            get_or_prompt_optional_password(
                hub_bot_token.or(existing.hub_bot_token),
                "Telegram BOT_TOKEN (optional, leave blank to skip)",
            )
        }
    } else {
        None
    };

    Ok(InstallConfig {
        domain,
        sub_domain,
        admin_path,
        install_dir,
        db_pass,
        admin_username,
        admin_password,
        hub_bot_token,
    })
}

pub fn generate_caddyfile(config: &InstallConfig) -> String {
    let admin_path = normalize_admin_path(config.admin_path.clone());
    let same_domain_sub = config.sub_domain.as_ref() == Some(&config.domain);
    let has_external_sub_domain = config.sub_domain.as_ref().is_some() && !same_domain_sub;

    let mut main_path_rules = vec![
        "/api".to_string(),
        "/api/*".to_string(),
        "/caramba-api".to_string(),
        "/caramba-api/*".to_string(),
        "/assets/*".to_string(),
        "/downloads/*".to_string(),
        "/install.sh".to_string(),
        "/nodes/*".to_string(),
        admin_path.clone(),
        format!("{}/*", admin_path),
    ];

    if !same_domain_sub {
        // Panel-only mode (or dedicated sub domain): panel serves /app and /sub URLs.
        main_path_rules.push("/app".to_string());
        main_path_rules.push("/app/*".to_string());
        main_path_rules.push("/sub/*".to_string());
    }

    let main_paths = main_path_rules.join(" ");
    let mut caddyfile = format!(
        "{domain} {{
    encode zstd gzip

{same_domain_frontend}
    @panel_routes path {main_paths}
    handle @panel_routes {{
        reverse_proxy 127.0.0.1:3000
    }}

    handle {{
        respond \"Not found\" 404
    }}
}}
",
        domain = config.domain,
        same_domain_frontend = if same_domain_sub {
            "    @same_domain_frontend path /app /app/* /sub/* /health\n    handle @same_domain_frontend {\n        reverse_proxy 127.0.0.1:8080\n    }\n\n"
        } else {
            ""
        },
        main_paths = main_paths
    );

    if has_external_sub_domain {
        if let Some(sub) = &config.sub_domain {
            caddyfile.push_str(&format!(
                "\n{sub} {{\n    encode zstd gzip\n\n    handle {{\n        reverse_proxy 127.0.0.1:8080\n    }}\n}}\n"
            ));
        }
    }

    caddyfile
}
