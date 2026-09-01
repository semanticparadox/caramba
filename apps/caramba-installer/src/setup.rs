use anyhow::{bail, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Input, Password};
use std::collections::HashMap;
use std::path::Path;

/// Default license server URL. The platform owner hosts this; operators can
/// override it per install with --license-server-url or the .env value.
pub const DEFAULT_LICENSE_SERVER_URL: &str = "https://license.carambaconnect.com";

/// Baked ed25519 public key (base64) used to verify activation signatures.
/// This is the root of trust for license verification against a third party who
/// does not hold the signing key. It is NOT a control against the self-hoster,
/// who owns this value and the whole .env.
///
/// Replace with the real platform owner public key before shipping. While this
/// is empty, every fresh install is unverifiable and fails safe to the Free
/// tier. Operators may override it per install, but that is an advanced option
/// and weakens the trust model.
pub const DEFAULT_LICENSE_PUBKEY: &str = "";

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
    /// License key for this instance. Empty means a Free instance.
    pub license_key: Option<String>,
    /// License server URL. Defaults to the platform owner URL when not set.
    pub license_server_url: String,
    /// ed25519 public key (base64) used to verify activation signatures.
    pub license_pubkey: String,
    /// Адреса обратных прокси перед панелью (например, релей поддомена подписок),
    /// чьему X-Forwarded-For Caddy должен верить. Без этого Caddy выбрасывает
    /// заголовок недоверенного прокси и подставляет адрес самого прокси — а он
    /// числится инфраструктурой, и учёт устройств в панели слепнет.
    pub trusted_proxies: Vec<String>,
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
    license_key: Option<String>,
    license_server_url: Option<String>,
    license_pubkey: Option<String>,
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
    let domain = env.get("PANEL_URL").and_then(|v| normalize_domain_like(v));
    let admin_path = env.get("ADMIN_PATH").cloned().filter(|v| !v.is_empty());
    let db_pass = env
        .get("DATABASE_URL")
        .and_then(|v| parse_db_password_from_url(v));
    let license_key = env
        .get("CARAMBA_LICENSE_KEY")
        .cloned()
        .filter(|v| !v.trim().is_empty());
    let license_server_url = env
        .get("CARAMBA_LICENSE_SERVER_URL")
        .cloned()
        .filter(|v| !v.trim().is_empty());
    let license_pubkey = env
        .get("CARAMBA_LICENSE_PUBKEY")
        .cloned()
        .filter(|v| !v.trim().is_empty());

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
        license_key,
        license_server_url,
        license_pubkey,
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

/// Prompt for an optional license key. Blank input means a Free instance.
fn get_or_prompt_license_key(value: Option<String>) -> Option<String> {
    if let Some(v) = value {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
        return None;
    }

    let theme = ColorfulTheme::default();
    let raw = Input::<String>::with_theme(&theme)
        .with_prompt("License key (leave blank for Free)")
        .allow_empty(true)
        .default(String::new())
        .interact_text()
        .unwrap_or_default();
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[allow(clippy::too_many_arguments)]
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
    license_key: Option<String>,
    license_server_url: Option<String>,
    license_pubkey: Option<String>,
    trusted_proxies: Option<String>,
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

    // License key: flag wins, then existing .env value, then prompt. Blank is
    // allowed and means a Free instance.
    let license_key = if license_key.is_some() {
        get_or_prompt_license_key(license_key)
    } else {
        get_or_prompt_license_key(existing.license_key.clone())
    };

    // Server URL and pubkey: flag wins, then existing .env value, then the
    // baked defaults. Both are overridable but not prompted for in the normal
    // flow to keep the install simple.
    let license_server_url = license_server_url
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or(existing.license_server_url.clone())
        .unwrap_or_else(|| DEFAULT_LICENSE_SERVER_URL.to_string());

    let license_pubkey = license_pubkey
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or(existing.license_pubkey.clone())
        .unwrap_or_else(|| DEFAULT_LICENSE_PUBKEY.to_string());

    // Список через запятую, флаг или переменная окружения; в обычном потоке не
    // спрашивается, как и параметры лицензии.
    let trusted_proxies = parse_trusted_proxies(trusted_proxies.as_deref());
    Ok(InstallConfig {
        domain,
        sub_domain,
        admin_path,
        install_dir,
        db_pass,
        admin_username,
        admin_password,
        hub_bot_token,
        license_key,
        license_server_url,
        license_pubkey,
        trusted_proxies,
    })
}

/// Разбирает список прокси через запятую, отбрасывая пустые элементы. Пробелы
/// вокруг адресов допускаются — так удобнее задавать значение из переменной.
pub fn parse_trusted_proxies(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

/// Глобальный блок Caddyfile. Пустой, если доверенных прокси нет: Caddy тогда
/// сохраняет поведение по умолчанию и не верит ничьему X-Forwarded-For.
fn caddy_global_options(trusted_proxies: &[String]) -> String {
    if trusted_proxies.is_empty() {
        return String::new();
    }
    format!(
        "# Обратные прокси перед панелью (релей поддомена подписок и т.п.). Их\n\
         # X-Forwarded-For принимается как есть — иначе Caddy подставил бы адрес\n\
         # самого прокси, и учёт устройств в панели видел бы только его.\n\
         {{\n    servers {{\n        trusted_proxies static {}\n    }}\n}}\n\n",
        trusted_proxies.join(" ")
    )
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
    let mut caddyfile = caddy_global_options(&config.trusted_proxies);
    caddyfile.push_str(&format!(
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
    ));

    if has_external_sub_domain {
        if let Some(sub) = &config.sub_domain {
            caddyfile.push_str(&format!(
                "\n{sub} {{\n    encode zstd gzip\n\n    handle {{\n        reverse_proxy 127.0.0.1:8080\n    }}\n}}\n"
            ));
        }
    }

    caddyfile
}

#[cfg(test)]
mod caddyfile_tests {
    use super::*;

    fn config(trusted_proxies: Vec<String>) -> InstallConfig {
        InstallConfig {
            domain: "panel.example.test".into(),
            sub_domain: Some("app.example.test".into()),
            admin_path: "/admin".into(),
            install_dir: "/opt/caramba".into(),
            db_pass: "x".into(),
            admin_username: "admin".into(),
            admin_password: "x".into(),
            hub_bot_token: None,
            license_key: None,
            license_server_url: String::new(),
            license_pubkey: String::new(),
            trusted_proxies,
        }
    }

    #[test]
    fn no_trusted_proxies_means_no_global_block() {
        let out = generate_caddyfile(&config(vec![]));
        assert!(!out.contains("trusted_proxies"));
        assert!(out.starts_with("panel.example.test {"));
    }

    #[test]
    fn trusted_proxies_render_as_a_global_block_before_sites() {
        let out = generate_caddyfile(&config(vec!["141.98.191.214/32".into(), "10.0.0.5".into()]));
        let global = out
            .find("trusted_proxies static 141.98.191.214/32 10.0.0.5")
            .unwrap();
        let site = out.find("panel.example.test {").unwrap();
        assert!(global < site, "глобальный блок обязан идти первым");
        assert!(out.contains("app.example.test {"));
    }

    #[test]
    fn parse_trims_and_drops_empty_entries() {
        assert_eq!(
            parse_trusted_proxies(Some(" 1.2.3.4 ,, 5.6.7.8/32 ,")),
            vec!["1.2.3.4".to_string(), "5.6.7.8/32".to_string()]
        );
        assert!(parse_trusted_proxies(None).is_empty());
        assert!(parse_trusted_proxies(Some("  ")).is_empty());
    }
}
